//! Heuristic filename parser for extracting comic metadata.
//!
//! Handles common naming conventions such as:
//!
//! - `Series Name v01 #005 (2023) (Publisher).cbz`
//! - `[Publisher] Series Name #001 (2023).cbz`
//! - `Series Name - Chapter 001 (2023).cbz`
//! - `Series_Name_001.cbz`
//! - `Series Name 001 (2023).cbz`
//! - `Series Name (2023).cbz`

use regex::Regex;
use std::path::Path;

// ---------------------------------------------------------------------------
// Public output type
// ---------------------------------------------------------------------------

/// Metadata extracted from a filename. All fields are optional because
/// filenames vary enormously in practice.
#[derive(Debug, Default, Clone)]
pub struct FilenameMetadata {
    pub series: Option<String>,
    pub number: Option<String>,
    pub volume: Option<i32>,
    pub year: Option<i32>,
    pub publisher: Option<String>,
    pub title: Option<String>,
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parse as much comic metadata as possible from `path`'s file stem.
///
/// Only the file stem (no directory, no extension) is examined.
pub fn parse_filename(path: &Path) -> FilenameMetadata {
    let stem = match path.file_stem().and_then(|s| s.to_str()) {
        Some(s) => s.to_owned(),
        None => return FilenameMetadata::default(),
    };

    parse_stem(&stem)
}

/// Parse a bare file stem string (no extension).
pub fn parse_stem(stem: &str) -> FilenameMetadata {
    let mut meta = FilenameMetadata::default();

    // Normalise underscores and multiple spaces → single space
    let normalised = stem.replace('_', " ");
    let normalised = collapse_spaces(&normalised);

    // -----------------------------------------------------------------------
    // 1. Strip a leading `[Publisher]` block
    // -----------------------------------------------------------------------
    let (publisher_prefix, rest) = strip_leading_bracket_publisher(&normalised);
    if let Some(p) = publisher_prefix {
        meta.publisher = Some(clean_string(&p));
    }

    // -----------------------------------------------------------------------
    // 2. Collect trailing parenthesised tokens (year, publisher, misc)
    // -----------------------------------------------------------------------
    let (core, trailing) = strip_trailing_parens(&rest);
    for token in &trailing {
        if meta.year.is_none() {
            if let Some(y) = parse_year(token) {
                meta.year = Some(y);
                continue;
            }
        }
        // Second parenthesised token that isn't a year → publisher
        if meta.publisher.is_none() && !token.is_empty() && !looks_like_misc(token) {
            meta.publisher = Some(clean_string(token));
        }
    }

    // -----------------------------------------------------------------------
    // 3. Extract volume (`v01`, `vol.2`, `Vol 3`, `Volume 4`)
    // -----------------------------------------------------------------------
    let (core, volume) = extract_volume(&core);
    if let Some(v) = volume {
        meta.volume = Some(v);
    }

    // -----------------------------------------------------------------------
    // 4. Extract issue number (`#005`, `No.3`, `- 007 -`, or bare trailing
    //    digits not part of the series name)
    // -----------------------------------------------------------------------
    let (series_part, number) = extract_issue_number(&core);
    if let Some(n) = number {
        meta.number = Some(n);
    }

    // -----------------------------------------------------------------------
    // 5. Remaining text is the series name
    // -----------------------------------------------------------------------
    let series = clean_string(&series_part);
    if !series.is_empty() {
        meta.series = Some(series.clone());
        // If no explicit title was set, use series + number as title
        if let Some(ref num) = meta.number {
            meta.title = Some(format!("{} #{}", series, num));
        } else if let Some(ref vol) = meta.volume {
            meta.title = Some(format!("{} v{}", series, vol));
        } else {
            meta.title = Some(series);
        }
    }

    meta
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn collapse_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch == ' ' {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out.trim().to_owned()
}

/// Strip a leading `[something]` token; returns `(Some(publisher), rest)`.
fn strip_leading_bracket_publisher(s: &str) -> (Option<String>, String) {
    if let Some(rest) = s.strip_prefix('[') {
        if let Some(close) = rest.find(']') {
            let publisher = rest[..close].trim().to_owned();
            let remainder = rest[close + 1..].trim().to_owned();
            if !publisher.is_empty() {
                return (Some(publisher), remainder);
            }
        }
    }
    (None, s.to_owned())
}

/// Strip zero or more trailing `(token)` groups.
///
/// Returns `(core, tokens_in_order)`.
fn strip_trailing_parens(s: &str) -> (String, Vec<String>) {
    let mut tokens: Vec<String> = Vec::new();
    let mut working = s.trim().to_owned();

    loop {
        let trimmed = working.trim_end();
        if !trimmed.ends_with(')') {
            break;
        }
        // Find the matching opening paren (scan right-to-left)
        let bytes = trimmed.as_bytes();
        let mut depth = 0usize;
        let mut open_pos = None;
        for (i, &b) in bytes.iter().enumerate().rev() {
            match b {
                b')' => depth += 1,
                b'(' => {
                    depth -= 1;
                    if depth == 0 {
                        open_pos = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        match open_pos {
            None => break,
            Some(pos) => {
                let inner = trimmed[pos + 1..trimmed.len() - 1].trim().to_owned();
                tokens.push(inner);
                working = trimmed[..pos].trim().to_owned();
            }
        }
    }

    // tokens were collected right-to-left; reverse so they read left-to-right
    tokens.reverse();
    (working, tokens)
}

/// Try to parse a four-digit year in [1800, 2100].
fn parse_year(s: &str) -> Option<i32> {
    let s = s.trim();
    if s.len() == 4 {
        if let Ok(y) = s.parse::<i32>() {
            if (1800..=2100).contains(&y) {
                return Some(y);
            }
        }
    }
    None
}

fn looks_like_misc(s: &str) -> bool {
    // Tokens that look like scan tags, digital, etc. – don't use as publisher
    let lower = s.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "digital"
            | "hq"
            | "scan"
            | "scans"
            | "webrip"
            | "web"
            | "cbz"
            | "cbr"
            | "epub"
            | "pdf"
    ) || lower.starts_with("v")
        && lower[1..].chars().all(|c| c.is_ascii_digit())
}

/// Extract volume from the core string.
///
/// Recognises: `v01`, `v.01`, `vol01`, `vol.01`, `vol 01`, `volume 01`
/// Returns `(core_without_volume, Some(volume_number))`.
fn extract_volume(s: &str) -> (String, Option<i32>) {
    // Patterns (case-insensitive):
    //   \bvol(?:ume)?\.?\s*(\d+)\b   – "Volume 3", "vol.2", "Vol 01"
    //   \bv(\d+)\b                    – "v01", "v2"
    static RE_VOLUME_LONG: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static RE_VOLUME_SHORT: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();

    let re_long = RE_VOLUME_LONG
        .get_or_init(|| Regex::new(r"(?i)\bvol(?:ume)?\.?\s*(\d+)\b").unwrap());
    let re_short =
        RE_VOLUME_SHORT.get_or_init(|| Regex::new(r"(?i)\bv(\d{1,4})\b").unwrap());

    for re in [re_long, re_short] {
        if let Some(cap) = re.captures(s) {
            if let Ok(vol) = cap[1].parse::<i32>() {
                let cleaned = re.replace(s, "").trim().to_owned();
                let cleaned = collapse_spaces(&cleaned.trim_matches('-').trim().to_owned());
                return (cleaned, Some(vol));
            }
        }
    }

    (s.to_owned(), None)
}

/// Extract the issue/chapter number from the core string.
///
/// Priority:
/// 1. `#\d+` anywhere in the string
/// 2. `No.\d+` / `No \d+`
/// 3. `Chapter \d+` / `Ch.\d+` / `Ch \d+`
/// 4. A bare run of digits at the end of the string
///
/// Returns `(series_text, Some(issue_number_string))`.
fn extract_issue_number(s: &str) -> (String, Option<String>) {
    // Pattern 1: hash notation  #001
    static RE_HASH: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    // Pattern 2: No. / No notation
    static RE_NO: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    // Pattern 3: Chapter / Ch.
    static RE_CHAPTER: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    // Pattern 4: trailing digits (separated by space or dash)
    static RE_TRAILING: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();

    let re_hash = RE_HASH.get_or_init(|| Regex::new(r"#(\d+(?:\.\d+)?)").unwrap());
    let re_no =
        RE_NO.get_or_init(|| Regex::new(r"(?i)\bno\.?\s*(\d+(?:\.\d+)?)\b").unwrap());
    let re_chapter = RE_CHAPTER
        .get_or_init(|| Regex::new(r"(?i)\b(?:chapter|ch)\.?\s*(\d+(?:\.\d+)?)\b").unwrap());
    let re_trailing =
        RE_TRAILING.get_or_init(|| Regex::new(r"[\s\-]+(\d{1,4})$").unwrap());

    for re in [re_hash, re_no, re_chapter, re_trailing] {
        if let Some(cap) = re.captures(s) {
            let num = cap[1].to_owned();
            let cleaned = re.replace(s, "").trim().to_owned();
            let cleaned = collapse_spaces(&cleaned.trim_matches('-').trim().to_owned());
            return (cleaned, Some(num));
        }
    }

    (s.to_owned(), None)
}

fn clean_string(s: &str) -> String {
    // Remove stray leading/trailing punctuation: - . ,
    let trimmed = s.trim_matches(|c: char| c == '-' || c == '.' || c == ',' || c == ' ');
    collapse_spaces(&trimmed.to_owned())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> FilenameMetadata {
        parse_stem(s)
    }

    #[test]
    fn series_volume_issue_year_publisher() {
        let m = parse("My Series v02 #015 (2022) (Awesome Pub)");
        assert_eq!(m.series.as_deref(), Some("My Series"));
        assert_eq!(m.volume, Some(2));
        assert_eq!(m.number.as_deref(), Some("015"));
        assert_eq!(m.year, Some(2022));
        assert_eq!(m.publisher.as_deref(), Some("Awesome Pub"));
    }

    #[test]
    fn leading_bracket_publisher() {
        let m = parse("[Dark Horse] Hellboy #001 (1994)");
        assert_eq!(m.publisher.as_deref(), Some("Dark Horse"));
        assert_eq!(m.series.as_deref(), Some("Hellboy"));
        assert_eq!(m.number.as_deref(), Some("001"));
        assert_eq!(m.year, Some(1994));
    }

    #[test]
    fn underscore_separated() {
        let m = parse("Comic_Series_Name_042");
        assert_eq!(m.series.as_deref(), Some("Comic Series Name"));
        assert_eq!(m.number.as_deref(), Some("042"));
    }

    #[test]
    fn chapter_notation() {
        let m = parse("Manga Title - Chapter 007 (2021)");
        assert_eq!(m.series.as_deref(), Some("Manga Title"));
        assert_eq!(m.number.as_deref(), Some("007"));
        assert_eq!(m.year, Some(2021));
    }

    #[test]
    fn ch_dot_notation() {
        let m = parse("Some Manga Ch.023");
        assert_eq!(m.series.as_deref(), Some("Some Manga"));
        assert_eq!(m.number.as_deref(), Some("023"));
    }

    #[test]
    fn volume_only() {
        let m = parse("Big Series Volume 3 (2019)");
        assert_eq!(m.series.as_deref(), Some("Big Series"));
        assert_eq!(m.volume, Some(3));
        assert_eq!(m.year, Some(2019));
        assert!(m.number.is_none());
    }

    #[test]
    fn year_only() {
        let m = parse("Stand Alone Comic (2020)");
        assert_eq!(m.series.as_deref(), Some("Stand Alone Comic"));
        assert_eq!(m.year, Some(2020));
        assert!(m.number.is_none());
        assert!(m.volume.is_none());
    }

    #[test]
    fn no_metadata_at_all() {
        let m = parse("Just A Title");
        assert_eq!(m.series.as_deref(), Some("Just A Title"));
        assert!(m.number.is_none());
        assert!(m.year.is_none());
        assert!(m.volume.is_none());
        assert!(m.publisher.is_none());
    }

    #[test]
    fn title_generated_with_number() {
        let m = parse("Amazing Spider-Man #050 (1968)");
        assert_eq!(m.series.as_deref(), Some("Amazing Spider-Man"));
        assert_eq!(m.number.as_deref(), Some("050"));
        assert_eq!(m.year, Some(1968));
        assert_eq!(m.title.as_deref(), Some("Amazing Spider-Man #050"));
    }

    #[test]
    fn path_input() {
        use std::path::PathBuf;
        let p = PathBuf::from("/comics/My_Comic_v01_#003_(2021)_(Publisher).cbz");
        let m = parse_filename(&p);
        assert_eq!(m.series.as_deref(), Some("My Comic"));
        assert_eq!(m.volume, Some(1));
        assert_eq!(m.number.as_deref(), Some("003"));
        assert_eq!(m.year, Some(2021));
        assert_eq!(m.publisher.as_deref(), Some("Publisher"));
    }
}
