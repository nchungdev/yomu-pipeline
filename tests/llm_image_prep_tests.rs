#![cfg(feature = "llm")]

use std::io::Cursor;
use std::time::Instant;

use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use yomu_pipeline::llm::{prepare_page, PrepError, PrepOptions};

fn encode(img: &DynamicImage, fmt: ImageFormat) -> Vec<u8> {
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), fmt).unwrap();
    out
}

fn gradient_rgb(w: u32, h: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
        Rgb([(x % 251) as u8, (y % 241) as u8, ((x + y) % 199) as u8])
    }))
}

fn gradient_gray(w: u32, h: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
        let v = ((x * 3 + y * 5) % 256) as u8;
        Rgb([v, v, v])
    }))
}

fn decoded(jpeg: &[u8]) -> DynamicImage {
    image::load_from_memory_with_format(jpeg, ImageFormat::Jpeg).unwrap()
}

#[test]
fn hash_is_blake3_of_original_and_stable() {
    let bytes = encode(&gradient_rgb(64, 64), ImageFormat::Png);
    let a = prepare_page(&bytes, &PrepOptions::default()).unwrap();
    let b = prepare_page(&bytes, &PrepOptions::default()).unwrap();
    assert_eq!(a.source_hash, blake3::hash(&bytes).to_hex().to_string());
    assert_eq!(a.source_hash, b.source_hash);
    let opts = PrepOptions {
        max_edge: 32,
        jpeg_quality: 50,
        ..PrepOptions::default()
    };
    assert_eq!(
        prepare_page(&bytes, &opts).unwrap().source_hash,
        a.source_hash
    );
    let other = encode(&gradient_rgb(65, 64), ImageFormat::Png);
    assert_ne!(
        prepare_page(&other, &PrepOptions::default())
            .unwrap()
            .source_hash,
        a.source_hash
    );
}

#[test]
fn small_jpeg_is_passed_through_byte_identical() {
    let jpeg = encode(&gradient_rgb(200, 300), ImageFormat::Jpeg);
    assert!(jpeg.len() < 250_000);
    let p = prepare_page(&jpeg, &PrepOptions::default()).unwrap();
    assert_eq!(p.tiles.len(), 1);
    assert_eq!(p.tiles[0].jpeg, jpeg);
    assert_eq!((p.width, p.height), (200, 300));
    assert_eq!((p.tiles[0].y_start_px, p.tiles[0].y_end_px), (0, 300));
}

#[test]
fn oversized_dimension_jpeg_is_not_passed_through() {
    let jpeg = encode(
        &DynamicImage::ImageRgb8(RgbImage::new(1700, 100)),
        ImageFormat::Jpeg,
    );
    assert!(jpeg.len() < 250_000);
    let p = prepare_page(&jpeg, &PrepOptions::default()).unwrap();
    assert_ne!(p.tiles[0].jpeg, jpeg);
    assert_eq!(decoded(&p.tiles[0].jpeg).width(), 1600);
}

#[test]
fn large_png_is_downscaled_keeping_aspect() {
    let png = encode(&gradient_rgb(3200, 2000), ImageFormat::Png);
    let p = prepare_page(&png, &PrepOptions::default()).unwrap();
    assert_eq!(p.tiles.len(), 1);
    let t = &p.tiles[0];
    let img = decoded(&t.jpeg);
    assert_eq!((img.width(), img.height()), (1600, 1000));
    assert_eq!((p.width, p.height), (3200, 2000));
    assert_eq!((t.y_start_px, t.y_end_px), (0, 2000));
    assert!((t.scale - 0.5).abs() < 1e-3);
    assert_eq!(t.mime, "image/jpeg");
}

#[test]
fn grayscale_becomes_one_channel_and_colour_stays_rgb() {
    let gray = encode(&gradient_gray(400, 300), ImageFormat::Png);
    let g = prepare_page(&gray, &PrepOptions::default()).unwrap();
    assert_eq!(decoded(&g.tiles[0].jpeg).color().channel_count(), 1);
    let colour = encode(&gradient_rgb(400, 300), ImageFormat::Png);
    let c = prepare_page(&colour, &PrepOptions::default()).unwrap();
    assert_eq!(decoded(&c.tiles[0].jpeg).color().channel_count(), 3);
}

#[test]
fn tall_page_is_tiled_with_overlap_and_full_coverage() {
    let png = encode(&gradient_gray(800, 12000), ImageFormat::Png);
    let started = Instant::now();
    let p = prepare_page(&png, &PrepOptions::default()).unwrap();
    assert!(started.elapsed().as_secs() < 60, "tiling too slow");
    assert_eq!((p.width, p.height), (800, 12000));
    // span 2048, overlap 307, step 1741 -> ceil((12000 - 2048) / 1741) + 1 = 7 tiles.
    assert_eq!(p.tiles.len(), 7);
    assert_eq!(p.tiles[0].y_start_px, 0);
    assert_eq!(p.tiles.last().unwrap().y_end_px, 12000);
    for pair in p.tiles.windows(2) {
        let overlap = pair[0].y_end_px as i64 - pair[1].y_start_px as i64;
        assert!(overlap > 0, "gap between tiles");
        assert!(
            (overlap as f32 / 2048.0 - 0.15).abs() < 0.01,
            "overlap {overlap}"
        );
    }
    for t in &p.tiles {
        let img = decoded(&t.jpeg);
        assert!(img.width() <= 1280 && img.height() <= 2049);
        assert_eq!(img.width(), 800);
        assert_eq!(t.scale, 1.0);
    }
}

#[test]
fn wide_tall_page_is_scaled_to_tile_width() {
    let png = encode(&gradient_rgb(2560, 8000), ImageFormat::Png);
    let p = prepare_page(&png, &PrepOptions::default()).unwrap();
    assert!(p.tiles.len() > 1);
    assert_eq!(p.tiles.last().unwrap().y_end_px, 8000);
    for t in &p.tiles {
        let img = decoded(&t.jpeg);
        assert_eq!(img.width(), 1280);
        assert!(img.height() <= 2049);
        assert!((t.scale - 0.5).abs() < 1e-3);
    }
}

fn png_with_ihdr(w: u32, h: u32) -> Vec<u8> {
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 == 1 {
                    (c >> 1) ^ 0xEDB8_8320
                } else {
                    c >> 1
                };
            }
        }
        !c
    }
    let mut ihdr = b"IHDR".to_vec();
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(13u32.to_be_bytes());
    out.extend(&ihdr);
    out.extend(crc(&ihdr).to_be_bytes());
    out
}

#[test]
fn bad_inputs_return_errors_without_panicking() {
    let opts = PrepOptions::default();
    assert_eq!(prepare_page(&[], &opts), Err(PrepError::Empty));
    assert!(prepare_page(b"definitely not an image", &opts).is_err());
    let png = encode(&gradient_rgb(300, 300), ImageFormat::Png);
    assert!(prepare_page(&png[..png.len() / 2], &opts).is_err());
    assert!(prepare_page(&png[..20], &opts).is_err());
    assert!(prepare_page(&png_with_ihdr(0, 0), &opts).is_err());
    assert!(prepare_page(&png_with_ihdr(100_000, 100_000), &opts).is_err());
    let tiny_limit = PrepOptions {
        max_pixels: 100,
        ..PrepOptions::default()
    };
    assert_eq!(prepare_page(&png, &tiny_limit), Err(PrepError::TooLarge));
}

#[test]
fn error_messages_do_not_leak_data() {
    let msg = PrepError::Decode.to_string();
    assert!(!msg.contains("http") && !msg.contains("data:"));
}
