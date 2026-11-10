//! Pixel extent of what erasing one region actually changed; used to cut exact erase patches.

use image::RgbaImage;

use super::flat_fill::StatisticalFlatFill;
use crate::contour::BubbleContour;
use crate::types::NormalizedRect;

/// Extra pixels around a patch so anti-aliased edges are covered.
pub const PATCH_MARGIN_PX: u32 = 2;

/// Axis-aligned pixel box (`x`, `y` top-left; never empty).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelBox {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl PixelBox {
    /// Smallest box covering both.
    pub fn union(self, o: Self) -> Self {
        let (x, y) = (self.x.min(o.x), self.y.min(o.y));
        let right = (self.x + self.w).max(o.x + o.w);
        let bottom = (self.y + self.h).max(o.y + o.h);
        Self {
            x,
            y,
            w: right - x,
            h: bottom - y,
        }
    }

    /// Grows by `m` on every side, clamped to a `width` x `height` image.
    pub fn expand_clamped(self, m: u32, width: u32, height: u32) -> Self {
        let x = self.x.saturating_sub(m);
        let y = self.y.saturating_sub(m);
        let right = (self.x + self.w).saturating_add(m).min(width);
        let bottom = (self.y + self.h).saturating_add(m).min(height);
        Self {
            x,
            y,
            w: right.saturating_sub(x).max(1),
            h: bottom.saturating_sub(y).max(1),
        }
    }
}

/// Box of the contour in pixels of its image.
pub fn contour_box(c: &BubbleContour) -> PixelBox {
    PixelBox {
        x: c.box_x,
        y: c.box_y,
        w: c.box_w.max(1),
        h: c.box_h.max(1),
    }
}

/// Bounding box of every pixel where `clean` differs from `original`, searched only where an
/// erase of `bounds`/`contour` can write (flat-fill rectangle and contour box).
/// `None` when nothing changed. Images of different sizes yield `None`.
pub fn erase_extent(
    original: &RgbaImage,
    clean: &RgbaImage,
    bounds: NormalizedRect,
    contour: &BubbleContour,
) -> Option<PixelBox> {
    if original.dimensions() != clean.dimensions() {
        return None;
    }
    let (w, h) = clean.dimensions();
    let margin = StatisticalFlatFill::default().margin_ratio;
    let mut area = contour_box(contour);
    if let Some((x, y, bw, bh)) = BubbleContour::pixel_rect_coords(w, h, bounds, margin, 1) {
        area = area.union(PixelBox { x, y, w: bw, h: bh });
    }
    let right = (area.x + area.w).min(w);
    let bottom = (area.y + area.h).min(h);
    let mut found: Option<(u32, u32, u32, u32)> = None;
    for y in area.y..bottom {
        for x in area.x..right {
            if original.get_pixel(x, y) != clean.get_pixel(x, y) {
                found = Some(match found {
                    None => (x, y, x, y),
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                });
            }
        }
    }
    found.map(|(x0, y0, x1, y1)| PixelBox {
        x: x0,
        y: y0,
        w: x1 - x0 + 1,
        h: y1 - y0 + 1,
    })
}
