//! One page job shared by the `inpaint` subcommand and the worker.

use image::{ImageReader, RgbaImage};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use yomu_pipeline::{
    parse_page, process_page, FontRenderer, PipelineOptions, RegionReport, SidecarError,
};

/// Result of one page job, printed by the CLI and returned by the worker.
///
/// `status` is `ok` (every region rendered), `partial` (some region skipped or failed),
/// `skipped` (the sidecar says the page is not translated) or `error`.
#[derive(Debug, Serialize, Deserialize)]
pub struct WorkerResponse {
    pub id: Option<String>,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regions: Vec<RegionReport>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl WorkerResponse {
    pub fn error(id: Option<String>, message: impl Into<String>) -> Self {
        Self {
            id,
            status: "error".into(),
            error: Some(message.into()),
            output_path: None,
            regions: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

/// Saves via a temporary file in the same directory, then renames over `path`.
/// Returns `Ok(false)` without writing when `abandoned` is set (job timed out).
pub fn save_atomic(image: &RgbaImage, path: &Path, abandoned: &AtomicBool) -> Result<bool, String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "output path has no file name".to_string())?;
    let tmp = path.with_file_name(format!(".{}.{}.tmp.{name}", std::process::id(), nonce()));
    image.save(&tmp).map_err(|e| e.to_string())?;
    if abandoned.load(Ordering::Acquire) {
        let _ = std::fs::remove_file(&tmp);
        return Ok(false);
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })?;
    Ok(true)
}

fn nonce() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
}

/// Decodes `image_path`, processes the regions in `regions_json` and writes `output_path`.
pub fn run_page_job(
    id: Option<String>,
    renderer: &FontRenderer,
    image_path: &Path,
    regions_json: &serde_json::Value,
    output_path: &Path,
    abandoned: &AtomicBool,
) -> WorkerResponse {
    let parsed = match parse_page(regions_json) {
        Ok(p) => p,
        Err(SidecarError::NotOk(status)) => {
            return WorkerResponse {
                status: "skipped".into(),
                error: Some(format!("sidecar status is `{status}`")),
                ..WorkerResponse::error(id, "")
            };
        }
        Err(e) => return WorkerResponse::error(id, e.to_string()),
    };
    let rgba = match ImageReader::open(image_path)
        .map_err(|e| e.to_string())
        .and_then(|r| r.decode().map_err(|e| e.to_string()))
    {
        Ok(img) => img.into_rgba8(),
        Err(e) => return WorkerResponse::error(id, format!("Failed to read input image: {e}")),
    };

    let processed = process_page(&rgba, &parsed.regions, renderer, PipelineOptions::default());
    let mut regions = processed.reports(&parsed.regions);
    regions.extend(parsed.rejected);
    let all_ok = regions.iter().all(|r| r.status.is_rendered());

    match save_atomic(&processed.image, output_path, abandoned) {
        Ok(true) => WorkerResponse {
            id,
            status: if all_ok { "ok" } else { "partial" }.into(),
            error: None,
            output_path: Some(output_path.to_path_buf()),
            regions,
            warnings: parsed.warnings,
        },
        Ok(false) => WorkerResponse::error(id, "job abandoned after timeout"),
        Err(e) => WorkerResponse::error(id, format!("Failed to save output image: {e}")),
    }
}
