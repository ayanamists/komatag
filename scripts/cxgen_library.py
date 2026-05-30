#!/usr/bin/env python3
"""Drive cxgen across a whole manga library.

Two phases, kept separate on purpose so a human can review the risky middle step
(matching each series to the correct Bangumi subject) before anything is written:

  resolve  walk the library, query Bangumi per series, write an editable map
           (series dir -> bangumi id). Auto-picks the series head; flags the
           ambiguous ones (no series head, or name mismatch) as needs_review.

  apply    read the (reviewed) map and inject ComicInfo.xml into every volume,
           deriving the volume number from the inner filename.

The map is plain JSON: open it, fix any `needs_review` entry's `bangumi_id`,
then run `apply`. Nothing touches an archive until `apply --no-dry-run`.

Usage:
  cxgen_library.py resolve --root /data/manga --out series-map.json
  cxgen_library.py apply   --root /data/manga --map series-map.json          # dry-run
  cxgen_library.py apply   --root /data/manga --map series-map.json --no-dry-run
"""

import argparse
import json
import os
import re
import subprocess
import sys
import time

# cxgen binary: env override, else the release build next to this repo.
CXGEN = os.environ.get(
    "CXGEN",
    os.path.join(os.path.dirname(__file__), "..", "target", "release", "cxgen"),
)

# A search result line printed by `cxgen search`:
#   [208146] 五等分の花嫁 / 五等分的新娘 (2017-10-17) [系列, 漫画]
SEARCH_LINE = re.compile(r"^\[(\d+)\]\s+(.*)$")

# Volume number inside an archive filename: `Wdfdxn_01.cbz`, `第01卷`, `Volume_01`.
VOL_PATTERNS = [
    re.compile(r"_(\d{1,4})\b"),
    re.compile(r"第\s*(\d{1,4})\s*[卷册]"),
    re.compile(r"(?i)\bvol(?:ume)?\.?\s*(\d{1,4})\b"),
    re.compile(r"(?i)\bv(\d{1,4})\b"),
]

# Bracket tags that are not the title or the author (edition/quality markers).
NOISE_TAGS = {"置顶", "台版", "完全版", "爱藏版", "彩页版", "番外", "全彩"}


def parse_dir_name(name):
    """Return (title, author) guessed from a `[title][author][Vol..]` dir name."""
    brackets = re.findall(r"[\[【]([^\]】]+)[\]】]", name)
    brackets = [b.strip() for b in brackets if b.strip()]
    title = brackets[0] if brackets else name.strip()
    author = None
    for b in brackets[1:]:
        if b in NOISE_TAGS or re.search(r"(?i)vol", b) or "卷" in b:
            continue
        author = b
        break
    return title, author


def read_meta_json(series_dir):
    path = os.path.join(series_dir, "meta.json")
    if not os.path.isfile(path):
        return None
    try:
        with open(path, encoding="utf-8") as fh:
            return json.load(fh)
    except (OSError, ValueError):
        return None


def run_cxgen(extra_args, retries=3):
    """Run cxgen, retrying transient network resets. Returns (stdout, stderr, rc)."""
    last = ("", "", 1)
    for attempt in range(retries):
        proc = subprocess.run(
            [CXGEN, *extra_args],
            capture_output=True,
            text=True,
        )
        err = proc.stderr or ""
        if "Connection reset" in err or "error sending request" in err:
            last = (proc.stdout, err, proc.returncode)
            time.sleep(1.5 * (attempt + 1))
            continue
        return proc.stdout, err, proc.returncode
    return last


def search(title, token):
    args = ["search", title]
    if token:
        args += ["--token", token]
    out, _, _ = run_cxgen(args)
    hits = []
    for line in out.splitlines():
        m = SEARCH_LINE.match(line.strip())
        if not m:
            continue
        rest = m.group(2)
        is_series = bool(re.search(r"\[[^\]]*系列[^\]]*\]\s*$", rest))
        hits.append({"id": int(m.group(1)), "label": rest, "series": is_series})
    return hits


def normalize(s):
    return re.sub(r"[\s\-—~～_]", "", s or "").lower()


def choose(hits, title):
    """Pick the best hit; return (id, needs_review, reason)."""
    if not hits:
        return None, True, "no results"
    series_hits = [h for h in hits if h["series"]]
    chosen = series_hits[0] if series_hits else hits[0]
    # Name-mismatch guard (the "NANA -> NaNa" collision): the chosen label should
    # contain the title (or vice versa) once punctuation is stripped.
    nt = normalize(title)
    if nt and nt not in normalize(chosen["label"]):
        return chosen["id"], True, "name mismatch — verify"
    if not series_hits:
        return chosen["id"], True, "no series head — only volumes found"
    # Short / pure-ASCII titles collide easily (NANA vs NaNa, EDEN, DNA2…):
    # the normalized names match but the works don't. Force a human look.
    if len(nt) <= 4 and nt.isascii():
        return chosen["id"], True, "short ambiguous title — verify"
    return chosen["id"], False, "ok"


def cmd_resolve(args):
    root = args.root
    dirs = sorted(
        d for d in os.listdir(root) if os.path.isdir(os.path.join(root, d))
    )
    entries = []
    review = 0
    for name in dirs:
        series_dir = os.path.join(root, name)
        title, author = parse_dir_name(name)
        meta = read_meta_json(series_dir)
        if meta:
            title = meta.get("title") or title
            author = meta.get("author") or author
        hits = search(title, args.token)
        chosen_id, needs, reason = choose(hits, title)
        review += 1 if needs else 0
        entries.append(
            {
                "dir": name,
                "title": title,
                "author": author,
                "bangumi_id": chosen_id,
                "needs_review": needs,
                "reason": reason,
                "candidates": hits[:6],
            }
        )
        flag = "REVIEW" if needs else "ok    "
        print(f"  [{flag}] {title}  ->  {chosen_id}  ({reason})", file=sys.stderr)
        time.sleep(args.delay)

    with open(args.out, "w", encoding="utf-8") as fh:
        json.dump(entries, fh, ensure_ascii=False, indent=2)
    print(
        f"\nWrote {args.out}: {len(entries)} series, {review} need review.\n"
        f"Open it, fix the `bangumi_id` of any needs_review entry, then run `apply`.",
        file=sys.stderr,
    )


def parse_volume(filename):
    stem = os.path.splitext(filename)[0]
    for pat in VOL_PATTERNS:
        m = pat.search(stem)
        if m:
            return int(m.group(1))
    return None


def pick_archives(series_dir):
    """One archive per volume; prefer .cbz over .7z when both exist."""
    by_vol = {}
    for fn in sorted(os.listdir(series_dir)):
        ext = os.path.splitext(fn)[1].lower()
        if ext not in (".cbz", ".zip", ".7z"):
            continue
        vol = parse_volume(fn)
        key = vol if vol is not None else fn
        cur = by_vol.get(key)
        if cur is None or (cur.lower().endswith(".7z") and ext != ".7z"):
            by_vol[key] = fn
    return by_vol


def cmd_apply(args):
    with open(args.map, encoding="utf-8") as fh:
        entries = json.load(fh)

    injected = skipped = failed = 0
    for entry in entries:
        if entry.get("needs_review") and not args.include_review:
            print(f"  skip (needs review): {entry['title']}", file=sys.stderr)
            continue
        bid = entry.get("bangumi_id")
        if not bid:
            continue
        series_dir = os.path.join(args.root, entry["dir"])
        if not os.path.isdir(series_dir):
            continue
        vol_sort = lambda kv: (0, kv[0]) if isinstance(kv[0], int) else (1, str(kv[0]))
        for vol, fn in sorted(pick_archives(series_dir).items(), key=vol_sort):
            path = os.path.join(series_dir, fn)
            gen = ["gen", "--bangumi-id", str(bid), "--manga", "yes-rtl",
                   "--language", "zh-Hans"]
            if isinstance(vol, int):
                gen += ["--volume", str(vol)]
            if args.token:
                gen += ["--bangumi-token", args.token]
            if not path.lower().endswith(".7z"):
                gen += ["--inject", "--force"]
            gen.append(path)
            if args.dry_run:
                print("  DRY  " + " ".join(gen[:-1]) + f" {fn!r}")
                continue
            _, err, rc = run_cxgen(gen)
            if rc == 0:
                injected += 1
            else:
                failed += 1
                print(f"  FAIL {fn}: {err.strip().splitlines()[-1:]}", file=sys.stderr)
    print(
        f"\n{'(dry-run) ' if args.dry_run else ''}injected={injected} "
        f"failed={failed}",
        file=sys.stderr,
    )


def main():
    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--token", default=os.environ.get("BANGUMI_TOKEN"),
                   help="Bangumi token [env: BANGUMI_TOKEN]")
    sub = p.add_subparsers(dest="cmd", required=True)

    r = sub.add_parser("resolve", help="build the series -> bangumi_id map")
    r.add_argument("--root", required=True)
    r.add_argument("--out", default="series-map.json")
    r.add_argument("--delay", type=float, default=0.5,
                   help="seconds between searches (rate-limit politeness)")
    r.set_defaults(func=cmd_resolve)

    a = sub.add_parser("apply", help="inject ComicInfo.xml into every volume")
    a.add_argument("--root", required=True)
    a.add_argument("--map", default="series-map.json")
    a.add_argument("--no-dry-run", dest="dry_run", action="store_false",
                   help="actually inject (default is a dry run)")
    a.add_argument("--include-review", action="store_true",
                   help="also process entries still flagged needs_review")
    a.set_defaults(func=cmd_apply, dry_run=True)

    args = p.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
