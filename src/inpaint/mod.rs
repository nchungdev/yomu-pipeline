//! Inpainting algorithms: flat fill and smart inpaint with contour preservation.

pub mod flat_fill;
pub use flat_fill::*;

use image::{Rgba, RgbaImage};

use crate::contour::{BubbleContour, ContourDetectionOptions, DEFAULT_INSET};
use crate::types::{BubbleStyleHint, InpaintedPage, NormalizedRect, RegionInput};

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

fn is_non_standard_region(r: &RegionInput) -> bool {
    if r.style_hint == BubbleStyleHint::FloatingText {
        return true;
    }
    let id = r.id.trim().to_ascii_lowercase();
    let text = r.text.trim();
    ["float", "sfx"].iter().any(|k| id.contains(k))
        || ["*", "~", "[float]"].iter().any(|p| text.starts_with(p))
}

fn is_dark_region(r: &RegionInput) -> bool {
    if r.style_hint == BubbleStyleHint::DarkBalloon {
        return true;
    }
    let id = r.id.trim().to_ascii_lowercase();
    id.starts_with('d') || ["dark", "black"].iter().any(|k| id.contains(k))
}

pub fn inpaint_page(img: &RgbaImage, regions: &[RegionInput]) -> InpaintedPage {
    let mut clean = img.clone();
    let opts = ContourDetectionOptions::default();
    let flat_fill = StatisticalFlatFill::default();

    let contours: Vec<Option<BubbleContour>> = regions
        .iter()
        .enumerate()
        .map(|(idx, r)| {
            let bounded_box = clamp_rect_against_neighbors(r.bounds, idx, regions);
            let fill_res = flat_fill.inpaint_rgba_image(&mut clean, bounded_box);
            let non_standard = is_non_standard_region(r);

            if fill_res == InpaintResult::FlatFilled {
                (!non_standard).then(|| BubbleContour::detect_best(img, bounded_box, opts))?
            } else {
                let is_bright = !is_dark_region(r) && !non_standard;
                let contour =
                    is_bright.then(|| BubbleContour::detect_best(img, bounded_box, opts))??;
                contour.smart_inpaint(&mut clean, DEFAULT_INSET, None);
                Some(contour)
            }
        })
        .collect();

    InpaintedPage {
        image: clean,
        contours,
    }
}
