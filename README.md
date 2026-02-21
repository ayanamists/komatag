# cxgen — Comic XML Generator

A CLI tool that generates a `ComicInfo.xml` (v2.0 schema) for comic archives
in `.zip`, `.cbz`, or `.7z` format.

Metadata is assembled from up to three sources, in priority order:

| Priority | Source |
|----------|--------|
| 1 (highest) | Explicit CLI flags (`--series`, `--number`, …) |
| 2 | [Bangumi](https://bgm.tv) API (`--bangumi-id` / `--bangumi-search`) |
| 3 (lowest) | Heuristic filename parsing |

The resulting XML conforms to the [ComicInfo v2.0 schema](comicinfo/schema/v2.0/ComicInfo.xsd).

---

## Contents

- [Quick start](#quick-start)
- [Building](#building)
  - [With Nix (recommended)](#with-nix-recommended)
  - [With Cargo](#with-cargo)
- [Usage](#usage)
  - [Basic examples](#basic-examples)
  - [All options](#all-options)
- [Bangumi integration](#bangumi-integration)
- [Filename parsing](#filename-parsing)
- [ComicInfo.xml schema](#comicinfoxmal-schema)
- [Project layout](#project-layout)

---

## Quick start

```sh
# Print ComicInfo.xml to stdout (metadata inferred from filename)
cxgen "My Series v02 #015 (2022) (Publisher).cbz"

# Inject directly into a CBZ archive
cxgen --inject "My Series v02 #015 (2022) (Publisher).cbz"

# Fetch rich metadata from Bangumi and inject
cxgen --bangumi-id 12345 --inject "One Piece v01 (1997).cbz"

# Write to a sidecar file (useful for .7z archives)
cxgen -o ComicInfo.xml "archive.7z"
```

---

## Building

### With Nix (recommended)

The flake provides a reproducible build, a dev shell, and CI checks.

```sh
# Build the binary
nix build

# Run directly without installing
nix run . -- --help

# Enter the dev shell (includes cargo-edit, cargo-watch, p7zip, unzip)
nix develop

# Run all checks (build + clippy + fmt + tests)
nix flake check
```

> **macOS note:** The Security and SystemConfiguration frameworks are linked
> automatically on Darwin; no manual setup is needed.

### With Cargo

Requires a stable Rust toolchain (1.75+).

```sh
cargo build --release
# Binary: target/release/cxgen
```

No native system libraries are required.  The HTTP client uses `rustls` (pure
Rust TLS), and both `zip` and `sevenz-rust` are pure-Rust implementations.

---

## Usage

### Basic examples

```sh
# ── Output ──────────────────────────────────────────────────────────────────

# Print XML to stdout
cxgen archive.cbz

# Write to a file
cxgen -o ComicInfo.xml archive.cbz

# Inject into a ZIP/CBZ archive (creates a temp file, then replaces original)
cxgen --inject archive.cbz

# Inject and overwrite an existing ComicInfo.xml
cxgen --inject --force archive.cbz

# ── Metadata overrides ───────────────────────────────────────────────────────

cxgen --series "One Piece" --number 1 --volume 1 \
      --publisher "Shueisha" --language ja \
      --manga yes-rtl --inject archive.cbz

# ── Bangumi ──────────────────────────────────────────────────────────────────

# Search for a title (prints IDs, does not generate XML)
cxgen --bangumi-search "ワンピース" archive.cbz

# Fetch by ID and merge with filename-parsed data
cxgen --bangumi-id 950 --inject archive.cbz

# Use an access token for higher rate limits / NSFW subjects
BANGUMI_TOKEN=your_token cxgen --bangumi-id 950 --inject archive.cbz
```

### All options

```
Usage: cxgen [OPTIONS] <FILE>

Arguments:
  <FILE>  Comic archive file to process (.zip, .cbz, .7z)

Output:
  -o, --output <FILE>      Write the generated XML to FILE instead of stdout
  -i, --inject             Inject ComicInfo.xml directly into the archive (ZIP/CBZ only)
  -f, --force              Overwrite an existing ComicInfo.xml inside the archive

Metadata overrides:
      --series <SERIES>            Override the series name
      --number <NUMBER>            Override the issue number (e.g. "42" or "42.1")
      --volume <VOLUME>            Override the volume number
      --title <TITLE>              Override the title
      --year <YEAR>                Override the release year
      --month <MONTH>              Override the release month (1–12)
      --publisher <PUBLISHER>      Override the publisher
      --language <LANG>            Language ISO / BCP 47 code [default: en]
      --manga <MANGA>              [default: unknown] [possible values: unknown, no, yes, yes-rtl]
      --black-and-white <BW>       [default: unknown] [possible values: unknown, no, yes]
      --age-rating <RATING>        [default: unknown] [possible values: unknown, adults-only,
                                     early-childhood, everyone, everyone-10plus, g,
                                     kids-to-adults, m, ma15plus, mature-17plus, pg,
                                     r18plus, rating-pending, teen, x18plus]
      --summary <SUMMARY>          Free-text description
      --writer <WRITER>            Writer / scenario author (comma-separated)
      --penciller <PENCILLER>      Penciller / artist (comma-separated)
      --translator <TRANSLATOR>    Translator (comma-separated)
      --genre <GENRE>              Genre (comma-separated)

Bangumi:
      --bangumi-id <ID>            Fetch metadata from Bangumi by subject ID
      --bangumi-search <QUERY>     Search Bangumi and print matches (no XML generated)
      --bangumi-token <TOKEN>      Access token [env: BANGUMI_TOKEN]
      --bangumi-type <TYPE>        Subject type filter for search [default: 1]
                                     1=anime/manga, 2=book, 4=game

  -h, --help                       Print help
  -V, --version                    Print version
```

---

## Bangumi integration

[Bangumi (bgm.tv)](https://bgm.tv) is a Chinese anime/manga/game database.
`cxgen` can fetch structured metadata from its v0 API and map the fields to
`ComicInfo.xml`.

### Field mapping

| Bangumi field | ComicInfo field |
|---------------|-----------------|
| `name_cn` / `name` | `Series`, `Title` |
| `summary` | `Summary` |
| `date` (YYYY-MM-DD) | `Year`, `Month`, `Day` |
| `volumes` | `Count` |
| `rating.score` ÷ 2 | `CommunityRating` |
| infobox `出版社` | `Publisher` |
| infobox `作者` / `原作` | `Writer` |
| infobox `作画` / `漫画` | `Penciller` |
| infobox `译者` | `Translator` |
| infobox `类型` | `Genre` |
| infobox `连载杂志` | `Imprint` |
| infobox `语言` | `LanguageISO` |
| infobox `ISBN` | `Notes` |
| subject URL | `Web` |

### Authentication

A personal access token is **optional** for public subjects but:
- raises the API rate limit, and
- allows NSFW subjects to appear in search results.

Generate a token at <https://next.bgm.tv/demo/access-token>.

Pass it via the `BANGUMI_TOKEN` environment variable (recommended, keeps the
token out of your shell history) or the `--bangumi-token` flag.

### Workflow example

```sh
# 1. Find the Bangumi subject ID
cxgen --bangumi-search "进击的巨人" my_archive.cbz
#   [400]  進撃の巨人 / 进击的巨人 (2009-09-09)
#   [8431] 進撃の巨人 Before the Fall / ... (2011-08-09)
#   ...

# 2. Fetch and inject
cxgen --bangumi-id 400 --inject "Attack on Titan v01 (2009).cbz"
```

---

## Filename parsing

When no Bangumi ID is provided, `cxgen` attempts to extract metadata directly
from the archive filename.  Recognised patterns include:

| Filename | series | number | volume | year | publisher |
|----------|--------|--------|--------|------|-----------|
| `My Series v02 #015 (2022) (Publisher).cbz` | My Series | 015 | 2 | 2022 | Publisher |
| `[Dark Horse] Hellboy #001 (1994).cbz` | Hellboy | 001 | — | 1994 | Dark Horse |
| `Comic_Series_Name_042.cbz` | Comic Series Name | 042 | — | — | — |
| `Manga Title - Chapter 007 (2021).cbz` | Manga Title | 007 | — | 2021 | — |
| `Some Manga Ch.023.cbz` | Some Manga | 023 | — | — | — |
| `Big Series Volume 3 (2019).cbz` | Big Series | — | 3 | 2019 | — |

Underscores are normalised to spaces.  All patterns are case-insensitive.

---

## ComicInfo.xml schema

The generated XML conforms to **ComicInfo v2.0**.  The schema XSD and full
field documentation live in the bundled submodule:

```
comicinfo/
├── schema/v2.0/ComicInfo.xsd   ← normative schema
└── DOCUMENTATION.md            ← field-by-field reference
```

Key fields generated automatically:

- **`PageCount`** — number of image files found in the archive.
- **`Pages`** — one `<Page>` element per image; the first is marked
  `FrontCover`, the last `BackCover`, and the rest `Story`.
- **`Notes`** — stamped with `Generated by cxgen vX.Y.Z from '<filename>'`.

---

## Project layout

```
comic-xml-generator/
├── api/
│   └── bangumi-openapi.json   Reference: Bangumi API v2026-01-22
├── comicinfo/                 Git submodule: ComicInfo schema & docs
│   ├── schema/v2.0/ComicInfo.xsd
│   └── DOCUMENTATION.md
├── src/
│   ├── main.rs                CLI (clap) + orchestration
│   ├── archive.rs             ZIP / CBZ / 7Z reading and injection
│   ├── comic_info.rs          ComicInfo v2.0 struct + XML serialization
│   ├── filename_parser.rs     Heuristic filename metadata extraction
│   └── bangumi.rs             Bangumi API client + field mapping
├── Cargo.toml
├── flake.nix                  Nix flake (crane, rust-overlay)
└── README.md
```

---

## License

MIT