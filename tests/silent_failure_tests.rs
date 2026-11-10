//! A region whose translation cannot be drawn must never lose its source text.

use image::{Rgba, RgbaImage};
use yomu_pipeline::contour::{BubbleContour, ContourDetectionOptions};
use yomu_pipeline::contract::v1::{RegionFailure, RegionKind, RegionStatus, SkipReason};
use yomu_pipeline::types::{BubbleStyleHint, NormalizedRect, RegionInput};
use yomu_pipeline::{
    inpaint_page, layout_region, process_page, FontRange, FontRenderer, InferredTypography,
    PipelineOptions,
};

fn bubble_page() -> RgbaImage {
    let mut img = RgbaImage::from_pixel(300, 300, Rgba([30, 30, 30, 255]));
    for y in 30..270 {
        for x in 30..270 {
            let border = !(33..267).contains(&x) || !(33..267).contains(&y);
            img.put_pixel(
                x,
                y,
                if border {
                    Rgba([0, 0, 0, 255])
                } else {
                    Rgba([250, 250, 250, 255])
                },
            );
        }
    }
    for y in 140..160 {
        for x in 100..200 {
            img.put_pixel(x, y, Rgba([10, 10, 10, 255]));
        }
    }
    img
}

fn region(text: &str) -> RegionInput {
    let bounds = NormalizedRect::new(0.15, 0.15, 0.7, 0.7).expect("valid");
    RegionInput::new("r1", bounds, text)
}

fn run(img: &RgbaImage, regions: &[RegionInput]) -> yomu_pipeline::ProcessedPage {
    process_page(
        img,
        regions,
        &FontRenderer::default(),
        PipelineOptions::default(),
    )
}

#[test]
fn renders_and_erases_when_text_fits() {
    let img = bubble_page();
    let out = run(&img, &[region("Xin chào")]);
    assert_eq!(out.statuses, vec![RegionStatus::Rendered]);
    assert!(out.layouts[0].is_some());
    assert_ne!(out.image, img);
    assert!(out.all_rendered());
}

#[test]
fn text_that_does_not_fit_leaves_the_source_untouched() {
    let img = bubble_page();
    let long = "x".repeat(600);
    let out = run(&img, &[region(&long)]);
    assert_eq!(
        out.statuses,
        vec![RegionStatus::Failed {
            reason: RegionFailure::LayoutDoesNotFit
        }]
    );
    assert_eq!(out.image, img, "source text must not be erased");
    assert!(out.layouts[0].is_none());
}

#[test]
fn sfx_and_empty_text_are_skipped_without_touching_pixels() {
    let img = bubble_page();
    let mut sfx = region("Rầm!");
    sfx.kind = RegionKind::Sfx;
    let mut blank = region("   ");
    blank.id = "r2".into();
    let out = run(&img, &[sfx, blank]);
    assert_eq!(
        out.statuses,
        vec![
            RegionStatus::Skipped {
                reason: SkipReason::SfxPolicy
            },
            RegionStatus::Skipped {
                reason: SkipReason::EmptyText
            },
        ]
    );
    assert_eq!(out.image, img);
    let reports = out.reports(&[region("a"), region("b")]);
    assert_eq!(reports.len(), 2);
    assert!(!out.all_rendered());
}

#[test]
fn id_and_text_prefixes_no_longer_change_behavior() {
    let img = bubble_page();
    let mut r = region("*~Xin chào");
    r.id = "float-sfx-d1".into();
    let out = run(&img, &[r]);
    assert_eq!(out.statuses, vec![RegionStatus::Rendered]);
}

#[test]
fn invalid_bounds_fail_that_region_only() {
    let img = bubble_page();
    let mut bad = region("Xin chào");
    bad.bounds = NormalizedRect {
        x: 0.9,
        y: 0.9,
        width: 0.9,
        height: 0.9,
    };
    bad.id = "bad".into();
    let out = run(&img, &[bad, region("Xin chào")]);
    assert_eq!(
        out.statuses[0],
        RegionStatus::Failed {
            reason: RegionFailure::InvalidBounds
        }
    );
    assert_eq!(out.statuses[1], RegionStatus::Rendered);
}

#[test]
fn floating_text_on_flat_background_uses_bounds_fallback() {
    let mut img = RgbaImage::from_pixel(300, 300, Rgba([120, 120, 120, 255]));
    for y in 140..160 {
        for x in 100..200 {
            img.put_pixel(x, y, Rgba([10, 10, 10, 255]));
        }
    }
    let mut r = region("Xin chào");
    r.style_hint = BubbleStyleHint::FloatingText;
    let out = run(&img, &[r]);
    assert_eq!(out.statuses, vec![RegionStatus::Rendered]);
    assert!(out.layouts[0].is_some());
}

#[test]
fn complex_background_without_outline_is_skipped_untouched() {
    let mut img = RgbaImage::new(300, 300);
    let mut seed = 12345u32;
    for p in img.pixels_mut() {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let v = (seed >> 24) as u8;
        *p = Rgba([v, v, v, 255]);
    }
    let mut r = region("Xin chào");
    r.style_hint = BubbleStyleHint::DarkBalloon;
    let out = run(&img, &[r]);
    assert_eq!(
        out.statuses,
        vec![RegionStatus::Skipped {
            reason: SkipReason::ComplexBackground
        }]
    );
    assert_eq!(out.image, img);
}

#[test]
fn inpaint_page_ignores_sfx_and_blank_regions() {
    let img = bubble_page();
    let mut sfx = region("Rầm!");
    sfx.kind = RegionKind::Sfx;
    let out = inpaint_page(&img, &[sfx, region("")]);
    assert!(out.contours.iter().all(Option::is_none));
    assert_eq!(out.image, img);
}

#[test]
fn contour_from_bounds_covers_the_rectangle() {
    let img = bubble_page();
    let b = NormalizedRect::new(0.2, 0.2, 0.5, 0.5).expect("valid");
    let c = BubbleContour::from_bounds(&img, b).expect("contour");
    assert_eq!((c.box_x, c.box_y, c.box_w, c.box_h), (60, 60, 150, 150));
    assert!(c.row_spans.iter().all(|s| *s == Some((0, 149))));
    let tiny = NormalizedRect::new(0.0, 0.0, 0.001, 0.001).expect("valid");
    assert!(BubbleContour::from_bounds(&img, tiny).is_none());
}

#[test]
fn detect_in_pixel_rect_guards_degenerate_rectangles() {
    let img = bubble_page();
    let o = ContourDetectionOptions::default();
    assert!(BubbleContour::detect_in_pixel_rect(&img, 10, 10, 0, 50, o).is_none());
    assert!(BubbleContour::detect_in_pixel_rect(&img, 10, 10, 50, 0, o).is_none());
    assert!(BubbleContour::detect_in_pixel_rect(&img, 280, 280, 50, 50, o).is_none());
    assert!(BubbleContour::detect_in_pixel_rect(&img, u32::MAX, 0, 5, 5, o).is_none());
}

#[test]
fn layout_search_terminates_for_degenerate_font_ranges() {
    let img = bubble_page();
    let c = BubbleContour::from_bounds(&img, NormalizedRect::new(0.2, 0.2, 0.5, 0.5).expect("v"))
        .expect("contour");
    let never_fits = |_: &str, _px: f32| 1.0e9_f32;
    for step_px in [0.0, -1.0, 1.0e-9, f32::NAN, f32::INFINITY] {
        let range = FontRange {
            min_px: 1.0,
            max_px: 1.0e6,
            step_px,
        };
        assert!(layout_region(&c, "abc def", &never_fits, range).is_none());
    }
    let nan = FontRange {
        min_px: f32::NAN,
        max_px: 20.0,
        step_px: 0.5,
    };
    assert!(layout_region(&c, "abc", &never_fits, nan).is_none());
}

#[test]
fn huge_stroke_width_is_clamped() {
    let renderer = FontRenderer::default();
    let img = bubble_page();
    let c = BubbleContour::from_bounds(&img, NormalizedRect::new(0.2, 0.2, 0.5, 0.5).expect("v"))
        .expect("contour");
    let measure = |s: &str, px: f32| renderer.measure_width(s, px);
    let layout = layout_region(&c, "Xin chào", &measure, FontRange::default()).expect("fits");
    for width in [1.0e9, f32::NAN, f32::INFINITY] {
        let typo = InferredTypography::new([255, 255, 255, 255]).with_stroke([0, 0, 0, 255], width);
        let mut canvas = img.clone();
        renderer.render_layout(&mut canvas, &layout, &typo);
    }
}

#[test]
fn layout_v1_origin_is_the_baseline_pen_position() {
    let renderer = FontRenderer::default();
    let img = bubble_page();
    let c = BubbleContour::from_bounds(&img, NormalizedRect::new(0.2, 0.2, 0.5, 0.5).expect("v"))
        .expect("contour");
    let measure = |s: &str, px: f32| renderer.measure_width(s, px);
    let layout = layout_region(&c, "Xin chào", &measure, FontRange::default()).expect("fits");
    let v1 = renderer.to_layout_v1(&layout);
    assert_eq!(v1.lines.len(), layout.lines.len());
    for (a, b) in v1.lines.iter().zip(&layout.lines) {
        assert!((a.origin_px.x + a.width_px * 0.5 - b.slice_center_x).abs() < 1e-3);
        assert!(
            a.origin_px.y > b.slice_y,
            "baseline lies below the slice top"
        );
    }
}
