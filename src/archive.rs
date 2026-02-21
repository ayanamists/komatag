//! Archive reading and writing for .zip / .cbz / .7z comic files.
//!
//! # Reading
//! - Counts image pages, returning a list of sorted [`ImageEntry`]s.
//! - Checks whether a `ComicInfo.xml` is already present.
//!
//! # Writing (injection)
//! - ZIP / CBZ: copies all entries into a temp file, appends `ComicInfo.xml`,
//!   then atomically replaces the original.
//! - 7Z: not supported for in-place injection; callers should write the XML
//!   alongside the archive or to stdout instead.

use std::{
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// File name used inside archives for the metadata file.
pub const COMIC_INFO_FILENAME: &str = "ComicInfo.xml";

/// File extensions that are treated as comic image pages (case-insensitive).
const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "bmp", "tiff", "tif", "avif",
];

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Format of the archive on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,  // .zip or .cbz
    SevenZip, // .7z
}

impl ArchiveFormat {
    /// Detect the archive format from the file extension.
    pub fn detect(path: &Path) -> Option<Self> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase());

        match ext.as_deref() {
            Some("zip") | Some("cbz") => Some(ArchiveFormat::Zip),
            Some("7z") => Some(ArchiveFormat::SevenZip),
            _ => None,
        }
    }
}

/// A single image entry found inside an archive.
#[derive(Debug, Clone)]
pub struct ImageEntry {
    /// Path as stored inside the archive (e.g. `"001.jpg"`).
    pub name: String,
    /// Compressed or stored size in bytes (0 if unknown).
    pub size: u64,
}

/// Information extracted from an archive without fully decompressing images.
#[derive(Debug)]
pub struct ArchiveInfo {
    /// Detected format.
    pub format: ArchiveFormat,
    /// Image pages found, sorted by their in-archive name.
    pub images: Vec<ImageEntry>,
    /// `true` when the archive already contains a `ComicInfo.xml`.
    pub has_comic_info: bool,
}

impl ArchiveInfo {
    /// Convenience: total page count.
    pub fn page_count(&self) -> usize {
        self.images.len()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_image(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    // Ignore directory entries and macOS resource forks
    if lower.ends_with('/') || lower.contains("__macosx") {
        return false;
    }
    if let Some(ext) = lower.rsplit('.').next() {
        return IMAGE_EXTENSIONS.contains(&ext);
    }
    false
}

fn is_comic_info(name: &str) -> bool {
    // Accept any path depth, e.g. "ComicInfo.xml" or "sub/ComicInfo.xml"
    name.to_ascii_lowercase().ends_with("comicinfo.xml")
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// Inspect a ZIP/CBZ archive and return metadata without decompressing images.
pub fn inspect_zip(path: &Path) -> Result<ArchiveInfo> {
    let file = File::open(path)
        .with_context(|| format!("Cannot open '{}'", path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("'{}' is not a valid ZIP archive", path.display()))?;

    let mut images: Vec<ImageEntry> = Vec::new();
    let mut has_comic_info = false;

    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .with_context(|| format!("Cannot read ZIP entry #{i}"))?;

        let name = entry.name().to_owned();

        if is_comic_info(&name) {
            has_comic_info = true;
        } else if is_image(&name) {
            images.push(ImageEntry {
                size: entry.size(),
                name,
            });
        }
    }

    images.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(ArchiveInfo {
        format: ArchiveFormat::Zip,
        images,
        has_comic_info,
    })
}

/// Inspect a 7Z archive and return metadata without decompressing images.
pub fn inspect_7z(path: &Path) -> Result<ArchiveInfo> {
    use sevenz_rust::SevenZReader;

    let reader = SevenZReader::open(path, sevenz_rust::Password::empty())
        .with_context(|| format!("Cannot open '{}' as a 7z archive", path.display()))?;

    let mut images: Vec<ImageEntry> = Vec::new();
    let mut has_comic_info = false;

    // Access the archive metadata directly without decompressing any content.
    for entry in &reader.archive().files {
        let name = entry.name.clone();
        let size = entry.size;

        if is_comic_info(&name) {
            has_comic_info = true;
        } else if is_image(&name) {
            images.push(ImageEntry { name, size });
        }
    }

    images.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(ArchiveInfo {
        format: ArchiveFormat::SevenZip,
        images,
        has_comic_info,
    })
}

/// Inspect any supported archive, auto-detecting the format.
pub fn inspect(path: &Path) -> Result<ArchiveInfo> {
    let fmt = ArchiveFormat::detect(path)
        .with_context(|| format!(
            "'{}' has an unsupported extension; expected .zip, .cbz, or .7z",
            path.display()
        ))?;

    match fmt {
        ArchiveFormat::Zip => inspect_zip(path),
        ArchiveFormat::SevenZip => inspect_7z(path),
    }
}

// ---------------------------------------------------------------------------
// Injection
// ---------------------------------------------------------------------------

/// Inject `xml_content` as `ComicInfo.xml` into a ZIP/CBZ archive.
///
/// The implementation:
/// 1. Opens the original archive.
/// 2. Writes all existing entries (except any pre-existing `ComicInfo.xml`)
///    into a temporary file in the same directory.
/// 3. Appends the new `ComicInfo.xml`.
/// 4. Atomically replaces the original file.
///
/// The temporary file is removed on error to avoid leaving partial files.
pub fn inject_zip(path: &Path, xml_content: &str, force: bool) -> Result<()> {
    use zip::{write::SimpleFileOptions, ZipWriter};

    // Check for existing ComicInfo.xml first
    {
        let f = File::open(path)
            .with_context(|| format!("Cannot open '{}'", path.display()))?;
        let mut archive = zip::ZipArchive::new(f)?;
        for i in 0..archive.len() {
            let entry = archive.by_index(i)?;
            if is_comic_info(entry.name()) {
                if !force {
                    bail!(
                        "'{}' already contains ComicInfo.xml. Use --force to overwrite.",
                        path.display()
                    );
                }
                break;
            }
        }
    }

    // Build temp file path alongside the original
    let tmp_path = tmp_path_for(path);

    let result = (|| -> Result<()> {
        let src_file = File::open(path)
            .with_context(|| format!("Cannot open '{}'", path.display()))?;
        let mut src_archive = zip::ZipArchive::new(src_file)?;

        let dst_file = File::create(&tmp_path)
            .with_context(|| format!("Cannot create temp file '{}'", tmp_path.display()))?;
        let mut dst = ZipWriter::new(dst_file);

        // Copy all existing entries, skipping old ComicInfo.xml
        for i in 0..src_archive.len() {
            let mut entry = src_archive.by_index(i)?;

            if is_comic_info(entry.name()) {
                // drop it; we'll add the new one below
                continue;
            }

            let opts = SimpleFileOptions::default()
                .compression_method(entry.compression())
                .last_modified_time(entry.last_modified().unwrap_or_default());

            let name = entry.name().to_owned();
            dst.start_file(&name, opts)
                .with_context(|| format!("Cannot start ZIP entry '{name}'"))?;

            io::copy(&mut entry, &mut dst)
                .with_context(|| format!("Cannot copy ZIP entry '{name}'"))?;
        }

        // Append the new ComicInfo.xml (stored, no compression needed)
        let opts = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        dst.start_file(COMIC_INFO_FILENAME, opts)?;
        dst.write_all(xml_content.as_bytes())?;

        dst.finish()?;
        Ok(())
    })();

    if result.is_err() {
        // Clean up temp file on failure
        let _ = fs::remove_file(&tmp_path);
        return result;
    }

    // Atomically replace the original
    fs::rename(&tmp_path, path).with_context(|| {
        format!(
            "Cannot replace '{}' with temp file '{}'",
            path.display(),
            tmp_path.display()
        )
    })?;

    Ok(())
}

/// Attempt to inject `ComicInfo.xml` into any supported archive.
///
/// Returns an error for 7Z archives because in-place 7z rewriting is
/// not supported; callers should write the XML to a sidecar file instead.
pub fn inject(path: &Path, xml_content: &str, force: bool) -> Result<()> {
    let fmt = ArchiveFormat::detect(path)
        .with_context(|| format!(
            "'{}' has an unsupported extension; expected .zip, .cbz, or .7z",
            path.display()
        ))?;

    match fmt {
        ArchiveFormat::Zip => inject_zip(path, xml_content, force),
        ArchiveFormat::SevenZip => bail!(
            "In-place injection is not supported for .7z archives.\n\
             Use -o to write ComicInfo.xml to a separate file instead."
        ),
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn tmp_path_for(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let tmp_name = format!(".{}.cxgen_tmp", file_name);
    path.with_file_name(tmp_name)
}
