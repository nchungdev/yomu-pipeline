use serde::{Deserialize, Serialize};

use super::error::ContractError;
use super::region::{ImageBlob, RegionResult};
use super::SCHEMA_VERSION;

/// Size of the original page image in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PagePx {
    pub width: u32,
    pub height: u32,
}

/// Overlay description of a whole page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageResult {
    pub schema_version: u32,
    pub page_id: String,
    pub source_hash: String,
    pub cache_key: String,
    pub page_px: PagePx,
    pub regions: Vec<RegionResult>,
    /// Only present in `RenderMode::Composited`.
    #[serde(default)]
    pub composited: Option<ImageBlob>,
}

impl PageResult {
    /// Validates the schema version and every region.
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ContractError::SchemaVersion {
                found: self.schema_version,
                expected: SCHEMA_VERSION,
            });
        }
        self.regions.iter().try_for_each(RegionResult::validate)
    }
}
