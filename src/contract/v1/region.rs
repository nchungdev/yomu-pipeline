use serde::{Deserialize, Serialize};

use super::error::ContractError;
use super::layout::TextLayoutV1;
use crate::types::{InferredTypography, NormalizedRect, PxRect};

/// Semantic category of a detected text region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionKind {
    #[default]
    #[serde(alias = "Balloon")]
    Balloon,
    #[serde(alias = "Narration")]
    Narration,
    #[serde(alias = "Sfx", alias = "SFX")]
    Sfx,
    #[serde(alias = "Sign")]
    Sign,
    #[serde(alias = "Other")]
    Other,
}

/// Why a region was intentionally left untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    SfxPolicy,
    EmptyText,
    MissingTranslation,
    ComplexBackground,
}

/// Why processing of a region failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionFailure {
    LayoutDoesNotFit,
    InvalidBounds,
    EraseFailed,
    Malformed,
}

/// Outcome of one region.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RegionStatus {
    Rendered,
    Skipped { reason: SkipReason },
    Failed { reason: RegionFailure },
}

impl RegionStatus {
    #[inline]
    pub fn is_rendered(&self) -> bool {
        matches!(self, Self::Rendered)
    }
}

/// Encoding of an embedded image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageFormat {
    Png,
    Jpeg,
    Webp,
}

/// Encoded image bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageBlob {
    pub format: ImageFormat,
    pub data: Vec<u8>,
}

/// Pixel encoding of an erase patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatchFormat {
    /// `w * h * 4` bytes, row-major RGBA8, no header (embedded transport).
    RawRgba,
    /// A PNG file (HTTP transport).
    Png,
}

/// How to remove the source text of a region on the original image.
///
/// Only pre-rendered patches cross the contract (ADR-0002 section 7.1); consumers never inpaint.
/// A `Rendered` region may carry `None` when there is nothing to erase.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EraseSpec {
    /// Paste a pre-rendered patch at `rect_px` (pixels of the original image).
    /// `rect_norm` is the same rectangle normalized to the page, for clients that rescale.
    Patch {
        rect_norm: NormalizedRect,
        rect_px: PxRect,
        format: PatchFormat,
        data: Vec<u8>,
    },
    /// The source text stays untouched.
    None,
}

/// Result for one region of a page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegionResult {
    pub region_id: String,
    pub kind: RegionKind,
    pub reading_order: u32,
    pub source_text: String,
    pub translated_text: String,
    pub confidence: f32,
    pub bounds: NormalizedRect,
    /// Region outline in normalized page coordinates; empty when only `bounds` is known.
    #[serde(default)]
    pub polygon: Vec<[f32; 2]>,
    pub typography: InferredTypography,
    pub layout: Option<TextLayoutV1>,
    pub erase: EraseSpec,
    pub status: RegionStatus,
    /// Speaker, gender, age, emotion and intensity as reported by the model (ADR-0002 7.2);
    /// the app feeds them back into glossary and context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gender: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub age: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emotion: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intensity: Option<f32>,
}

impl RegionResult {
    /// Checks the invariants: a region that is not `Rendered` never erases anything, and a
    /// `Rendered` region always has a layout (its `erase` may still be `None`).
    pub fn validate(&self) -> Result<(), ContractError> {
        let id = || self.region_id.clone();
        self.bounds
            .validate()
            .map_err(|_| ContractError::InvalidBounds(id()))?;
        if !self.status.is_rendered() && self.erase != EraseSpec::None {
            return Err(ContractError::EraseWithoutRender(id()));
        }
        if self.status.is_rendered() && self.layout.is_none() {
            return Err(ContractError::RenderedWithoutLayout(id()));
        }
        Ok(())
    }
}
