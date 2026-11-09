use image::{Rgba, RgbaImage};
use yomu_pipeline::infer_typography;
use yomu_pipeline::types::NormalizedRect;

fn create_canvas(w: u32, h: u32, bg: Rgba<u8>) -> RgbaImage {
    RgbaImage::from_pixel(w, h, bg)
}

#[test]
fn test_black_text_on_white_background() {
    let white = Rgba([255, 255, 255, 255]);
    let black = Rgba([0, 0, 0, 255]);
    let mut img = create_canvas(100, 100, white);
    // Draw two horizontal black text bars inside the box
    for y in 30..36 {
        for x in 25..75 {
            img.put_pixel(x, y, black);
        }
    }
    for y in 50..56 {
        for x in 25..75 {
            img.put_pixel(x, y, black);
        }
    }

    let norm_box = NormalizedRect::new(0.1, 0.1, 0.8, 0.8).unwrap();
    let typo = infer_typography(&img, norm_box);

    assert_eq!(typo.text_color(), [0, 0, 0, 255]);
    assert_eq!(typo.stroke_color(), None);
    assert_eq!(typo.stroke_width(), 0.0);
    assert_eq!(typo.rotation_rad(), 0.0);
    assert!(!typo.is_vertical());
    assert!(typo.estimated_font_size().is_some());
}

#[test]
fn test_white_text_with_black_outline() {
    let gray = Rgba([80, 80, 80, 255]);
    let black = Rgba([0, 0, 0, 255]);
    let white = Rgba([255, 255, 255, 255]);
    let mut img = create_canvas(100, 100, gray);

    // Thick black stroke around white text core
    for y in 30..60 {
        for x in 30..70 {
            let is_edge = (30..34).contains(&y)
                || (56..60).contains(&y)
                || (30..34).contains(&x)
                || (66..70).contains(&x);
            if is_edge {
                img.put_pixel(x, y, black);
            } else {
                img.put_pixel(x, y, white);
            }
        }
    }

    let norm_box = NormalizedRect::new(0.1, 0.1, 0.8, 0.8).unwrap();
    let typo = infer_typography(&img, norm_box);

    assert_eq!(typo.text_color(), [255, 255, 255, 255]);
    assert_eq!(typo.stroke_color(), Some([0, 0, 0, 255]));
    assert!(typo.stroke_width() >= 1.5);
}
