//! Real-page contract test. Ignored by default; needs a page image and its regions:
//!
//! ```text
//! YOMU_E2E_IMAGE=page.png YOMU_E2E_REGIONS=regions.json [YOMU_E2E_OUT=dir] \
//!     cargo test --release -- --ignored e2e
//! ```
//!
//! `YOMU_E2E_REGIONS` is a `RegionInput` array (or a sidecar object, see `parse_page`). The
//! before/after PNGs go to `YOMU_E2E_OUT` (default: a temp dir) for visual inspection. Never
//! commit page images or their derivatives.

mod common;

use common::full_check;
use std::path::PathBuf;
use yomu_pipeline::parse_page;

#[test]
#[ignore = "needs YOMU_E2E_IMAGE and YOMU_E2E_REGIONS"]
fn e2e_real_page_satisfies_contract() {
    let (Some(image_path), Some(regions_path)) = (
        std::env::var_os("YOMU_E2E_IMAGE"),
        std::env::var_os("YOMU_E2E_REGIONS"),
    ) else {
        eprintln!("SKIPPED: set YOMU_E2E_IMAGE and YOMU_E2E_REGIONS to run the real-page test");
        return;
    };
    let out_dir = std::env::var_os("YOMU_E2E_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("yomu-e2e"));
    std::fs::create_dir_all(&out_dir).expect("create output dir");

    let bytes = std::fs::read(&image_path).expect("read image");
    let original = image::load_from_memory(&bytes).expect("decode").to_rgba8();
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&regions_path).expect("read regions"))
            .expect("regions json");
    let parsed = parse_page(&json).expect("parse regions");
    assert!(
        parsed.rejected.is_empty(),
        "rejected: {:?}",
        parsed.rejected
    );

    let (overlay, comp) = full_check(&original, &parsed.regions, Some(&bytes));

    let stem = PathBuf::from(&image_path)
        .file_stem()
        .map_or_else(|| "page".into(), |s| s.to_string_lossy().into_owned());
    original
        .save(out_dir.join(format!("{stem}.before.png")))
        .expect("save before");
    overlay
        .clean
        .save(out_dir.join(format!("{stem}.clean.png")))
        .expect("save clean");
    let blob = comp.result.composited.as_ref().expect("composited");
    std::fs::write(out_dir.join(format!("{stem}.after.png")), &blob.data).expect("save after");

    for r in &overlay.result.regions {
        eprintln!("{}: {:?}", r.region_id, r.status);
    }
    eprintln!("outputs in {}", out_dir.display());
}
