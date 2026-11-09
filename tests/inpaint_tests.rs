use image::{Rgba, RgbaImage};
use yomu_pipeline::contour::{BubbleContour, ContourDetectionOptions, DEFAULT_INSET};
use yomu_pipeline::types::{BubbleStyleHint, NormalizedRect, RegionInput};
use yomu_pipeline::{inpaint_page, layout_region, FontRange};

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
fn test_smart_inpaint_preserves_stroke() {
    let mut img = create_test_bubble_image(100, 100, 10, 10, 80, 80, 3);
    let norm = NormalizedRect::new(0.1, 0.1, 0.8, 0.8).expect("valid rect");
    let opts = ContourDetectionOptions::default();

    let contour = BubbleContour::detect(&img, norm, opts).expect("contour found");

    // Draw dark ink text inside
    for y in 45..55 {
        for x in 30..70 {
            img.put_pixel(x, y, Rgba([10, 10, 10, 255]));
        }
    }

    let modified = contour.smart_inpaint(&mut img, DEFAULT_INSET, None);
    assert!(modified > 0);

    // Inner region should be filled with seed color (whiteish)
    let center = *img.get_pixel(50, 50);
    assert!(center[0] >= 240);
}

#[test]
fn test_inpaint_page_and_layout() {
    let img = create_test_bubble_image(200, 200, 20, 20, 160, 160, 2);
    let norm = NormalizedRect::new(0.1, 0.1, 0.8, 0.8).unwrap();
    let regions = vec![RegionInput {
        id: "b1".to_string(),
        bounds: norm,
        text: "Xin chào bạn nhỏ".to_string(),
        style_hint: BubbleStyleHint::Balloon,
    }];

    let result = inpaint_page(&img, &regions);
    assert_eq!(result.contours.len(), 1);
    let c = result.contours[0].as_ref().expect("contour detected");

    let measure = |s: &str, px: f32| s.chars().count() as f32 * px * 0.55;
    let layout = layout_region(c, "Xin chào bạn nhỏ", &measure, FontRange::default());
    assert!(layout.is_some());
}
