use image::{Rgba, RgbaImage};
use yomu_pipeline::contract::v1::{
    apply_erase, ApplyWarning, EraseSpec, PatchFormat, RegionKind, RegionResult, RegionStatus,
};
use yomu_pipeline::types::{InferredTypography, NormalizedRect, PxRect};

const RED: Rgba<u8> = Rgba([255, 0, 0, 255]);
const BLUE: Rgba<u8> = Rgba([0, 0, 255, 255]);

fn region(id: &str, erase: EraseSpec) -> RegionResult {
    RegionResult {
        region_id: id.into(),
        kind: RegionKind::Balloon,
        reading_order: 0,
        source_text: String::new(),
        translated_text: String::new(),
        confidence: 1.0,
        bounds: NormalizedRect::new(0.0, 0.0, 0.5, 0.5).expect("valid"),
        polygon: vec![],
        typography: InferredTypography::default(),
        layout: None,
        erase,
        status: RegionStatus::Rendered,
        speaker: None,
        gender: None,
        age: None,
        emotion: None,
        intensity: None,
    }
}

fn patch(x: f32, y: f32, w: f32, h: f32, format: PatchFormat, data: Vec<u8>) -> EraseSpec {
    EraseSpec::Patch {
        rect_norm: NormalizedRect::new(0.0, 0.0, 0.5, 0.5).expect("valid"),
        rect_px: PxRect::new(x, y, w, h),
        format,
        data,
    }
}

fn raw(w: u32, h: u32, px: Rgba<u8>) -> Vec<u8> {
    (0..w * h).flat_map(|_| px.0).collect()
}

fn png(w: u32, h: u32, px: Rgba<u8>) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    RgbaImage::from_pixel(w, h, px)
        .write_to(&mut buf, image::ImageFormat::Png)
        .expect("encode");
    buf.into_inner()
}

fn base() -> RgbaImage {
    RgbaImage::from_pixel(10, 10, Rgba([255, 255, 255, 255]))
}

#[test]
fn raw_and_png_patches_are_pasted() {
    let regions = [
        region(
            "a",
            patch(1.0, 1.0, 2.0, 2.0, PatchFormat::RawRgba, raw(2, 2, RED)),
        ),
        region(
            "b",
            patch(5.0, 5.0, 3.0, 2.0, PatchFormat::Png, png(3, 2, BLUE)),
        ),
    ];
    let (out, warnings) = apply_erase(&base(), &regions);
    assert!(warnings.is_empty());
    assert_eq!(*out.get_pixel(1, 1), RED);
    assert_eq!(*out.get_pixel(2, 2), RED);
    assert_eq!(*out.get_pixel(3, 3), Rgba([255; 4]));
    assert_eq!(*out.get_pixel(7, 6), BLUE);
}

#[test]
fn none_region_leaves_image_untouched() {
    let (out, warnings) = apply_erase(&base(), &[region("a", EraseSpec::None)]);
    assert!(warnings.is_empty());
    assert_eq!(out, base());
}

#[test]
fn patch_partly_outside_is_clipped() {
    let spec = patch(8.0, 8.0, 4.0, 4.0, PatchFormat::RawRgba, raw(4, 4, RED));
    let (out, warnings) = apply_erase(&base(), &[region("a", spec)]);
    assert!(warnings.is_empty());
    assert_eq!(*out.get_pixel(9, 9), RED);
    assert_eq!(*out.get_pixel(7, 7), Rgba([255; 4]));
}

#[test]
fn patch_fully_outside_or_negative_is_reported() {
    let far = patch(20.0, 20.0, 2.0, 2.0, PatchFormat::RawRgba, raw(2, 2, RED));
    let neg = patch(-1.0, 0.0, 2.0, 2.0, PatchFormat::RawRgba, raw(2, 2, RED));
    let nan = patch(
        f32::NAN,
        0.0,
        2.0,
        2.0,
        PatchFormat::RawRgba,
        raw(2, 2, RED),
    );
    let (out, warnings) = apply_erase(
        &base(),
        &[region("a", far), region("b", neg), region("c", nan)],
    );
    assert_eq!(out, base());
    assert_eq!(warnings.len(), 3);
    assert!(warnings
        .iter()
        .all(|w| matches!(w, ApplyWarning::OutOfBounds { .. })));
}

#[test]
fn wrong_raw_length_is_reported() {
    let spec = patch(0.0, 0.0, 2.0, 2.0, PatchFormat::RawRgba, vec![0; 7]);
    let (out, warnings) = apply_erase(&base(), &[region("a", spec)]);
    assert_eq!(out, base());
    assert_eq!(
        warnings,
        vec![ApplyWarning::SizeMismatch {
            region_id: "a".into(),
            expected: 16,
            actual: 7
        }]
    );
}

#[test]
fn zero_sized_raw_patch_is_reported_not_panicking() {
    let spec = patch(0.0, 0.0, 0.0, 0.0, PatchFormat::RawRgba, vec![]);
    let (_, warnings) = apply_erase(&base(), &[region("a", spec)]);
    assert_eq!(warnings.len(), 1);
}

#[test]
fn corrupt_png_is_reported_and_others_still_apply() {
    let bad = patch(0.0, 0.0, 2.0, 2.0, PatchFormat::Png, vec![137, 80, 1]);
    let good = patch(4.0, 4.0, 1.0, 1.0, PatchFormat::RawRgba, raw(1, 1, RED));
    let (out, warnings) = apply_erase(&base(), &[region("bad", bad), region("ok", good)]);
    assert_eq!(
        warnings,
        vec![ApplyWarning::UndecodablePng {
            region_id: "bad".into()
        }]
    );
    assert_eq!(*out.get_pixel(4, 4), RED);
}
