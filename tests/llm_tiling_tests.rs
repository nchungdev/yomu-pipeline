#![cfg(feature = "llm")]

use yomu_pipeline::llm::{dedupe_overlapping, remap_tile_rect, PreparedTile};
use yomu_pipeline::types::NormalizedRect;

fn tile(y0: u32, y1: u32) -> PreparedTile {
    PreparedTile {
        jpeg: Vec::new(),
        mime: "image/jpeg",
        y_start_px: y0,
        y_end_px: y1,
        scale: 1.0,
    }
}

fn r(x: f32, y: f32, w: f32, h: f32) -> NormalizedRect {
    NormalizedRect::new(x, y, w, h).unwrap()
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn remap_maps_tile_rect_to_page_rect() {
    // Tile covers px 2000..4000 of a 10000 px page.
    let out = remap_tile_rect(&tile(2000, 4000), 10_000, 800, r(0.1, 0.5, 0.3, 0.25)).unwrap();
    assert!(close(out.x, 0.1) && close(out.width, 0.3));
    assert!(close(out.y, 0.3), "y {}", out.y);
    assert!(close(out.height, 0.05));
}

#[test]
fn remap_handles_bottom_tile() {
    // Short last tile 11000..12000 of a 12000 px page; rect in its lower half.
    let out = remap_tile_rect(&tile(11_000, 12_000), 12_000, 800, r(0.0, 0.5, 1.0, 0.5)).unwrap();
    assert!(close(out.y, 11_500.0 / 12_000.0));
    assert!(close(out.y + out.height, 1.0));
}

#[test]
fn remap_clamps_and_drops_degenerate() {
    let t = tile(0, 1000);
    let slightly_over = NormalizedRect {
        x: 0.9,
        y: 0.9,
        width: 0.3,
        height: 0.3,
    };
    let out = remap_tile_rect(&t, 1000, 100, slightly_over).unwrap();
    assert!(out.x + out.width <= 1.0001 && out.y + out.height <= 1.0001);
    let outside = NormalizedRect {
        x: 0.0,
        y: 1.5,
        width: 0.2,
        height: 0.2,
    };
    assert!(remap_tile_rect(&t, 1000, 100, outside).is_none());
    let nan = NormalizedRect {
        x: f32::NAN,
        y: 0.0,
        width: 0.2,
        height: 0.2,
    };
    assert!(remap_tile_rect(&t, 1000, 100, nan).is_none());
    assert!(remap_tile_rect(&t, 0, 100, r(0.0, 0.0, 0.5, 0.5)).is_none());
}

type Item = (NormalizedRect, &'static str);

fn dedupe(items: Vec<Item>) -> Vec<Item> {
    dedupe_overlapping(items, |i| i.0, |i| i.1)
}

#[test]
fn dedupe_removes_overlap_duplicate_keeping_more_complete_box() {
    let cut = (r(0.1, 0.40, 0.3, 0.05), "Hello there friend");
    let full = (r(0.1, 0.40, 0.3, 0.08), "Hello there friend");
    let out = dedupe(vec![cut, full]);
    assert_eq!(out, vec![full]);
    // Order independent.
    assert_eq!(dedupe(vec![full, cut]), vec![full]);
}

#[test]
fn dedupe_keeps_distinct_regions_with_similar_boxes() {
    let a = (r(0.1, 0.40, 0.3, 0.08), "Hello there friend");
    let b = (r(0.1, 0.41, 0.3, 0.08), "Run away right now");
    assert_eq!(dedupe(vec![a, b]).len(), 2);
}

#[test]
fn dedupe_keeps_same_text_in_disjoint_boxes() {
    let a = (r(0.1, 0.1, 0.2, 0.05), "Ha ha");
    let b = (r(0.1, 0.7, 0.2, 0.05), "Ha ha");
    assert_eq!(dedupe(vec![a, b]).len(), 2);
}

#[test]
fn dedupe_is_deterministic_on_exact_ties() {
    let a = (r(0.1, 0.1, 0.2, 0.05), "same");
    let b = (r(0.1, 0.1, 0.2, 0.05), "same");
    assert_eq!(dedupe(vec![a, b]).len(), 1);
    assert_eq!(dedupe(Vec::new()).len(), 0);
}
