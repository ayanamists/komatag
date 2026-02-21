//! `cxgen` – generate ComicInfo.xml for comic archives.
//!
//! # Usage
//!
//! ```text
//! cxgen [OPTIONS] <FILE>
//! ```
//!
//! See `cxgen --help` for full option list.

mod archive;
mod bangumi;
mod comic_info;
mod filename_parser;

use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};

use archive::ArchiveFormat;
use comic_info::{AgeRating, ComicInfo, Manga, PageInfo, PageType, YesNo};
use filename_parser::parse_filename;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

/// Generate ComicInfo.xml for a comic archive (.zip, .cbz, .7z).
///
/// Metadata is assembled from three sources (in priority order):
///   1. Explicit CLI flags (--series, --number, …)
///   2. Bangumi API (--bangumi-id or --bangumi-search)
///   3. Heuristic filename parsing
///
/// By default the XML is printed to stdout.  Use -o to write a file or
/// --inject to embed it directly inside a ZIP/CBZ archive.
#[derive(Parser, Debug)]
#[command(name = "cxgen", version, about, long_about = None)]
struct Cli {
    /// Comic archive file to process (.zip, .cbz, .7z).
    #[arg(value_name = "FILE")]
    file: PathBuf,

    // -----------------------------------------------------------------------
    // Output
    // -----------------------------------------------------------------------
    /// Write the generated XML to FILE instead of stdout.
    #[arg(short = 'o', long, value_name = "FILE")]
    output: Option<PathBuf>,

    /// Inject ComicInfo.xml directly into the archive (ZIP/CBZ only).
    #[arg(short = 'i', long)]
    inject: bool,

    /// Overwrite an existing ComicInfo.xml inside the archive (requires --inject).
    #[arg(short = 'f', long)]
    force: bool,

    // -----------------------------------------------------------------------
    // Metadata overrides
    // -----------------------------------------------------------------------
    /// Override the series name.
    #[arg(long)]
    series: Option<String>,

    /// Override the issue number (string, e.g. "42" or "42.1").
    #[arg(long)]
    number: Option<String>,

    /// Override the volume number.
    #[arg(long)]
    volume: Option<i32>,

    /// Override the title.
    #[arg(long)]
    title: Option<String>,

    /// Override the release year.
    #[arg(long)]
    year: Option<i32>,

    /// Override the release month (1-12).
    #[arg(long)]
    month: Option<i32>,

    /// Override the publisher.
    #[arg(long)]
    publisher: Option<String>,

    /// Set the language ISO code (IETF BCP 47 recommended, e.g. "en", "ja",
    /// "zh-Hans").
    #[arg(long, default_value = "en")]
    language: String,

    /// Set the manga reading direction.
    #[arg(long, value_enum, default_value_t = MangaArg::Unknown)]
    manga: MangaArg,

    /// Set black-and-white status.
    #[arg(long, value_enum, default_value_t = YesNoArg::Unknown)]
    black_and_white: YesNoArg,

    /// Set the age rating.
    #[arg(long, value_enum, default_value_t = AgeRatingArg::Unknown)]
    age_rating: AgeRatingArg,

    /// Free-text summary / description.
    #[arg(long)]
    summary: Option<String>,

    /// Writer / scenario author (comma-separated for multiple).
    #[arg(long)]
    writer: Option<String>,

    /// Penciller / artist (comma-separated for multiple).
    #[arg(long)]
    penciller: Option<String>,

    /// Translator (comma-separated for multiple).
    #[arg(long)]
    translator: Option<String>,

    /// Genre (comma-separated for multiple).
    #[arg(long)]
    genre: Option<String>,

    // -----------------------------------------------------------------------
    // Bangumi
    // -----------------------------------------------------------------------
    /// Fetch metadata from Bangumi by numeric subject ID.
    #[arg(long, value_name = "ID")]
    bangumi_id: Option<u64>,

    /// Search Bangumi for a title and print matches (does not generate XML).
    #[arg(long, value_name = "QUERY")]
    bangumi_search: Option<String>,

    /// Bangumi access token (or set BANGUMI_TOKEN env var).
    #[arg(long, env = "BANGUMI_TOKEN", value_name = "TOKEN")]
    bangumi_token: Option<String>,

    /// Bangumi subject type filter for --bangumi-search
    /// (1=anime, 2=manga/book, 4=game).  Defaults to 1 (manga).
    #[arg(long, default_value_t = 1)]
    bangumi_type: u8,
}

// ---------------------------------------------------------------------------
// Clap value enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, ValueEnum)]
enum MangaArg {
    Unknown,
    No,
    Yes,
    /// Right-to-left manga reading direction.
    #[value(name = "yes-rtl")]
    YesRtl,
}

impl From<MangaArg> for Manga {
    fn from(a: MangaArg) -> Self {
        match a {
            MangaArg::Unknown => Manga::Unknown,
            MangaArg::No => Manga::No,
            MangaArg::Yes => Manga::Yes,
            MangaArg::YesRtl => Manga::YesAndRightToLeft,
        }
    }
}

#[derive(Debug, Clone, ValueEnum)]
enum YesNoArg {
    Unknown,
    No,
    Yes,
}

impl From<YesNoArg> for YesNo {
    fn from(a: YesNoArg) -> Self {
        match a {
            YesNoArg::Unknown => YesNo::Unknown,
            YesNoArg::No => YesNo::No,
            YesNoArg::Yes => YesNo::Yes,
        }
    }
}

#[derive(Debug, Clone, ValueEnum)]
enum AgeRatingArg {
    Unknown,
    #[value(name = "adults-only")]
    AdultsOnly18,
    #[value(name = "early-childhood")]
    EarlyChildhood,
    #[value(name = "everyone")]
    Everyone,
    #[value(name = "everyone-10plus")]
    Everyone10Plus,
    G,
    #[value(name = "kids-to-adults")]
    KidsToAdults,
    M,
    #[value(name = "ma15plus")]
    Ma15Plus,
    #[value(name = "mature-17plus")]
    Mature17Plus,
    Pg,
    #[value(name = "r18plus")]
    R18Plus,
    #[value(name = "rating-pending")]
    RatingPending,
    Teen,
    #[value(name = "x18plus")]
    X18Plus,
}

impl From<AgeRatingArg> for AgeRating {
    fn from(a: AgeRatingArg) -> Self {
        match a {
            AgeRatingArg::Unknown => AgeRating::Unknown,
            AgeRatingArg::AdultsOnly18 => AgeRating::AdultsOnly18,
            AgeRatingArg::EarlyChildhood => AgeRating::EarlyChildhood,
            AgeRatingArg::Everyone => AgeRating::Everyone,
            AgeRatingArg::Everyone10Plus => AgeRating::Everyone10Plus,
            AgeRatingArg::G => AgeRating::G,
            AgeRatingArg::KidsToAdults => AgeRating::KidsToAdults,
            AgeRatingArg::M => AgeRating::M,
            AgeRatingArg::Ma15Plus => AgeRating::Ma15Plus,
            AgeRatingArg::Mature17Plus => AgeRating::Mature17Plus,
            AgeRatingArg::Pg => AgeRating::Pg,
            AgeRatingArg::R18Plus => AgeRating::R18Plus,
            AgeRatingArg::RatingPending => AgeRating::RatingPending,
            AgeRatingArg::Teen => AgeRating::Teen,
            AgeRatingArg::X18Plus => AgeRating::X18Plus,
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let cli = Cli::parse();

    if let Err(e) = run(cli) {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Core logic
// ---------------------------------------------------------------------------

fn run(cli: Cli) -> Result<()> {
    // ------------------------------------------------------------------
    // 0. Bangumi-only search mode (no XML generated)
    // ------------------------------------------------------------------
    if let Some(ref query) = cli.bangumi_search {
        return run_bangumi_search(query, cli.bangumi_type, cli.bangumi_token.as_deref());
    }

    // ------------------------------------------------------------------
    // 1. Validate the archive path
    // ------------------------------------------------------------------
    let archive_path = &cli.file;

    if !archive_path.exists() {
        bail!("'{}' does not exist", archive_path.display());
    }
    if !archive_path.is_file() {
        bail!("'{}' is not a file", archive_path.display());
    }

    let format = ArchiveFormat::detect(archive_path).with_context(|| {
        format!(
            "'{}' has an unsupported extension; expected .zip, .cbz, or .7z",
            archive_path.display()
        )
    })?;

    // ------------------------------------------------------------------
    // 2. Inspect the archive
    // ------------------------------------------------------------------
    let archive_info = archive::inspect(archive_path)
        .with_context(|| format!("Failed to inspect '{}'", archive_path.display()))?;

    if archive_info.has_comic_info && !cli.force {
        bail!(
            "'{}' already contains ComicInfo.xml.\n\
             Use --force to overwrite, or --inject --force to replace in-archive.",
            archive_path.display()
        );
    }

    // ------------------------------------------------------------------
    // 3. Build ComicInfo from filename heuristics (lowest priority)
    // ------------------------------------------------------------------
    let filename_meta = parse_filename(archive_path);
    let mut comic_info = ComicInfo::default();

    // Apply filename-derived metadata
    comic_info.series = filename_meta.series;
    comic_info.number = filename_meta.number;
    comic_info.volume = filename_meta.volume;
    comic_info.title = filename_meta.title;
    comic_info.year = filename_meta.year;
    comic_info.publisher = filename_meta.publisher;

    // ------------------------------------------------------------------
    // 4. Merge Bangumi metadata (medium priority, overrides filename)
    // ------------------------------------------------------------------
    if let Some(subject_id) = cli.bangumi_id {
        eprintln!("Fetching Bangumi subject {}...", subject_id);

        let client = bangumi::BangumiClient::new(cli.bangumi_token.clone())
            .context("Failed to create Bangumi client")?;

        let bangumi_info = client
            .fetch_subject(subject_id)
            .with_context(|| format!("Failed to fetch Bangumi subject {subject_id}"))?;

        // Bangumi data wins over filename parsing
        let mut merged = bangumi_info;
        merged.merge_from(comic_info);
        comic_info = merged;
    }

    // ------------------------------------------------------------------
    // 5. Apply explicit CLI overrides (highest priority)
    // ------------------------------------------------------------------
    if let Some(s) = cli.series {
        comic_info.series = Some(s);
    }
    if let Some(n) = cli.number {
        comic_info.number = Some(n);
    }
    if let Some(v) = cli.volume {
        comic_info.volume = Some(v);
    }
    if let Some(t) = cli.title {
        comic_info.title = Some(t);
    }
    if let Some(y) = cli.year {
        comic_info.year = Some(y);
    }
    if let Some(m) = cli.month {
        comic_info.month = Some(m);
    }
    if let Some(p) = cli.publisher {
        comic_info.publisher = Some(p);
    }
    if let Some(s) = cli.summary {
        comic_info.summary = Some(s);
    }
    if let Some(w) = cli.writer {
        comic_info.writer = Some(w);
    }
    if let Some(p) = cli.penciller {
        comic_info.penciller = Some(p);
    }
    if let Some(t) = cli.translator {
        comic_info.translator = Some(t);
    }
    if let Some(g) = cli.genre {
        comic_info.genre = Some(g);
    }

    // Enum fields: only apply non-default values from flags
    let manga: Manga = cli.manga.into();
    if manga != Manga::Unknown {
        comic_info.manga = manga;
    }
    let bw: YesNo = cli.black_and_white.into();
    if bw != YesNo::Unknown {
        comic_info.black_and_white = bw;
    }
    let age: AgeRating = cli.age_rating.into();
    if age != AgeRating::Unknown {
        comic_info.age_rating = age;
    }

    // Language is always set (default "en")
    if comic_info.language_iso.is_none() || cli.language != "en" {
        comic_info.language_iso = Some(cli.language.clone());
    }

    // ------------------------------------------------------------------
    // 6. Populate page list from archive image listing
    // ------------------------------------------------------------------
    let image_count = archive_info.images.len();
    comic_info.page_count = image_count as i32;

    if !archive_info.images.is_empty() {
        comic_info.pages = archive_info
            .images
            .iter()
            .enumerate()
            .map(|(idx, img)| {
                let page_type = if idx == 0 {
                    PageType::FrontCover
                } else if idx == image_count - 1 && image_count > 1 {
                    PageType::BackCover
                } else {
                    PageType::Story
                };
                let mut page = PageInfo::new(idx as i32, page_type);
                page.image_size = img.size;
                page
            })
            .collect();
    }

    // Notes: stamp with generator info
    let generator_note = format!(
        "Generated by cxgen v{} from '{}'",
        env!("CARGO_PKG_VERSION"),
        archive_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
    );
    comic_info.notes = Some(match comic_info.notes.take() {
        Some(prev) => format!("{prev}\n{generator_note}"),
        None => generator_note,
    });

    // ------------------------------------------------------------------
    // 7. Serialize to XML
    // ------------------------------------------------------------------
    let xml = comic_info.to_xml();

    // ------------------------------------------------------------------
    // 8. Output
    // ------------------------------------------------------------------
    if cli.inject {
        // Validate --inject is only used with ZIP/CBZ
        if format == ArchiveFormat::SevenZip {
            bail!(
                "--inject is not supported for .7z archives.\n\
                 Use -o to write ComicInfo.xml to a separate file instead."
            );
        }

        archive::inject_zip(archive_path, &xml, cli.force).with_context(|| {
            format!(
                "Failed to inject ComicInfo.xml into '{}'",
                archive_path.display()
            )
        })?;

        eprintln!(
            "ComicInfo.xml injected into '{}'",
            archive_path.display()
        );
    } else if let Some(ref out_path) = cli.output {
        fs::write(out_path, &xml)
            .with_context(|| format!("Failed to write to '{}'", out_path.display()))?;

        eprintln!("ComicInfo.xml written to '{}'", out_path.display());
    } else {
        // Default: stdout
        io::stdout()
            .write_all(xml.as_bytes())
            .context("Failed to write to stdout")?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Bangumi search helper
// ---------------------------------------------------------------------------

fn run_bangumi_search(query: &str, subject_type: u8, token: Option<&str>) -> Result<()> {
    let client = bangumi::BangumiClient::new(token.map(str::to_owned))
        .context("Failed to create Bangumi client")?;

    eprintln!("Searching Bangumi for '{query}' (type={subject_type})...\n");

    let hits = client
        .search(query, Some(subject_type), 10)
        .context("Bangumi search failed")?;

    if hits.is_empty() {
        eprintln!("No results found.");
        return Ok(());
    }

    for hit in &hits {
        println!("{hit}");
    }

    eprintln!(
        "\nTo use a result, run: cxgen --bangumi-id <ID> <archive>"
    );

    Ok(())
}
