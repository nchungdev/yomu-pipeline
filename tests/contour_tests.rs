use image::{Rgba, RgbaImage};
use yomu_pipeline::contour::{pixel_luminance, BubbleContour, ContourDetectionOptions};
use yomu_pipeline::types::NormalizedRect;

fn create_test_bubble_image(
    w: u32,
    h: u32,
    bx: u32,
    by: u32,
    bw: u32,
    bh: u32,
    stroke_thickness: u32,
) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(w, h, Rgba([30, 30, 30, 255]));

    for y in 0..bh {
        for x in 0..bw {
            let px = bx + x;
            let py = by + y;
            let is_border = x < stroke_thickness
                || x >= bw - stroke_thickness
                || y < stroke_thickness
                || y >= bh - stroke_thickness;

            if is_border {
                img.put_pixel(px, py, Rgba([0, 0, 0, 255]));
            } else {
                img.put_pixel(px, py, Rgba([250, 250, 250, 255]));
            }
        }
    }

    img
}

#[test]
fn test_pixel_luminance() {
    let white = Rgba([255, 255, 255, 255]);
    let black = Rgba([0, 0, 0, 255]);
    let gray = Rgba([128, 128, 128, 255]);

    assert!((pixel_luminance(&white) - 1.0).abs() < 1e-3);
    assert!(pixel_luminance(&black) < 1e-3);
    assert!((pixel_luminance(&gray) - 0.5).abs() < 0.05);
}

#[test]
fn test_detect_bubble_contour() {
    let img = create_test_bubble_image(200, 200, 50, 50, 100, 80, 2);
    let norm_box = NormalizedRect::new(0.25, 0.25, 0.5, 0.4).unwrap();
    let opts = ContourDetectionOptions::default();

    let contour = BubbleContour::detect(&img, norm_box, opts).expect("contour should be detected");

    assert_eq!(contour.box_x, 50);
    assert_eq!(contour.box_y, 50);
    assert_eq!(contour.box_w, 100);
    assert_eq!(contour.box_h, 80);
    assert!(contour.row_spans.len() == 80);

    // Mid row should be valid and span across the bubble interior
    let mid_span = contour.row_spans[40].expect("mid row has span");
    assert!(mid_span.0 < 10);
    assert!(mid_span.1 > 85);
}

#[test]
fn test_detect_best_with_margin() {
    let img = create_test_bubble_image(200, 200, 50, 50, 100, 80, 2);
    // Bounding box slightly smaller than the true bubble
    let tight_box = NormalizedRect::new(0.27, 0.27, 0.45, 0.35).unwrap();
    let opts = ContourDetectionOptions::default();

    let contour = BubbleContour::detect_best(&img, tight_box, opts)
        .expect("best detection should find bubble with margin");

    assert!(contour.box_w >= 90);
    assert!(contour.box_h >= 70);
}
