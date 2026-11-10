//! Inpainting algorithms: flat fill and smart inpaint with contour preservation.

pub mod extent;
pub mod flat_fill;
pub mod region;
pub use flat_fill::*;
pub use region::{erase_region, region_contour, ContourSource};

use image::{Rgba, RgbaImage};

use crate::contour::BubbleContour;
use crate::contract::v1::RegionKind;
use crate::types::{InpaintedPage, NormalizedRect, RegionInput};

pub fn clamp_rect_against_neighbors(
    bounds: NormalizedRect,
    idx: usize,
    all_bubbles: &[RegionInput],
) -> NormalizedRect {
    let (mut x, mut y, w, h) = (bounds.x(), bounds.y(), bounds.width(), bounds.height());
    let mut right = x + w;
    let mut bottom = y + h;
    let (cx, cy) = (x + w * 0.5, y + h * 0.5);

    for (j, other) in all_bubbles.iter().enumerate() {
        if j == idx {
            continue;
        }
        let ob = other.bounds;
        let (ocx, ocy) = (ob.x() + ob.width() * 0.5, ob.y() + ob.height() * 0.5);

        let x_overlap = x.max(ob.x()) < right.min(ob.x() + ob.width());
        if x_overlap {
            let (v_gap, is_below) = if ocy > cy {
                (ob.y() - bottom, true)
            } else {
                (y - (ob.y() + ob.height()), false)
            };
            if (-0.02..=0.035).contains(&v_gap) {
                if is_below {
                    bottom = bottom.min(ob.y() + 0.005);
                } else {
                    y = y.max(ob.y() + ob.height() - 0.005);
                }
            }
        }

        let y_overlap = y.max(ob.y()) < bottom.min(ob.y() + ob.height());
        if y_overlap {
            let (h_gap, is_right) = if ocx > cx {
                (ob.x() - right, true)
            } else {
                (x - (ob.x() + ob.width()), false)
            };
            if (-0.02..=0.035).contains(&h_gap) {
                if is_right {
                    right = right.min(ob.x() + 0.005);
                } else {
                    x = x.max(ob.x() + ob.width() - 0.005);
                }
            }
        }
    }
    let new_w = (right - x).max(0.01);
    let new_h = (bottom - y).max(0.01);
    NormalizedRect::new(x, y, new_w, new_h).unwrap_or(bounds)
}

pub fn smart_inpaint(
    contour: &BubbleContour,
    image: &mut RgbaImage,
    inset: u32,
    fill: Option<Rgba<u8>>,
) -> usize {
    contour.smart_inpaint(image, inset, fill)
}

/// Erases the source text of every eligible region and returns the contours used.
///
/// Regions of kind `Sfx` or with blank text are left untouched. This function does **not** check
/// that the translated text fits; use [`crate::process_page`], which lays out before erasing, when
/// the text must be drawn afterwards. A region whose background cannot be erased cleanly has a
/// `None` contour and is left untouched.
pub fn inpaint_page(img: &RgbaImage, regions: &[RegionInput]) -> InpaintedPage {
    let mut clean = img.clone();
    let contours = regions
        .iter()
        .enumerate()
        .map(|(idx, r)| {
            if r.kind == RegionKind::Sfx || r.text.trim().is_empty() {
                return None;
            }
            let bounds = clamp_rect_against_neighbors(r.bounds, idx, regions);
            let (contour, source) = region_contour(img, r, bounds)?;
            erase_region(&mut clean, bounds, &contour, source).ok()?;
            Some(contour)
        })
        .collect();

    InpaintedPage {
        image: clean,
        contours,
    }
}
