//! End-to-end contract test on a synthetic page: bubbles, a complex region, an SFX region.

mod common;

use common::{full_check, run};
use image::{Rgba, RgbaImage};
use yomu_pipeline::contract::v1::{
    EraseSpec, RegionFailure, RegionKind, RegionStatus, RenderMode, SkipReason,
};
use yomu_pipeline::types::{BubbleStyleHint, NormalizedRect};
use yomu_pipeline::{process_page, FontRenderer, PipelineOptions, RegionInput};

const GRAY: Rgba<u8> = Rgba([90, 90, 90, 255]);
const W: u32 = 900;
const H: u32 = 600;

fn bubble(img: &mut RgbaImage, x0: u32, y0: u32, w: u32, h: u32) {
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            let edge = x < x0 + 3 || x >= x0 + w - 3 || y < y0 + 3 || y >= y0 + h - 3;
            img.put_pixel(
                x,
                y,
                if edge {
                    Rgba([0, 0, 0, 255])
                } else {
                    Rgba([250, 250, 250, 255])
                },
            );
        }
    }
    for y in y0 + h / 2 - 8..y0 + h / 2 + 8 {
        for x in x0 + w / 4..x0 + 3 * w / 4 {
            img.put_pixel(x, y, Rgba([10, 10, 10, 255]));
        }
    }
}

/// Large black/white checkerboard: not flat, so the flat fill refuses it.
fn noise(img: &mut RgbaImage, x0: u32, y0: u32, w: u32, h: u32) {
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            let v = if (x / 12 + y / 12) % 2 == 0 { 30 } else { 220 };
            img.put_pixel(x, y, Rgba([v, v, v, 255]));
        }
    }
}

fn page() -> RgbaImage {
    let mut img = RgbaImage::from_pixel(W, H, GRAY);
    bubble(&mut img, 40, 40, 260, 180);
    bubble(&mut img, 360, 60, 240, 170);
    bubble(&mut img, 60, 330, 300, 200);
    noise(&mut img, 620, 330, 240, 200);
    img
}

fn rect(x: u32, y: u32, w: u32, h: u32) -> NormalizedRect {
    NormalizedRect::new(
        x as f32 / W as f32,
        y as f32 / H as f32,
        w as f32 / W as f32,
        h as f32 / H as f32,
    )
    .expect("valid")
}

fn regions() -> Vec<RegionInput> {
    // FloatingText forbids bubble detection, so the noisy background cannot be erased cleanly.
    let mut complex = RegionInput::new("complex", rect(620, 330, 240, 200), "Nền phức tạp");
    complex.style_hint = BubbleStyleHint::FloatingText;
    let mut sfx = RegionInput::new("sfx", rect(380, 400, 120, 100), "RẦM");
    sfx.kind = RegionKind::Sfx;
    vec![
        RegionInput::new("b1", rect(40, 40, 260, 180), "Xin chào các bạn"),
        RegionInput::new("b2", rect(360, 60, 240, 170), "Mình đi thôi nào"),
        RegionInput::new("b3", rect(60, 330, 300, 200), "Đây là bong bóng thứ ba"),
        complex,
        sfx,
    ]
}

#[test]
fn synthetic_page_satisfies_contract() {
    let img = page();
    let regions = regions();
    let (overlay, _) = full_check(&img, &regions, None);
    let r = &overlay.result.regions;

    for region in r.iter().take(3) {
        assert_eq!(
            region.status,
            RegionStatus::Rendered,
            "{}",
            region.region_id
        );
        assert!(matches!(region.erase, EraseSpec::Patch { .. }));
        assert!(region.polygon.len() >= 3);
    }
    assert_eq!(
        r[3].status,
        RegionStatus::Skipped {
            reason: SkipReason::ComplexBackground
        }
    );
    assert_eq!(
        r[4].status,
        RegionStatus::Skipped {
            reason: SkipReason::SfxPolicy
        }
    );

    // Skipped regions keep every source pixel.
    for (x, y, p) in img.enumerate_pixels() {
        if (620..860).contains(&x) && (330..530).contains(&y)
            || (380..500).contains(&x) && (400..500).contains(&y)
        {
            assert_eq!(p, overlay.clean.get_pixel(x, y));
        }
    }
    // The text blob of bubble 1 is gone from the clean page.
    assert_eq!(
        overlay.clean.get_pixel(170, 130),
        &Rgba([250, 250, 250, 255])
    );
    assert_eq!(img.get_pixel(170, 130), &Rgba([10, 10, 10, 255]));
}

#[test]
fn composited_matches_process_page() {
    let img = page();
    let regions = regions();
    let comp = run(&img, &regions, RenderMode::Composited, None);
    let blob = comp.result.composited.expect("blob");
    let baked = image::load_from_memory(&blob.data).expect("png").to_rgba8();
    let legacy = process_page(
        &img,
        &regions,
        &FontRenderer::default(),
        PipelineOptions::default(),
    );
    assert_eq!(baked, legacy.image);
    assert_eq!(
        comp.result
            .regions
            .iter()
            .map(|r| r.status.clone())
            .collect::<Vec<_>>(),
        legacy.statuses
    );
}

#[test]
fn invalid_bounds_region_still_validates() {
    let img = page();
    let bad = NormalizedRect {
        x: 2.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    };
    let mut regions = regions();
    regions.push(RegionInput::new("bad", bad, "x"));
    let (overlay, _) = full_check(&img, &regions, None);
    let last = overlay.result.regions.last().expect("region");
    assert_eq!(
        last.status,
        RegionStatus::Failed {
            reason: RegionFailure::InvalidBounds
        }
    );
    assert_eq!(last.bounds.width, 1.0);
}

#[test]
fn empty_page_and_empty_text() {
    let img = page();
    let (overlay, _) = full_check(&img, &[], None);
    assert!(overlay.result.regions.is_empty());
    assert_eq!(overlay.clean, img);

    let blank = vec![RegionInput::new("e", rect(40, 40, 260, 180), "  ")];
    let (o, _) = full_check(&img, &blank, None);
    assert_eq!(
        o.result.regions[0].status,
        RegionStatus::Skipped {
            reason: SkipReason::EmptyText
        }
    );
    assert_eq!(o.clean, img);
}

#[test]
fn region_at_image_corner_stays_in_bounds() {
    let mut img = RgbaImage::from_pixel(W, H, GRAY);
    bubble(&mut img, 0, 0, 220, 160);
    let regions = vec![RegionInput::new("c", rect(0, 0, 220, 160), "Góc trang")];
    let (o, _) = full_check(&img, &regions, None);
    assert_eq!(o.result.regions[0].status, RegionStatus::Rendered);
}

#[test]
fn source_hash_uses_bytes_when_given() {
    let img = page();
    let a = run(&img, &[], RenderMode::Overlay, Some(b"file-bytes"));
    let b = run(&img, &[], RenderMode::Overlay, Some(b"other-bytes"));
    let c = run(&img, &[], RenderMode::Overlay, None);
    let c2 = run(&img, &[], RenderMode::Overlay, None);
    assert_ne!(a.result.source_hash, b.result.source_hash);
    assert_ne!(a.result.source_hash, c.result.source_hash);
    assert_eq!(c.result.source_hash, c2.result.source_hash);
}
