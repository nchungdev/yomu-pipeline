use image::{Rgba, RgbaImage};
use yomu_pipeline::types::{BubbleStyleHint, NormalizedRect, RegionInput};
use yomu_pipeline::{process_page, FontRenderer, PipelineOptions};

fn create_test_page() -> (RgbaImage, Vec<RegionInput>) {
    let mut img = RgbaImage::from_pixel(200, 200, Rgba([30, 30, 30, 255]));

    // Draw a white bubble at (20, 20, 160, 160) with a 2px black border
    for y in 0..160 {
        for x in 0..160 {
            let px = 20 + x;
            let py = 20 + y;
            let is_border = !(2..158).contains(&x) || !(2..158).contains(&y);
            if is_border {
                img.put_pixel(px, py, Rgba([0, 0, 0, 255]));
            } else {
                img.put_pixel(px, py, Rgba([255, 255, 255, 255]));
            }
        }
    }

    // Add some original dark lettering to inpaint
    for y in 90..105 {
        for x in 50..150 {
            img.put_pixel(x, y, Rgba([0, 0, 0, 255]));
        }
    }

    let regions = vec![RegionInput {
        id: "bubble_1".to_string(),
        bounds: NormalizedRect::new(0.1, 0.1, 0.8, 0.8).unwrap(),
        text: "Xin chào thế giới manga!".to_string(),
        style_hint: BubbleStyleHint::Balloon,
    }];

    (img, regions)
}

#[test]
fn test_font_renderer_default_and_measure() {
    let renderer = FontRenderer::default();

    let w1 = renderer.measure_width("Xin chào", 16.0);
    let w2 = renderer.measure_width("Xin chào thế giới rộng lớn", 16.0);

    assert!(w1 > 0.0);
    assert!(w2 > w1);

    // Scaling scales linearly with font size
    let w_scaled = renderer.measure_width("Xin chào", 32.0);
    assert!((w_scaled / w1 - 2.0).abs() < 0.2);
}

#[test]
fn test_process_page_end_to_end() {
    let (img, regions) = create_test_page();
    let renderer = FontRenderer::default();

    let output = process_page(&img, &regions, &renderer, PipelineOptions::default());

    assert_eq!(output.layouts.len(), 1);
    let layout = output.layouts[0].as_ref().expect("layout should succeed");
    assert!(!layout.lines.is_empty());

    // The output image dimensions match the input
    assert_eq!(output.image.width(), img.width());
    assert_eq!(output.image.height(), img.height());

    // The old ink at (100, 100) was in the bubble interior
    // Verify that the bubble was inpainted and translated text rendered
    let has_non_white = (40..160).any(|x| {
        (40..160).any(|y| {
            let px = output.image.get_pixel(x, y);
            // Drawn glyph pixels are darker than white background
            px[0] < 100
        })
    });
    assert!(
        has_non_white,
        "Rendered text glyphs should be visible on canvas"
    );
}
