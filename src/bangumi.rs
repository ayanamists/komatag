//! Bangumi API client.
//!
//! Fetches subject metadata from <https://api.bgm.tv> and maps it into a
//! [`ComicInfo`] struct.
//!
//! # Authentication
//!
//! A personal access token is **optional** for public subjects but raises the
//! rate limit and enables NSFW results.  Pass it via `--bangumi-token` or the
//! `BANGUMI_TOKEN` environment variable.
//!
//! API reference: api/bangumi-openapi.json  (v2026-01-22)

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

use crate::comic_info::{ComicInfo, Manga, YesNo};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const BASE_URL: &str = "https://api.bgm.tv";
const USER_AGENT: &str = concat!(
    "komatag/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/ayanamists/komatag)"
);

// ---------------------------------------------------------------------------
// Raw API response types
// ---------------------------------------------------------------------------

/// Subset of the `Subject` schema we actually use.
#[derive(Debug, Deserialize)]
struct Subject {
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    name_cn: String,
    #[serde(default)]
    summary: String,
    /// Air / release date in `YYYY-MM-DD` format (optional).
    #[serde(default)]
    date: Option<String>,
    /// Total volume count (books).
    #[serde(default)]
    volumes: i32,
    /// Community rating information.
    #[serde(default)]
    rating: Option<RatingBlock>,
    /// Structured wiki infobox entries.
    #[serde(default)]
    infobox: Vec<InfoboxItem>,
    /// Whether this subject is a series (vs a single volume/episode).
    #[serde(default)]
    series: bool,
}

#[derive(Debug, Deserialize, Default)]
struct RatingBlock {
    score: f64,
}

/// A single infobox key-value pair.
///
/// The `value` field is polymorphic in the API (string **or** array of KV
/// objects), so we deserialise it as raw [`Value`] and handle both cases.
#[derive(Debug, Deserialize)]
struct InfoboxItem {
    key: String,
    value: Value,
}

// ---------------------------------------------------------------------------
// Relation response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone)]
pub struct RelatedSubject {
    pub id: u64,
    #[serde(rename = "type")]
    pub subject_type: u8,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub name_cn: String,
    #[serde(default)]
    pub relation: String,
}

// ---------------------------------------------------------------------------
// Search response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    data: Vec<SearchResult>,
}

#[derive(Debug, Deserialize)]
struct SearchResult {
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    name_cn: String,
    #[serde(default)]
    date: Option<String>,
    #[serde(rename = "type")]
    subject_type: u8,
    /// `true` when this is the main entry of a book series (系列主条目),
    /// as opposed to an individual 单行本 volume.
    #[serde(default)]
    series: bool,
    /// Book platform, e.g. "漫画", "小说", "画集".
    #[serde(default)]
    platform: String,
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// Thin wrapper around a blocking [`reqwest::blocking::Client`] pre-configured
/// for the Bangumi API.
pub struct BangumiClient {
    http: reqwest::blocking::Client,
    token: Option<String>,
}

impl BangumiClient {
    /// Create a new client.  `token` is the optional Bearer access token.
    pub fn new(token: Option<String>) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .context("Failed to build HTTP client")?;

        Ok(Self { http, token })
    }

    /// Add authorisation header when a token is present.
    fn request(&self, method: reqwest::Method, url: &str) -> reqwest::blocking::RequestBuilder {
        let builder = self.http.request(method, url);
        if let Some(ref tok) = self.token {
            builder.bearer_auth(tok)
        } else {
            builder
        }
    }

    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    /// Fetch a subject by its numeric Bangumi ID and convert it to a
    /// [`ComicInfo`].
    ///
    /// The `page_count` argument should come from the archive; it will be set
    /// on the returned struct unchanged (Bangumi doesn't track page counts).
    pub fn fetch_subject(&self, subject_id: u64) -> Result<ComicInfo> {
        let url = format!("{}/v0/subjects/{}", BASE_URL, subject_id);

        let resp = self
            .request(reqwest::Method::GET, &url)
            .send()
            .with_context(|| format!("GET {url} failed"))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().unwrap_or_default();
            anyhow::bail!("Bangumi API returned HTTP {status} for subject {subject_id}:\n{body}");
        }

        let subject: Subject = resp
            .json()
            .with_context(|| format!("Failed to parse subject {subject_id} response"))?;

        Ok(subject_to_comic_info(subject))
    }

    /// Fetch related subjects for a given subject ID.
    pub fn fetch_relations(&self, subject_id: u64) -> Result<Vec<RelatedSubject>> {
        let url = format!("{}/v0/subjects/{}/subjects", BASE_URL, subject_id);

        let resp = self
            .request(reqwest::Method::GET, &url)
            .send()
            .with_context(|| format!("GET {url} failed"))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().unwrap_or_default();
            anyhow::bail!(
                "Bangumi API returned HTTP {status} for subject relations {subject_id}:\n{body}"
            );
        }

        let relations: Vec<RelatedSubject> = resp
            .json()
            .with_context(|| format!("Failed to parse subject relations {subject_id} response"))?;

        Ok(relations)
    }

    /// Search for subjects matching `keyword` (manga type = 1 by default).
    ///
    /// Returns up to `limit` results (max 25).
    pub fn search(
        &self,
        keyword: &str,
        subject_type: Option<u8>,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        let url = format!("{}/v0/search/subjects?limit={}", BASE_URL, limit.min(25));

        let mut body = serde_json::json!({ "keyword": keyword });
        if let Some(t) = subject_type {
            body["filter"] = serde_json::json!({ "type": [t] });
        }

        let resp = self
            .request(reqwest::Method::POST, &url)
            .json(&body)
            .send()
            .with_context(|| format!("POST {url} failed"))?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().unwrap_or_default();
            anyhow::bail!("Bangumi search returned HTTP {status}:\n{body_text}");
        }

        let search_resp: SearchResponse = resp.json().context("Failed to parse search response")?;

        let hits = search_resp
            .data
            .into_iter()
            .map(|r| SearchHit {
                id: r.id,
                name: r.name,
                name_cn: r.name_cn,
                date: r.date,
                subject_type: r.subject_type,
                series: r.series,
                platform: r.platform,
            })
            .collect();

        Ok(hits)
    }
}

// ---------------------------------------------------------------------------
// Public search result type
// ---------------------------------------------------------------------------

/// A single result from the search endpoint.
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub id: u64,
    pub name: String,
    pub name_cn: String,
    pub date: Option<String>,
    pub subject_type: u8,
    /// `true` when this is the main entry of a book series (系列主条目).
    pub series: bool,
    /// Book platform, e.g. "漫画", "小说", "画集".
    pub platform: String,
}

impl std::fmt::Display for SearchHit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let cn = if self.name_cn.is_empty() {
            String::new()
        } else {
            format!(" / {}", self.name_cn)
        };
        let date = self
            .date
            .as_deref()
            .map(|d| format!(" ({})", d))
            .unwrap_or_default();
        // Tag the series head and platform so a human (or LLM) picking an ID
        // can tell the series entry apart from individual 单行本 volumes.
        let mut tags = Vec::new();
        if self.series {
            tags.push("系列".to_owned());
        }
        if !self.platform.is_empty() {
            tags.push(self.platform.clone());
        }
        let tag_str = if tags.is_empty() {
            String::new()
        } else {
            format!(" [{}]", tags.join(", "))
        };
        write!(f, "[{}] {}{}{}{}", self.id, self.name, cn, date, tag_str)
    }
}

// ---------------------------------------------------------------------------
// Mapping helpers
// ---------------------------------------------------------------------------

/// Convert a raw `Subject` into a `ComicInfo`.
fn subject_to_comic_info(s: Subject) -> ComicInfo {
    let mut ci = ComicInfo::default();

    // Prefer Chinese name as title when available
    let title_name = if !s.name_cn.is_empty() {
        s.name_cn.clone()
    } else {
        s.name.clone()
    };

    if s.series {
        ci.series = Some(title_name.clone());
    }
    ci.title = Some(title_name);

    if !s.summary.is_empty() {
        ci.summary = Some(s.summary);
    }

    // Release date: "YYYY-MM-DD"
    if let Some(ref date_str) = s.date {
        let parts: Vec<&str> = date_str.splitn(3, '-').collect();
        if let Some(y) = parts.first().and_then(|p| p.parse::<i32>().ok()) {
            ci.year = Some(y);
        }
        if let Some(m) = parts.get(1).and_then(|p| p.parse::<i32>().ok()) {
            ci.month = Some(m);
        }
        if let Some(d) = parts.get(2).and_then(|p| p.parse::<i32>().ok()) {
            ci.day = Some(d);
        }
    }

    // Volume count
    if s.volumes > 0 {
        ci.count = Some(s.volumes);
    }

    // Community rating (0–10 on Bangumi → scale to 0–5)
    if let Some(ref r) = s.rating {
        if r.score > 0.0 {
            let scaled = (r.score / 2.0).clamp(0.0, 5.0) as f32;
            ci.community_rating = Some((scaled * 100.0).round() / 100.0);
        }
    }

    // Add link to Bangumi page in the Web field
    ci.web = Some(format!("https://bgm.tv/subject/{}", s.id));

    // Infobox
    apply_infobox(&mut ci, &s.infobox);

    ci
}

/// Walk the infobox entries and populate `ComicInfo` fields.
fn apply_infobox(ci: &mut ComicInfo, infobox: &[InfoboxItem]) {
    for item in infobox {
        let text = flatten_infobox_value(&item.value);
        if text.is_empty() {
            continue;
        }

        match item.key.as_str() {
            // Publisher
            "出版社" | "发行" => {
                if ci.publisher.is_none() {
                    ci.publisher = Some(text);
                }
            }
            // Writer / scenario
            "作者" | "原作" | "著" => {
                if ci.writer.is_none() {
                    ci.writer = Some(text);
                }
            }
            // Artist / penciller
            "作画" | "漫画" | "绘" => {
                if ci.penciller.is_none() {
                    ci.penciller = Some(text);
                }
            }
            // Translator
            "译者" | "翻译" => {
                if ci.translator.is_none() {
                    ci.translator = Some(text);
                }
            }
            // Genre
            "类型" | "分类" => {
                if ci.genre.is_none() {
                    ci.genre = Some(text);
                }
            }
            // Serialized in
            "连载杂志" | "杂志" => {
                // Stash in imprint if not set yet
                if ci.imprint.is_none() {
                    ci.imprint = Some(text);
                }
            }
            // Volume number for a specific entry
            "卷号" | "册" => {
                if ci.volume.is_none() {
                    if let Ok(v) = text.trim().parse::<i32>() {
                        ci.volume = Some(v);
                    }
                }
            }
            // Language
            "语言" | "言語" => {
                if ci.language_iso.is_none() {
                    ci.language_iso = Some(lang_name_to_iso(&text));
                }
            }
            // Format
            "装帧" | "开本" => {
                if ci.format.is_none() {
                    ci.format = Some(text);
                }
            }
            // Colour / B&W
            "是否彩色" | "色彩" => {
                if ci.black_and_white == YesNo::Unknown {
                    ci.black_and_white = if text.contains("彩")
                        || text.to_ascii_lowercase().contains("color")
                    {
                        YesNo::No // "No" it is not B&W → it IS colour
                    } else if text.contains("黑白") || text.to_ascii_lowercase().contains("b&w") {
                        YesNo::Yes
                    } else {
                        YesNo::Unknown
                    };
                }
            }
            // Manga flag: origin country
            "国家/地区" | "国家" | "地区" => {
                if ci.manga == Manga::Unknown {
                    let lower = text.to_ascii_lowercase();
                    if lower.contains("日本") || lower.contains("japan") {
                        ci.manga = Manga::Yes;
                    }
                }
            }
            // Age rating
            "年龄分级" | "分级" | "レーティング" => {
                // Leave AgeRating::Unknown; mapping is too locale-specific
            }
            // ISBN – stick in Notes
            "ISBN" | "ISBN-13" | "ISBN-10" => {
                let note = format!("ISBN: {}", text);
                ci.notes = Some(match ci.notes.take() {
                    Some(prev) => format!("{}\n{}", prev, note),
                    None => note,
                });
            }
            _ => {}
        }
    }
}

/// Flatten a possibly-nested infobox value to a comma-separated string.
///
/// The API returns either:
/// - a plain string, or
/// - an array of `{"v": "..."}` / `{"k": "...", "v": "..."}` objects.
fn flatten_infobox_value(val: &Value) -> String {
    match val {
        Value::String(s) => s.trim().to_owned(),
        Value::Array(arr) => arr
            .iter()
            .filter_map(|item| {
                // Try the "v" key first; fall back to the whole item as string
                item.get("v")
                    .and_then(Value::as_str)
                    .map(|s| s.trim().to_owned())
                    .or_else(|| item.as_str().map(|s| s.trim().to_owned()))
            })
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// Best-effort mapping of a human-readable language name to an ISO 639-1 code.
fn lang_name_to_iso(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if lower.contains("日本語") || lower.contains("japanese") || lower.contains("日语") {
        return "ja".to_owned();
    }
    if lower.contains("中文") || lower.contains("chinese") || lower.contains("汉语") {
        if lower.contains("繁") || lower.contains("traditional") {
            return "zh-Hant".to_owned();
        }
        if lower.contains("简") || lower.contains("simplified") {
            return "zh-Hans".to_owned();
        }
        return "zh".to_owned();
    }
    if lower.contains("english") || lower.contains("英语") || lower.contains("英文") {
        return "en".to_owned();
    }
    if lower.contains("korean") || lower.contains("韩语") || lower.contains("朝鲜") {
        return "ko".to_owned();
    }
    // Return the original string if we can't map it
    name.to_owned()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_string_value() {
        let v = Value::String("Shueisha".to_owned());
        assert_eq!(flatten_infobox_value(&v), "Shueisha");
    }

    #[test]
    fn flatten_array_value() {
        let v = serde_json::json!([{"v": "Oda Eiichiro"}, {"v": "Shueisha"}]);
        assert_eq!(flatten_infobox_value(&v), "Oda Eiichiro, Shueisha");
    }

    #[test]
    fn flatten_kv_array_value() {
        let v = serde_json::json!([{"k": "Original", "v": "Weekly Shonen Jump"}]);
        assert_eq!(flatten_infobox_value(&v), "Weekly Shonen Jump");
    }

    #[test]
    fn lang_name_mapping() {
        assert_eq!(lang_name_to_iso("日本語"), "ja");
        assert_eq!(lang_name_to_iso("简体中文"), "zh-Hans");
        assert_eq!(lang_name_to_iso("繁體中文"), "zh-Hant");
        assert_eq!(lang_name_to_iso("English"), "en");
        assert_eq!(lang_name_to_iso("Korean"), "ko");
        assert_eq!(lang_name_to_iso("Unknown Lang"), "Unknown Lang");
    }

    #[test]
    fn subject_to_comic_info_basic() {
        let s = Subject {
            id: 12345,
            name: "One Piece".to_owned(),
            name_cn: "海贼王".to_owned(),
            summary: "A pirate adventure.".to_owned(),
            date: Some("1997-07-22".to_owned()),
            volumes: 105,
            rating: Some(RatingBlock { score: 9.0 }),
            series: true,
            infobox: vec![
                InfoboxItem {
                    key: "出版社".to_owned(),
                    value: Value::String("集英社".to_owned()),
                },
                InfoboxItem {
                    key: "作者".to_owned(),
                    value: Value::String("尾田栄一郎".to_owned()),
                },
            ],
        };

        let ci = subject_to_comic_info(s);
        assert_eq!(ci.series.as_deref(), Some("海贼王"));
        assert_eq!(ci.year, Some(1997));
        assert_eq!(ci.month, Some(7));
        assert_eq!(ci.day, Some(22));
        assert_eq!(ci.count, Some(105));
        assert_eq!(ci.publisher.as_deref(), Some("集英社"));
        assert_eq!(ci.writer.as_deref(), Some("尾田栄一郎"));
        // 9.0 / 2 = 4.50
        assert_eq!(ci.community_rating, Some(4.5));
        assert_eq!(ci.web.as_deref(), Some("https://bgm.tv/subject/12345"));
    }
}
