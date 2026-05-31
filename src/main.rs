//! `komatag` – generate ComicInfo.xml for comic archives.
//!
//! # Usage
//!
//! ```text
//! komatag [OPTIONS] <FILE_OR_DIR>
//! ```
//!
//! See `komatag --help` for full option list.

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
use clap::{Args, Parser, ValueEnum};

use archive::ArchiveFormat;
use comic_info::{AgeRating, ComicInfo, Manga, PageInfo, PageType, YesNo};
use filename_parser::parse_filename;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

/// Generate ComicInfo.xml for a comic archive (.zip, .cbz, .7z) or a directory of archives.
///
/// Metadata is assembled from three sources (in priority order):
///   1. Explicit CLI flags (--series, --number, …)
///   2. Bangumi API (--bangumi-id or --bangumi-search)
///   3. Heuristic filename parsing
///
/// By default the XML is printed to stdout.  Use -o to write a file or
/// --inject to embed it directly inside a ZIP/CBZ archive.
#[derive(Parser, Debug)]
#[command(name = "komatag", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand, Debug)]
enum Command {
    /// Generate ComicInfo.xml for an archive or a directory of archives.
    Gen(GenArgs),

    /// Search Bangumi and print matching subjects.
    ///
    /// Use the printed `[id]` with `gen --bangumi-id <ID>`. The `[系列]` tag
    /// marks the series head (vs. an individual volume).
    Search(SearchArgs),
}

/// Arguments for the `gen` subcommand.
#[derive(Args, Debug)]
struct GenArgs {
    /// Comic archive file or directory to process (.zip, .cbz, .7z).
    #[arg(value_name = "FILE_OR_DIR")]
    file: PathBuf,

    #[command(flatten)]
    output: OutputArgs,

    #[command(flatten)]
    overrides: OverrideArgs,

    #[command(flatten)]
    bangumi: BangumiArgs,
}

/// Arguments for the `search` subcommand.
#[derive(Args, Debug)]
struct SearchArgs {
    /// Title or keyword to search for.
    #[arg(value_name = "QUERY")]
    query: String,

    /// Subject type filter (1=book/manga, 2=anime, 3=music, 4=game, 6=real).
    #[arg(long = "type", value_name = "TYPE", default_value_t = 1)]
    subject_type: u8,

    /// Bangumi access token (or set BANGUMI_TOKEN env var).
    #[arg(long, env = "BANGUMI_TOKEN", value_name = "TOKEN")]
    token: Option<String>,
}

/// Where the generated XML goes.
#[derive(Args, Debug)]
#[command(next_help_heading = "Output")]
struct OutputArgs {
    /// Write the generated XML to FILE instead of stdout.
    #[arg(short = 'o', long, value_name = "FILE")]
    output: Option<PathBuf>,

    /// Inject ComicInfo.xml directly into the archive (ZIP/CBZ only).
    #[arg(short = 'i', long)]
    inject: bool,

    /// Overwrite an existing ComicInfo.xml inside the archive (requires --inject).
    #[arg(short = 'f', long)]
    force: bool,
}

/// Explicit metadata overrides (highest priority — they win over Bangumi and
/// filename parsing).
#[derive(Args, Debug)]
#[command(next_help_heading = "Metadata overrides")]
struct OverrideArgs {
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
}

impl OverrideArgs {
    /// Apply the explicit overrides onto `ci`. Only fields the user actually
    /// set (or set to a non-default enum value) are touched, so this can run
    /// unconditionally as the final, highest-priority merge step.
    fn apply(&self, ci: &mut ComicInfo) {
        macro_rules! set_str {
            ($field:ident) => {
                if let Some(v) = &self.$field {
                    ci.$field = Some(v.clone());
                }
            };
        }
        set_str!(series);
        set_str!(number);
        set_str!(title);
        set_str!(publisher);
        set_str!(summary);
        set_str!(writer);
        set_str!(penciller);
        set_str!(translator);
        set_str!(genre);

        if let Some(v) = self.volume {
            ci.volume = Some(v);
        }
        if let Some(y) = self.year {
            ci.year = Some(y);
        }
        if let Some(m) = self.month {
            ci.month = Some(m);
        }

        // Enum fields: only apply non-default (explicitly chosen) values.
        let manga: Manga = self.manga.clone().into();
        if manga != Manga::Unknown {
            ci.manga = manga;
        }
        let bw: YesNo = self.black_and_white.clone().into();
        if bw != YesNo::Unknown {
            ci.black_and_white = bw;
        }
        let age: AgeRating = self.age_rating.clone().into();
        if age != AgeRating::Unknown {
            ci.age_rating = age;
        }

        // Language always carries a value (default "en"): honour an explicit
        // non-default choice, otherwise only fill when nothing set it upstream.
        if ci.language_iso.is_none() || self.language != "en" {
            ci.language_iso = Some(self.language.clone());
        }
    }
}

/// Bangumi (bgm.tv) lookup options.
#[derive(Args, Debug)]
#[command(next_help_heading = "Bangumi")]
struct BangumiArgs {
    /// Automatically search Bangumi using the parsed series name and use the
    /// best match (prefers the series head over individual volumes).
    #[arg(long = "bangumi-auto")]
    auto: bool,

    /// Fetch metadata from Bangumi by numeric subject ID.
    #[arg(long = "bangumi-id", value_name = "ID")]
    id: Option<u64>,

    /// Bangumi access token (or set BANGUMI_TOKEN env var).
    #[arg(long = "bangumi-token", env = "BANGUMI_TOKEN", value_name = "TOKEN")]
    token: Option<String>,

    /// Bangumi subject type filter for --bangumi-search
    /// (1=book/manga, 2=anime, 3=music, 4=game, 6=real).
    /// Defaults to 1 (book/manga).
    #[arg(long = "bangumi-type", value_name = "TYPE", default_value_t = 1)]
    subject_type: u8,
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
    match cli.command {
        Command::Search(args) => {
            run_bangumi_search(&args.query, args.subject_type, args.token.as_deref())
        }
        Command::Gen(args) => run_gen(&args),
    }
}

/// Run the `gen` subcommand: generate ComicInfo.xml for a file or a directory.
fn run_gen(args: &GenArgs) -> Result<()> {
    let target_path = &args.file;

    if !target_path.exists() {
        bail!("'{}' does not exist", target_path.display());
    }

    if target_path.is_dir() {
        if args.output.output.is_some() {
            bail!("Cannot use --output when processing a directory. Use --inject instead.");
        }
        if !args.output.inject {
            eprintln!("Warning: Processing a directory without --inject will print all generated XMLs to stdout.");
        }

        let mut success_count = 0;
        let mut error_count = 0;
        for entry in fs::read_dir(target_path).context("Failed to read directory")? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                // Skip files with unsupported extensions silently in directory mode
                if archive::ArchiveFormat::detect(&path).is_some() {
                    if let Err(e) = process_file(&path, args) {
                        eprintln!("Error processing '{}': {}", path.display(), e);
                        error_count += 1;
                    } else {
                        success_count += 1;
                    }
                }
            }
        }
        eprintln!(
            "Processed {} files ({} errors).",
            success_count, error_count
        );
        Ok(())
    } else {
        process_file(target_path, args)
    }
}

fn process_file(archive_path: &std::path::Path, cli: &GenArgs) -> Result<()> {
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

    if archive_info.has_comic_info && !cli.output.force {
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
    let mut bangumi_id = cli.bangumi.id;

    // Use a unified Bangumi client if needed
    let bangumi_client = if bangumi_id.is_some() || cli.bangumi.auto {
        bangumi::BangumiClient::new(cli.bangumi.token.clone()).ok()
    } else {
        None
    };

    if bangumi_id.is_none() && cli.bangumi.auto {
        if let Some(ref client) = bangumi_client {
            if let Some(ref series) = comic_info.series {
                eprintln!("Auto-searching Bangumi for '{}'...", series);
                match client.search(series, Some(cli.bangumi.subject_type), 10) {
                    Ok(hits) => {
                        // Prefer the series head (系列主条目) over individual
                        // 单行本 volume entries, which otherwise pollute the top
                        // results (e.g. "課長島耕作 (1)" outranking the series).
                        let chosen = hits
                            .iter()
                            .find(|h| h.series)
                            .or_else(|| hits.first())
                            .cloned();
                        if let Some(hit) = chosen {
                            let mut best_id = hit.id;

                            // If we have a specific volume/number in our local context,
                            // see if this top-level subject has a "单行本" relation matching it.
                            if let Some(vol) = comic_info.volume {
                                eprintln!(
                                    "Found series match: {} (ID: {}). Checking for Volume {}...",
                                    hit.name, hit.id, vol
                                );
                                if let Ok(relations) = client.fetch_relations(hit.id) {
                                    let mut matched_relation = None;
                                    for rel in relations {
                                        if rel.relation == "单行本" {
                                            // Check name for (Vol), e.g. "進撃の巨人 (1)" or "第1卷"
                                            // A simple heuristic: check if the number padded or raw is in the title
                                            let v_str = vol.to_string();
                                            let v_str_pad = format!("{:02}", vol);
                                            if rel.name.contains(&format!("({})", v_str))
                                                || rel.name.contains(&format!("({})", v_str_pad))
                                                || rel.name.contains(&format!(" {} ", v_str))
                                            {
                                                matched_relation = Some(rel.id);
                                                break;
                                            }
                                        }
                                    }
                                    if let Some(rel_id) = matched_relation {
                                        eprintln!("Found specific volume match ID: {}", rel_id);
                                        best_id = rel_id;
                                    } else {
                                        eprintln!("Could not find a specific sub-entry for Volume {}. Using series ID {}.", vol, hit.id);
                                    }
                                }
                            } else {
                                eprintln!("Found match: {} (ID: {})", hit.name, hit.id);
                            }

                            bangumi_id = Some(best_id);
                        } else {
                            eprintln!("No results found for '{}'.", series);
                        }
                    }
                    Err(e) => {
                        eprintln!("Warning: Auto-search failed: {}", e);
                    }
                }
            } else {
                eprintln!("Warning: Could not extract series name for Bangumi auto-search.");
            }
        }
    }

    if let Some(subject_id) = bangumi_id {
        eprintln!("Fetching Bangumi subject {}...", subject_id);

        let client = bangumi_client.unwrap();
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
    cli.overrides.apply(&mut comic_info);

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
        "Generated by komatag v{} from '{}'",
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
    if cli.output.inject {
        // Validate --inject is only used with ZIP/CBZ
        if format == ArchiveFormat::SevenZip {
            bail!(
                "--inject is not supported for .7z archives.\n\
                 Use -o to write ComicInfo.xml to a separate file instead."
            );
        }

        archive::inject_zip(archive_path, &xml, cli.output.force).with_context(|| {
            format!(
                "Failed to inject ComicInfo.xml into '{}'",
                archive_path.display()
            )
        })?;

        eprintln!("ComicInfo.xml injected into '{}'", archive_path.display());
    } else if let Some(ref out_path) = cli.output.output {
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

    eprintln!("\nTo use a result, run: komatag --bangumi-id <ID> <archive>");

    Ok(())
}
