//! Shared contract assertions for the synthetic and the real-page end-to-end tests.
#![allow(dead_code)]

use image::RgbaImage;
use yomu_pipeline::contract::v1::{
    apply_erase, EraseSpec, PageResult, PatchFormat, RegionStatus, RenderMode,
};
use yomu_pipeline::{process_page_result, FontRenderer, PageOutcome, PipelineOptions, RegionInput};

pub fn run(
    img: &RgbaImage,
    regions: &[RegionInput],
    mode: RenderMode,
    bytes: Option<&[u8]>,
) -> PageOutcome {
    process_page_result(
        "page-1",
        img,
        bytes,
        regions,
        &FontRenderer::default(),
        PipelineOptions::default(),
        mode,
    )
}

/// Patch rectangles in integer pixels: `(x, y, w, h)`.
pub fn patch_rects(result: &PageResult) -> Vec<(u32, u32, u32, u32)> {
    result
        .regions
        .iter()
        .filter_map(|r| match &r.erase {
            EraseSpec::Patch { rect_px, .. } => Some((
                rect_px.x as u32,
                rect_px.y as u32,
                rect_px.w as u32,
                rect_px.h as u32,
            )),
            EraseSpec::None => None,
        })
        .collect()
}

fn inside(rects: &[(u32, u32, u32, u32)], x: u32, y: u32) -> bool {
    rects
        .iter()
        .any(|&(rx, ry, w, h)| x >= rx && x < rx + w && y >= ry && y < ry + h)
}

/// Checks everything the contract promises for one page; returns the client-side clean image.
pub fn assert_contract(original: &RgbaImage, out: &PageOutcome) -> RgbaImage {
    let result = &out.result;
    result.validate().expect("PageResult must validate");

    let json = serde_json::to_string(result).expect("serialize");
    let back: PageResult = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(&back, result, "JSON round-trip must be lossless");

    assert_eq!(
        (result.page_px.width, result.page_px.height),
        original.dimensions()
    );
    assert!(result.cache_key.is_empty(), "engine fills cache_key later");
    assert_eq!(result.source_hash.len(), 64);

    for (i, r) in result.regions.iter().enumerate() {
        assert_eq!(r.reading_order as usize, i);
        match (&r.status, &r.erase) {
            (RegionStatus::Rendered, _) => assert!(r.layout.is_some(), "{}", r.region_id),
            (_, erase) => assert_eq!(erase, &EraseSpec::None, "{}", r.region_id),
        }
        if let EraseSpec::Patch { format, data, .. } = &r.erase {
            assert_eq!(*format, PatchFormat::RawRgba);
            assert!(!data.is_empty());
        }
    }

    let (applied, warnings) = apply_erase(original, &result.regions);
    assert!(warnings.is_empty(), "apply warnings: {warnings:?}");
    assert_eq!(applied.dimensions(), original.dimensions());

    let rects = patch_rects(result);
    for (x, y, p) in applied.enumerate_pixels() {
        if inside(&rects, x, y) {
            assert_eq!(p, out.clean.get_pixel(x, y), "inside patch at ({x},{y})");
        } else {
            assert_eq!(p, original.get_pixel(x, y), "outside patches at ({x},{y})");
            assert_eq!(out.clean.get_pixel(x, y), original.get_pixel(x, y));
        }
    }
    applied
}

/// Overlay output has no composited image; Composited differs from the clean image only inside
/// the laid-out text lines (plus stroke slack) and differs somewhere when a region rendered.
pub fn assert_composited_only_adds_text(clean: &RgbaImage, out: &PageOutcome) {
    let blob = out.result.composited.as_ref().expect("composited blob");
    let baked = image::load_from_memory(&blob.data)
        .expect("decode composited")
        .to_rgba8();
    assert_eq!(baked.dimensions(), clean.dimensions());

    let mut boxes = Vec::new();
    for r in &out.result.regions {
        let Some(l) = &r.layout else { continue };
        let slack = l.font_px * 0.15 + 3.0;
        for line in &l.lines {
            boxes.push((
                line.origin_px.x - slack,
                line.origin_px.y - l.font_px - slack,
                line.origin_px.x + line.width_px + slack,
                line.origin_px.y + l.font_px * 0.5 + slack,
            ));
        }
    }
    let mut diffs = 0usize;
    for (x, y, p) in baked.enumerate_pixels() {
        if p != clean.get_pixel(x, y) {
            diffs += 1;
            let (fx, fy) = (x as f32, y as f32);
            assert!(
                boxes
                    .iter()
                    .any(|&(x0, y0, x1, y1)| fx >= x0 && fx <= x1 && fy >= y0 && fy <= y1),
                "composited differs outside any text line at ({x},{y})"
            );
        }
    }
    if out.result.regions.iter().any(|r| r.status.is_rendered()) {
        assert!(diffs > 0, "composited image must contain drawn text");
    }
}

/// Runs both render modes and every assertion; returns `(overlay, composited)` outcomes.
pub fn full_check(
    original: &RgbaImage,
    regions: &[RegionInput],
    bytes: Option<&[u8]>,
) -> (PageOutcome, PageOutcome) {
    let overlay = run(original, regions, RenderMode::Overlay, bytes);
    assert!(overlay.result.composited.is_none(), "Overlay draws no text");
    let applied = assert_contract(original, &overlay);
    assert_eq!(applied, overlay.clean);

    let comp = run(original, regions, RenderMode::Composited, bytes);
    assert_contract(original, &comp);
    assert_eq!(comp.clean, overlay.clean, "clean image is mode independent");
    assert_composited_only_adds_text(&comp.clean, &comp);
    (overlay, comp)
}
