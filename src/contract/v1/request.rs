use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

use super::region::RegionResult;
use super::SCHEMA_VERSION;

/// Where the bytes of a page come from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageSource {
    Path(PathBuf),
    Bytes(Vec<u8>),
}

/// Previously computed result for a page, used to skip unchanged work.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriorPage {
    pub cache_key: String,
    pub regions: Vec<RegionResult>,
}

/// One page to translate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageInput {
    pub page_id: String,
    pub image: ImageSource,
    /// Hash of the original file bytes when the caller already knows it.
    #[serde(default)]
    pub known_source_hash: Option<String>,
    #[serde(default)]
    pub prior: Option<PriorPage>,
    /// A few preceding lines (speaker and emotion included) to keep tone and pronouns consistent.
    #[serde(default)]
    pub context: Vec<ContextLine>,
}

/// One earlier line of dialogue offered to the model as context (ADR-0002 section 7.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextLine {
    #[serde(default)]
    pub speaker: Option<String>,
    pub text: String,
    #[serde(default)]
    pub emotion: Option<String>,
}

/// Provider secret. Never serialized and never printed.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Credentials {
    pub api_key: Option<String>,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = if self.api_key.is_some() {
            "set"
        } else {
            "unset"
        };
        write!(f, "Credentials(<redacted:{state}>)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlossaryEntry {
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub category: Option<GlossaryCategory>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlossaryCategory {
    Character,
    Location,
    Skill,
    Honorific,
    General,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Gemini,
    OpenAiCompatible,
    CustomHttp,
}

/// One link of the provider fallback chain. The secret is injected by the caller, never
/// serialized, redacted in `Debug` and never part of the cache key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSpec {
    pub kind: ProviderKind,
    #[serde(default)]
    pub endpoint: Option<String>,
    pub model: String,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(skip)]
    pub credentials: Credentials,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadingDirection {
    #[default]
    Rtl,
    Ltr,
}

/// `Overlay` is the default; `Composited` is for export only and never crosses the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderMode {
    #[default]
    Overlay,
    Composited,
}

/// Size and time bounds of a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    pub max_pages: u32,
    pub max_image_bytes: u64,
    pub timeout_ms: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_pages: 64,
            max_image_bytes: 32 * 1024 * 1024,
            timeout_ms: 120_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranslateOptions {
    /// Deprecated: single provider name, superseded by `chain`. Kept for old payloads.
    #[serde(default)]
    pub provider: String,
    /// Providers tried in order; empty means "use `provider`".
    #[serde(default)]
    pub chain: Vec<ProviderSpec>,
    /// Injected per request by the caller; skipped on the wire.
    #[serde(skip)]
    pub credentials: Credentials,
    pub source_lang: String,
    pub target_lang: String,
    #[serde(default)]
    pub glossary: Vec<GlossaryEntry>,
    #[serde(default)]
    pub reading_direction: ReadingDirection,
    #[serde(default)]
    pub render_mode: RenderMode,
    #[serde(default)]
    pub font_id: Option<String>,
    #[serde(default = "default_limits")]
    pub limits: Limits,
}

fn default_limits() -> Limits {
    Limits::default()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranslateBatchRequest {
    pub schema_version: u32,
    pub request_id: String,
    pub pages: Vec<PageInput>,
    pub options: TranslateOptions,
}

impl TranslateBatchRequest {
    pub fn new(request_id: String, pages: Vec<PageInput>, options: TranslateOptions) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            request_id,
            pages,
            options,
        }
    }
}
