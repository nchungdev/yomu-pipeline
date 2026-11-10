//! Per-region contour selection and erasing. Erasing is only legal once the translated text is
//! known to fit: callers lay out first, then call [`erase_region`].

use image::RgbaImage;

use super::flat_fill::{InpaintResult, StatisticalFlatFill};
use crate::contour::{BubbleContour, ContourDetectionOptions, DEFAULT_INSET};
use crate::contract::v1::SkipReason;
use crate::types::{BubbleStyleHint, NormalizedRect, RegionInput};

/// Where a region contour came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContourSource {
    /// A bright bubble outline was found in the image.
    Detected,
    /// No outline was found (or the hint forbids detection); a rectangle from the bounds is used.
    BoundsFallback,
}

/// Picks the contour for a region: a detected bubble when possible, else the bounds rectangle.
pub fn region_contour(
    img: &RgbaImage,
    region: &RegionInput,
    bounds: NormalizedRect,
) -> Option<(BubbleContour, ContourSource)> {
    let detect = !matches!(
        region.style_hint,
        BubbleStyleHint::DarkBalloon | BubbleStyleHint::FloatingText
    );
    let detected = detect
        .then(|| BubbleContour::detect_best(img, bounds, ContourDetectionOptions::default()))
        .flatten();
    match detected {
        Some(c) => Some((c, ContourSource::Detected)),
        None => BubbleContour::from_bounds(img, bounds).map(|c| (c, ContourSource::BoundsFallback)),
    }
}

/// Erases the source text of one region on `clean`. Flat backgrounds are filled; otherwise a
/// detected bubble is smart-inpainted. Without a detected outline a complex background cannot be
/// erased cleanly, so the region is left untouched and `ComplexBackground` is returned.
pub fn erase_region(
    clean: &mut RgbaImage,
    bounds: NormalizedRect,
    contour: &BubbleContour,
    source: ContourSource,
) -> Result<(), SkipReason> {
    match StatisticalFlatFill::default().inpaint_rgba_image(clean, bounds) {
        InpaintResult::FlatFilled => Ok(()),
        InpaintResult::ComplexBackground if source == ContourSource::Detected => {
            contour.smart_inpaint(clean, DEFAULT_INSET, None);
            Ok(())
        }
        InpaintResult::ComplexBackground => Err(SkipReason::ComplexBackground),
    }
}
