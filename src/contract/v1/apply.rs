//! Client-side helper: build the clean full-page image from the original and the region patches.

use image::{ImageFormat as CodecFormat, RgbaImage};

use super::region::{EraseSpec, PatchFormat, RegionResult};

/// A patch that could not be applied; the rest of the page is still produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyWarning {
    /// Raw data length differs from `w * h * 4`.
    SizeMismatch {
        region_id: String,
        expected: usize,
        actual: usize,
    },
    /// The PNG bytes could not be decoded.
    UndecodablePng { region_id: String },
    /// The patch rectangle is empty, non-finite or entirely outside the image.
    OutOfBounds { region_id: String },
}

/// Returns a copy of `original` with every `Patch` pasted at its `rect_px`.
///
/// `None` specs are skipped. Patches partly outside the image are clipped; patches fully outside,
/// size-mismatched raw data and undecodable PNGs are skipped and reported. Never panics.
pub fn apply_erase(
    original: &RgbaImage,
    regions: &[RegionResult],
) -> (RgbaImage, Vec<ApplyWarning>) {
    let mut out = original.clone();
    let mut warnings = Vec::new();
    for region in regions {
        let EraseSpec::Patch {
            rect_px,
            format,
            data,
            ..
        } = &region.erase
        else {
            continue;
        };
        let id = region.region_id.clone();
        let (x, y) = (rect_px.x, rect_px.y);
        if !(x.is_finite() && y.is_finite() && x >= 0.0 && y >= 0.0) {
            warnings.push(ApplyWarning::OutOfBounds { region_id: id });
            continue;
        }
        let patch = match decode(*format, data, rect_px.w, rect_px.h) {
            Ok(p) => p,
            Err(kind) => {
                warnings.push(kind.into_warning(id));
                continue;
            }
        };
        let (x, y) = (x.round() as u32, y.round() as u32);
        if patch.width() == 0 || patch.height() == 0 || x >= out.width() || y >= out.height() {
            warnings.push(ApplyWarning::OutOfBounds { region_id: id });
            continue;
        }
        let w = patch.width().min(out.width() - x);
        let h = patch.height().min(out.height() - y);
        for row in 0..h {
            for col in 0..w {
                out.put_pixel(x + col, y + row, *patch.get_pixel(col, row));
            }
        }
    }
    (out, warnings)
}

enum DecodeError {
    Size { expected: usize, actual: usize },
    Png,
    Empty,
}

impl DecodeError {
    fn into_warning(self, region_id: String) -> ApplyWarning {
        match self {
            Self::Size { expected, actual } => ApplyWarning::SizeMismatch {
                region_id,
                expected,
                actual,
            },
            Self::Png => ApplyWarning::UndecodablePng { region_id },
            Self::Empty => ApplyWarning::OutOfBounds { region_id },
        }
    }
}

fn decode(format: PatchFormat, data: &[u8], w: f32, h: f32) -> Result<RgbaImage, DecodeError> {
    match format {
        PatchFormat::Png => image::load_from_memory_with_format(data, CodecFormat::Png)
            .map(|i| i.to_rgba8())
            .map_err(|_| DecodeError::Png),
        PatchFormat::RawRgba => {
            if !(w.is_finite() && h.is_finite() && w >= 1.0 && h >= 1.0) {
                return Err(DecodeError::Empty);
            }
            let (pw, ph) = (w.round() as u32, h.round() as u32);
            let expected = (pw as usize).saturating_mul(ph as usize).saturating_mul(4);
            if data.len() != expected {
                return Err(DecodeError::Size {
                    expected,
                    actual: data.len(),
                });
            }
            RgbaImage::from_raw(pw, ph, data.to_vec()).ok_or(DecodeError::Size {
                expected,
                actual: data.len(),
            })
        }
    }
}
