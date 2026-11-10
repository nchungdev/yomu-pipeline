//! Parsing of region input: a plain `RegionInput` array or a `.yomu.json` sidecar object.
//!
//! The single entry point [`parse_page`] is shared by the CLI, the worker and FFI consumers.
//! Regions that cannot be processed are not silently dropped: they come back in
//! [`ParsedPage::rejected`] with a status and a reason.

use serde_json::Value;

use crate::contract::v1::{RegionFailure, RegionKind, RegionStatus, SkipReason};
use crate::pipeline::RegionReport;
use crate::types::{BubbleStyleHint, InferredTypography, NormalizedRect, RegionInput};

/// Why a page cannot be parsed at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SidecarError {
    /// The sidecar says the page was not translated successfully; the page must be skipped.
    #[error("sidecar status is `{0}`, not `ok`")]
    NotOk(String),
    #[error("unrecognized region input: {0}")]
    Format(String),
}

/// Regions ready for [`crate::process_page`] plus those rejected before processing.
#[derive(Debug, Clone, Default)]
pub struct ParsedPage {
    pub regions: Vec<RegionInput>,
    pub rejected: Vec<RegionReport>,
    pub warnings: Vec<String>,
}

/// Parses either `[RegionInput, ...]` or a sidecar `{ "status": "ok", "regions": [...] }`.
pub fn parse_page(value: &Value) -> Result<ParsedPage, SidecarError> {
    if let Some(items) = value.as_array() {
        return Ok(parse_items(items, parse_plain));
    }
    let obj = value
        .as_object()
        .ok_or_else(|| SidecarError::Format("expected an array or an object".into()))?;
    if let Some(status) = obj.get("status") {
        match status.as_str() {
            Some("ok") => {}
            Some(other) => return Err(SidecarError::NotOk(other.to_string())),
            None => return Err(SidecarError::NotOk(status.to_string())),
        }
    }
    let items = obj
        .get("regions")
        .and_then(Value::as_array)
        .ok_or_else(|| SidecarError::Format("missing `regions` array".into()))?;
    Ok(parse_items(items, parse_sidecar_item))
}

type ItemResult = Result<RegionInput, RegionReport>;

fn parse_items(items: &[Value], parse: fn(usize, &Value) -> ItemResult) -> ParsedPage {
    let mut page = ParsedPage::default();
    for (i, item) in items.iter().enumerate() {
        match parse(i, item) {
            Ok(region) => page.regions.push(region),
            Err(report) => {
                page.warnings.push(format!(
                    "region `{}` rejected: {:?}",
                    report.id, report.status
                ));
                page.rejected.push(report);
            }
        }
    }
    page
}

fn reject(id: String, status: RegionStatus) -> RegionReport {
    RegionReport { id, status }
}

fn failed(id: String, reason: RegionFailure) -> RegionReport {
    reject(id, RegionStatus::Failed { reason })
}

fn valid_bounds(value: Option<&Value>) -> Option<NormalizedRect> {
    let rect: NormalizedRect = serde_json::from_value(value?.clone()).ok()?;
    rect.validate().ok().map(|()| rect)
}

fn parse_plain(i: usize, item: &Value) -> ItemResult {
    let id = item
        .get("id")
        .and_then(Value::as_str)
        .map_or_else(|| format!("region-{i}"), str::to_string);
    let region: RegionInput = serde_json::from_value(item.clone())
        .map_err(|_| failed(id.clone(), RegionFailure::Malformed))?;
    if region.bounds.validate().is_err() {
        return Err(failed(id, RegionFailure::InvalidBounds));
    }
    Ok(region)
}

fn parse_sidecar_item(i: usize, item: &Value) -> ItemResult {
    let recognized = item.get("recognized");
    let field = |name: &str| item.get(name).or_else(|| recognized?.get(name));
    let id = recognized
        .and_then(|r| r.get("id"))
        .and_then(Value::as_str)
        .map_or_else(|| format!("region-{i}"), str::to_string);

    let Some(bounds) = valid_bounds(recognized.and_then(|r| r.get("bounds"))) else {
        return Err(failed(id, RegionFailure::InvalidBounds));
    };
    // Never fall back to the recognized (source) text: drawing it back would be a no-op lie.
    let Some(text) = item.get("translated_text").and_then(Value::as_str) else {
        return Err(reject(
            id,
            RegionStatus::Skipped {
                reason: SkipReason::MissingTranslation,
            },
        ));
    };
    let kind = field("kind")
        .map(|v| serde_json::from_value(v.clone()).unwrap_or(RegionKind::Other))
        .unwrap_or_default();
    let typography = field("typography")
        .and_then(|v| serde_json::from_value::<InferredTypography>(v.clone()).ok());
    Ok(RegionInput {
        id,
        bounds,
        text: text.to_string(),
        style_hint: BubbleStyleHint::Balloon,
        kind,
        typography,
    })
}
