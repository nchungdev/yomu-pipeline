//! Provider-neutral result of one vision LLM call (ADR-0002 section 7.2).

use serde::{Deserialize, Serialize};

use crate::contract::v1::RegionKind;
use crate::types::NormalizedRect;

/// One text region reported by the model, already normalized to the page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmRegion {
    pub id: String,
    /// Page-normalized `[0, 1]` bounds, clamped to the page.
    pub bounds: NormalizedRect,
    /// Source text as read by the model.
    pub text: String,
    pub translated: String,
    pub kind: RegionKind,
    /// Speaker as named by the model; `None` when unsure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    /// One of `m`, `f`, `u`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gender: Option<String>,
    /// One of `child`, `teen`, `adult`, `elder`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub age: Option<String>,
    /// One of `neutral`, `happy`, `sad`, `angry`, `surprised`, `serious`, `whisper`, `fear`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emotion: Option<String>,
    /// Emotion intensity clamped to `[0, 1]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intensity: Option<f32>,
}

/// All regions of one page, tied to the page image by its content hash.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmPage {
    pub source_hash: String,
    pub regions: Vec<LlmRegion>,
}
