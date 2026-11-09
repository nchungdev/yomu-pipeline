use image::{Rgba, RgbaImage};
use yomu_pipeline::types::NormalizedRect;
use yomu_pipeline::{InpaintResult, StatisticalFlatFill};

#[allow(clippy::too_many_arguments)]
fn create_bubble_canvas(
    w: u32,
    h: u32,
    bx: u32,
    by: u32,
    bw: u32,
    bh: u32,
    stroke_thickness: u32,
    bg_color: Rgba<u8>,
    stroke_color: Rgba<u8>,
    outside_color: Rgba<u8>,
) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(w, h, outside_color);
    for y in 0..bh {
        for x in 0..bw {
            let px = bx + x;
            let py = by + y;
            let is_border = x < stroke_thickness
                || x >= bw.saturating_sub(stroke_thickness)
                || y < stroke_thickness
                || y >= bh.saturating_sub(stroke_thickness);
            if is_border {
                img.put_pixel(px, py, stroke_color);
            } else {
                img.put_pixel(px, py, bg_color);
            }
        }
    }
    img
}

#[test]
fn test_white_bubble_with_black_text_cleans_text_and_preserves_border() {
    let mut img = create_bubble_canvas(
        100,
        100,
        10,
        10,
        80,
        80,
        2,
        Rgba([255, 255, 255, 255]),
        Rgba([0, 0, 0, 255]),
        Rgba([40, 40, 40, 255]),
    );

    // Draw dark text inside the bubble
    for y in 45..55 {
        for x in 30..70 {
            img.put_pixel(x, y, Rgba([0, 0, 0, 255]));
        }
    }

    let norm = NormalizedRect::new(0.1, 0.1, 0.8, 0.8).unwrap();
    let ff = StatisticalFlatFill::default();
    let res = ff.inpaint_rgba_image(&mut img, norm);

    assert_eq!(res, InpaintResult::FlatFilled);

    // Border should be preserved
    assert_eq!(*img.get_pixel(10, 50), Rgba([0, 0, 0, 255]));
    assert_eq!(*img.get_pixel(89, 50), Rgba([0, 0, 0, 255]));

    // Text area should be restored to white
    assert_eq!(*img.get_pixel(50, 50), Rgba([255, 255, 255, 255]));
}

#[test]
fn test_rejects_complex_noisy_background() {
    let mut img = RgbaImage::new(100, 100);
    for y in 0..100 {
        for x in 0..100 {
            let val = ((x * 17 + y * 31) % 256) as u8;
            img.put_pixel(x, y, Rgba([val, val, val, 255]));
        }
    }

    let norm = NormalizedRect::new(0.1, 0.1, 0.8, 0.8).unwrap();
    let ff = StatisticalFlatFill::default();
    let res = ff.inpaint_rgba_image(&mut img, norm);

    assert_eq!(res, InpaintResult::ComplexBackground);
}
