//! ComicInfo v2.0 data structures and XML serialization.
//!
//! Schema reference: comicinfo/schema/v2.0/ComicInfo.xsd

use std::fmt::Write as FmtWrite;

// ---------------------------------------------------------------------------
// Enum types
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, PartialEq)]
pub enum YesNo {
    #[default]
    Unknown,
    No,
    Yes,
}

impl YesNo {
    pub fn as_str(&self) -> &'static str {
        match self {
            YesNo::Unknown => "Unknown",
            YesNo::No => "No",
            YesNo::Yes => "Yes",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "Yes" | "yes" | "true" => YesNo::Yes,
            "No" | "no" | "false" => YesNo::No,
            _ => YesNo::Unknown,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub enum Manga {
    #[default]
    Unknown,
    No,
    Yes,
    YesAndRightToLeft,
}

impl Manga {
    pub fn as_str(&self) -> &'static str {
        match self {
            Manga::Unknown => "Unknown",
            Manga::No => "No",
            Manga::Yes => "Yes",
            Manga::YesAndRightToLeft => "YesAndRightToLeft",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "Yes" | "yes" => Manga::Yes,
            "No" | "no" => Manga::No,
            "YesAndRightToLeft" | "yes-rtl" | "rtl" => Manga::YesAndRightToLeft,
            _ => Manga::Unknown,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub enum AgeRating {
    #[default]
    Unknown,
    AdultsOnly18,
    EarlyChildhood,
    Everyone,
    Everyone10Plus,
    G,
    KidsToAdults,
    M,
    Ma15Plus,
    Mature17Plus,
    Pg,
    R18Plus,
    RatingPending,
    Teen,
    X18Plus,
}

impl AgeRating {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgeRating::Unknown => "Unknown",
            AgeRating::AdultsOnly18 => "Adults Only 18+",
            AgeRating::EarlyChildhood => "Early Childhood",
            AgeRating::Everyone => "Everyone",
            AgeRating::Everyone10Plus => "Everyone 10+",
            AgeRating::G => "G",
            AgeRating::KidsToAdults => "Kids to Adults",
            AgeRating::M => "M",
            AgeRating::Ma15Plus => "MA15+",
            AgeRating::Mature17Plus => "Mature 17+",
            AgeRating::Pg => "PG",
            AgeRating::R18Plus => "R18+",
            AgeRating::RatingPending => "Rating Pending",
            AgeRating::Teen => "Teen",
            AgeRating::X18Plus => "X18+",
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub enum PageType {
    FrontCover,
    InnerCover,
    Roundup,
    #[default]
    Story,
    Advertisement,
    Editorial,
    Letters,
    Preview,
    BackCover,
    Other,
    Deleted,
}

impl PageType {
    pub fn as_str(&self) -> &'static str {
        match self {
            PageType::FrontCover => "FrontCover",
            PageType::InnerCover => "InnerCover",
            PageType::Roundup => "Roundup",
            PageType::Story => "Story",
            PageType::Advertisement => "Advertisement",
            PageType::Editorial => "Editorial",
            PageType::Letters => "Letters",
            PageType::Preview => "Preview",
            PageType::BackCover => "BackCover",
            PageType::Other => "Other",
            PageType::Deleted => "Deleted",
        }
    }
}

// ---------------------------------------------------------------------------
// Page info
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PageInfo {
    /// 0-based page index.
    pub image: i32,
    pub page_type: PageType,
    pub double_page: bool,
    /// File size in bytes; 0 if unknown.
    pub image_size: u64,
    /// -1 if unknown.
    pub image_width: i32,
    /// -1 if unknown.
    pub image_height: i32,
}

impl PageInfo {
    pub fn new(image: i32, page_type: PageType) -> Self {
        Self {
            image,
            page_type,
            double_page: false,
            image_size: 0,
            image_width: -1,
            image_height: -1,
        }
    }
}

// ---------------------------------------------------------------------------
// Main struct
// ---------------------------------------------------------------------------

/// Full ComicInfo v2.0 document.
///
/// All optional fields default to `None` / their schema defaults.
/// `page_count` is set to 0 by default and should be updated from the archive.
#[derive(Debug, Default, Clone)]
pub struct ComicInfo {
    // Identity
    pub title: Option<String>,
    pub series: Option<String>,
    pub number: Option<String>,
    pub count: Option<i32>,
    pub volume: Option<i32>,

    // Alternate series cross-over fields
    pub alternate_series: Option<String>,
    pub alternate_number: Option<String>,
    pub alternate_count: Option<i32>,

    // Descriptive
    pub summary: Option<String>,
    pub notes: Option<String>,

    // Release date
    pub year: Option<i32>,
    pub month: Option<i32>,
    pub day: Option<i32>,

    // Creators
    pub writer: Option<String>,
    pub penciller: Option<String>,
    pub inker: Option<String>,
    pub colorist: Option<String>,
    pub letterer: Option<String>,
    pub cover_artist: Option<String>,
    pub editor: Option<String>,
    pub translator: Option<String>,

    // Publishing
    pub publisher: Option<String>,
    pub imprint: Option<String>,
    pub genre: Option<String>,
    pub tags: Option<String>,
    pub web: Option<String>,

    // Technical
    pub page_count: i32,
    pub language_iso: Option<String>,
    pub format: Option<String>,
    pub black_and_white: YesNo,
    pub manga: Manga,

    // Story
    pub characters: Option<String>,
    pub teams: Option<String>,
    pub locations: Option<String>,
    pub scan_information: Option<String>,
    pub story_arc: Option<String>,
    pub story_arc_number: Option<String>,
    pub series_group: Option<String>,

    // Rating
    pub age_rating: AgeRating,
    pub community_rating: Option<f32>,

    // Misc
    pub main_character_or_team: Option<String>,
    pub review: Option<String>,

    // Pages
    pub pages: Vec<PageInfo>,
}

// ---------------------------------------------------------------------------
// XML helpers
// ---------------------------------------------------------------------------

/// Escape the five predefined XML entities in text content.
fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

macro_rules! write_str_elem {
    ($buf:expr, $tag:literal, $opt:expr) => {
        if let Some(ref v) = $opt {
            writeln!($buf, "  <{tag}>{val}</{tag}>", tag = $tag, val = xml_escape(v)).unwrap();
        }
    };
}

macro_rules! write_int_elem {
    ($buf:expr, $tag:literal, $opt:expr) => {
        if let Some(v) = $opt {
            writeln!($buf, "  <{tag}>{val}</{tag}>", tag = $tag, val = v).unwrap();
        }
    };
}

// ---------------------------------------------------------------------------
// XML serialization
// ---------------------------------------------------------------------------

impl ComicInfo {
    /// Merge another `ComicInfo` into `self`, preferring values from `other`
    /// when `self`'s field is `None` / default.
    pub fn merge_from(&mut self, other: ComicInfo) {
        macro_rules! merge_opt {
            ($field:ident) => {
                if self.$field.is_none() {
                    self.$field = other.$field;
                }
            };
        }

        merge_opt!(title);
        merge_opt!(series);
        merge_opt!(number);
        merge_opt!(count);
        merge_opt!(volume);
        merge_opt!(alternate_series);
        merge_opt!(alternate_number);
        merge_opt!(alternate_count);
        merge_opt!(summary);
        merge_opt!(notes);
        merge_opt!(year);
        merge_opt!(month);
        merge_opt!(day);
        merge_opt!(writer);
        merge_opt!(penciller);
        merge_opt!(inker);
        merge_opt!(colorist);
        merge_opt!(letterer);
        merge_opt!(cover_artist);
        merge_opt!(editor);
        merge_opt!(translator);
        merge_opt!(publisher);
        merge_opt!(imprint);
        merge_opt!(genre);
        merge_opt!(tags);
        merge_opt!(web);
        merge_opt!(language_iso);
        merge_opt!(format);
        merge_opt!(scan_information);
        merge_opt!(story_arc);
        merge_opt!(story_arc_number);
        merge_opt!(series_group);
        merge_opt!(community_rating);
        merge_opt!(main_character_or_team);
        merge_opt!(review);
        merge_opt!(characters);
        merge_opt!(teams);
        merge_opt!(locations);

        if self.black_and_white == YesNo::Unknown {
            self.black_and_white = other.black_and_white;
        }
        if self.manga == Manga::Unknown {
            self.manga = other.manga;
        }
        if self.age_rating == AgeRating::Unknown {
            self.age_rating = other.age_rating;
        }
        if self.page_count == 0 && other.page_count > 0 {
            self.page_count = other.page_count;
        }
        if self.pages.is_empty() && !other.pages.is_empty() {
            self.pages = other.pages;
        }
    }

    /// Serialize to a UTF-8 XML string conforming to ComicInfo v2.0.
    pub fn to_xml(&self) -> String {
        let mut buf = String::with_capacity(4096);

        buf.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
        buf.push_str(
            "<ComicInfo\
             \n  xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"\
             \n  xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\"\
             \n>\n",
        );

        write_str_elem!(buf, "Title", self.title);
        write_str_elem!(buf, "Series", self.series);
        write_str_elem!(buf, "Number", self.number);
        write_int_elem!(buf, "Count", self.count);
        write_int_elem!(buf, "Volume", self.volume);
        write_str_elem!(buf, "AlternateSeries", self.alternate_series);
        write_str_elem!(buf, "AlternateNumber", self.alternate_number);
        write_int_elem!(buf, "AlternateCount", self.alternate_count);
        write_str_elem!(buf, "Summary", self.summary);
        write_str_elem!(buf, "Notes", self.notes);
        write_int_elem!(buf, "Year", self.year);
        write_int_elem!(buf, "Month", self.month);
        write_int_elem!(buf, "Day", self.day);
        write_str_elem!(buf, "Writer", self.writer);
        write_str_elem!(buf, "Penciller", self.penciller);
        write_str_elem!(buf, "Inker", self.inker);
        write_str_elem!(buf, "Colorist", self.colorist);
        write_str_elem!(buf, "Letterer", self.letterer);
        write_str_elem!(buf, "CoverArtist", self.cover_artist);
        write_str_elem!(buf, "Editor", self.editor);
        write_str_elem!(buf, "Translator", self.translator);
        write_str_elem!(buf, "Publisher", self.publisher);
        write_str_elem!(buf, "Imprint", self.imprint);
        write_str_elem!(buf, "Genre", self.genre);
        write_str_elem!(buf, "Tags", self.tags);
        write_str_elem!(buf, "Web", self.web);
        writeln!(buf, "  <PageCount>{}</PageCount>", self.page_count).unwrap();

        if let Some(ref lang) = self.language_iso {
            writeln!(buf, "  <LanguageISO>{}</LanguageISO>", xml_escape(lang)).unwrap();
        }

        write_str_elem!(buf, "Format", self.format);
        writeln!(
            buf,
            "  <BlackAndWhite>{}</BlackAndWhite>",
            self.black_and_white.as_str()
        )
        .unwrap();
        writeln!(buf, "  <Manga>{}</Manga>", self.manga.as_str()).unwrap();
        write_str_elem!(buf, "Characters", self.characters);
        write_str_elem!(buf, "Teams", self.teams);
        write_str_elem!(buf, "Locations", self.locations);
        write_str_elem!(buf, "ScanInformation", self.scan_information);
        write_str_elem!(buf, "StoryArc", self.story_arc);
        write_str_elem!(buf, "StoryArcNumber", self.story_arc_number);
        write_str_elem!(buf, "SeriesGroup", self.series_group);
        writeln!(buf, "  <AgeRating>{}</AgeRating>", self.age_rating.as_str()).unwrap();

        if !self.pages.is_empty() {
            buf.push_str("  <Pages>\n");
            for page in &self.pages {
                write!(
                    buf,
                    "    <Page Image=\"{}\" Type=\"{}\"",
                    page.image,
                    page.page_type.as_str()
                )
                .unwrap();
                if page.double_page {
                    buf.push_str(" DoublePage=\"true\"");
                }
                if page.image_size > 0 {
                    write!(buf, " ImageSize=\"{}\"", page.image_size).unwrap();
                }
                if page.image_width >= 0 {
                    write!(buf, " ImageWidth=\"{}\"", page.image_width).unwrap();
                }
                if page.image_height >= 0 {
                    write!(buf, " ImageHeight=\"{}\"", page.image_height).unwrap();
                }
                buf.push_str(" />\n");
            }
            buf.push_str("  </Pages>\n");
        }

        if let Some(rating) = self.community_rating {
            writeln!(buf, "  <CommunityRating>{:.2}</CommunityRating>", rating).unwrap();
        }

        write_str_elem!(buf, "MainCharacterOrTeam", self.main_character_or_team);
        write_str_elem!(buf, "Review", self.review);

        buf.push_str("</ComicInfo>\n");
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_escape_works() {
        assert_eq!(xml_escape("A & B < C > D"), "A &amp; B &lt; C &gt; D");
        assert_eq!(xml_escape("it's \"fine\""), "it&apos;s &quot;fine&quot;");
    }

    #[test]
    fn empty_comic_info_serializes() {
        let ci = ComicInfo::default();
        let xml = ci.to_xml();
        assert!(xml.contains("<ComicInfo"));
        assert!(xml.contains("<PageCount>0</PageCount>"));
        assert!(xml.contains("<BlackAndWhite>Unknown</BlackAndWhite>"));
        assert!(xml.contains("<Manga>Unknown</Manga>"));
        assert!(xml.contains("<AgeRating>Unknown</AgeRating>"));
        assert!(xml.contains("</ComicInfo>"));
    }

    #[test]
    fn full_comic_info_round_trips() {
        let mut ci = ComicInfo::default();
        ci.series = Some("My Series".to_string());
        ci.number = Some("1".to_string());
        ci.year = Some(2024);
        ci.page_count = 24;
        ci.manga = Manga::Yes;
        ci.language_iso = Some("ja".to_string());
        ci.pages = vec![
            PageInfo::new(0, PageType::FrontCover),
            PageInfo::new(1, PageType::Story),
        ];

        let xml = ci.to_xml();
        assert!(xml.contains("<Series>My Series</Series>"));
        assert!(xml.contains("<Number>1</Number>"));
        assert!(xml.contains("<Year>2024</Year>"));
        assert!(xml.contains("<PageCount>24</PageCount>"));
        assert!(xml.contains("<Manga>Yes</Manga>"));
        assert!(xml.contains("<LanguageISO>ja</LanguageISO>"));
        assert!(xml.contains("Type=\"FrontCover\""));
        assert!(xml.contains("Type=\"Story\""));
    }

    #[test]
    fn merge_from_fills_blanks() {
        let mut base = ComicInfo::default();
        base.series = Some("Base".to_string());

        let mut other = ComicInfo::default();
        other.series = Some("Other".to_string());
        other.publisher = Some("Pub".to_string());
        other.page_count = 10;

        base.merge_from(other);
        // pre-existing value preserved
        assert_eq!(base.series.as_deref(), Some("Base"));
        // blank filled in
        assert_eq!(base.publisher.as_deref(), Some("Pub"));
        assert_eq!(base.page_count, 10);
    }
}
