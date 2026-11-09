//! Bubble contour detection, bounding box calculation, and line slice computations.

use image::{Rgba, RgbaImage};
use std::collections::VecDeque;

use crate::mask::{anchor_spans, fill_enclosed_holes, longest_run};
use crate::types::NormalizedRect;

pub const DEFAULT_LUMINANCE_THRESHOLD: f32 = 0.70;
pub const DEFAULT_INSET: u32 = 3;
pub const DEFAULT_MIN_SLICE_WIDTH: f32 = 40.0;
pub const DEFAULT_H_PADDING: f32 = 15.0;
pub const DEFAULT_MARGIN_RATIO: f32 = 0.02;
/// Rows above/below the text box (as a fraction of its height) still usable for layout.
pub const ANCHOR_PAD_RATIO: f32 = 0.15;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContourDetectionOptions {
    pub luminance_threshold: f32,
    pub min_box_size: u32,
}

impl Default for ContourDetectionOptions {
    fn default() -> Self {
        Self {
            luminance_threshold: DEFAULT_LUMINANCE_THRESHOLD,
            min_box_size: 5,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BubbleContour {
    pub img_w: u32,
    pub img_h: u32,
    pub box_x: u32,
    pub box_y: u32,
    pub box_w: u32,
    pub box_h: u32,
    pub min_y: u32,
    pub max_y: u32,
    pub row_spans: Vec<Option<(u32, u32)>>,
    pub seed_color: Rgba<u8>,
}

#[inline]
pub fn pixel_luminance(p: &Rgba<u8>) -> f32 {
    0.299 * (p[0] as f32 / 255.0) + 0.587 * (p[1] as f32 / 255.0) + 0.114 * (p[2] as f32 / 255.0)
}

impl BubbleContour {
    pub fn detect(
        image: &RgbaImage,
        b: NormalizedRect,
        o: ContourDetectionOptions,
    ) -> Option<Self> {
        let (bx, by, bw, bh) =
            Self::pixel_rect_coords(image.width(), image.height(), b, 0.0, o.min_box_size)?;
        Self::detect_in_pixel_rect(image, bx, by, bw, bh, o)
    }

    pub fn detect_with_margin(
        image: &RgbaImage,
        b: NormalizedRect,
        m: f32,
        o: ContourDetectionOptions,
    ) -> Option<Self> {
        let (bx, by, bw, bh) =
            Self::pixel_rect_coords(image.width(), image.height(), b, m, o.min_box_size)?;
        let c = Self::detect_in_pixel_rect(image, bx, by, bw, bh, o)?;
        let area: u32 = c.row_spans.iter().flatten().map(|(l, r)| r - l + 1).sum();
        (area >= ((bw * bh) / 6).max(16)).then_some(c)
    }

    pub fn detect_anchored(
        image: &RgbaImage,
        b: NormalizedRect,
        m: f32,
        o: ContourDetectionOptions,
    ) -> Option<Self> {
        let (w, h) = (image.width(), image.height());
        let (bx, by, bw, bh) = Self::pixel_rect_coords(w, h, b, m, o.min_box_size)?;
        let detected = Self::detect_in_pixel_rect(image, bx, by, bw, bh, o)?;
        let area: u32 = detected
            .row_spans
            .iter()
            .flatten()
            .map(|(l, r)| r - l + 1)
            .sum();
        if area < ((bw * bh) / 6).max(16) {
            return None;
        }

        let (_, ry, _, rh) = Self::pixel_rect_coords(w, h, b, 0.0, 1)?;
        let pad = (rh as f32 * ANCHOR_PAD_RATIO) as u32;
        let (y0, y1) = (
            ry.saturating_sub(pad).saturating_sub(by),
            ry + rh + pad - by,
        );
        let mut row_spans = detected.row_spans;
        anchor_spans(
            &mut row_spans,
            (y0 as usize, y1 as usize),
            (ry + rh / 2).saturating_sub(by) as usize,
            (rh / 4) as usize,
        );
        let min_y = row_spans.iter().position(Option::is_some)? as u32;
        let max_y = row_spans.iter().rposition(Option::is_some)? as u32;
        Some(Self {
            img_w: w,
            img_h: h,
            box_x: bx,
            box_y: by,
            box_w: bw,
            box_h: bh,
            min_y,
            max_y,
            row_spans,
            seed_color: detected.seed_color,
        })
    }

    pub fn detect_best(
        image: &RgbaImage,
        b: NormalizedRect,
        o: ContourDetectionOptions,
    ) -> Option<Self> {
        Self::detect_anchored(image, b, DEFAULT_MARGIN_RATIO, o)
            .or_else(|| Self::detect_anchored(image, b, 0.0, o))
    }

    pub fn pixel_rect_coords(
        w: u32,
        h: u32,
        b: NormalizedRect,
        margin: f32,
        min_sz: u32,
    ) -> Option<(u32, u32, u32, u32)> {
        if w == 0 || h == 0 {
            return None;
        }
        let (mx, my) = (
            ((w as f32) * margin.max(0.0)).round() as u32,
            ((h as f32) * margin.max(0.0)).round() as u32,
        );
        let rx = ((b.x() * w as f32).round() as u32).min(w - 1);
        let ry = ((b.y() * h as f32).round() as u32).min(h - 1);
        let rw = ((b.width() * w as f32).round() as u32).clamp(1, w - rx);
        let rh = ((b.height() * h as f32).round() as u32).clamp(1, h - ry);
        let bx = rx.saturating_sub(mx);
        let by = ry.saturating_sub(my);
        let bw = (rx + rw + mx).min(w).saturating_sub(bx).max(1);
        let bh = (ry + rh + my).min(h).saturating_sub(by).max(1);
        (bw >= min_sz && bh >= min_sz).then_some((bx, by, bw, bh))
    }

    pub fn detect_in_pixel_rect(
        image: &RgbaImage,
        box_x: u32,
        box_y: u32,
        box_w: u32,
        box_h: u32,
        opts: ContourDetectionOptions,
    ) -> Option<Self> {
        let (w, h) = (box_w as usize, box_h as usize);
        let bright = |x: usize, y: usize| {
            pixel_luminance(image.get_pixel(box_x + x as u32, box_y + y as u32))
                > opts.luminance_threshold
        };
        let mut visited = vec![false; w * h];
        let mut mask = vec![false; w * h];
        let mut queue = VecDeque::new();
        let mut best_seed_px = None;
        let mut max_lum = opts.luminance_threshold;

        let (start_y, end_y) = ((h / 10).max(1), ((h * 9) / 10).min(h - 1));
        let (start_x, end_x) = ((w / 10).max(1), ((w * 9) / 10).min(w - 1));
        for sy in start_y..=end_y {
            for sx in start_x..=end_x {
                if visited[sy * w + sx] || !bright(sx, sy) {
                    continue;
                }
                visited[sy * w + sx] = true;
                queue.push_back((sx, sy));
                let mut component = Vec::new();
                while let Some((cx, cy)) = queue.pop_front() {
                    let cur_px = *image.get_pixel(box_x + cx as u32, box_y + cy as u32);
                    let cur_lum = pixel_luminance(&cur_px);
                    if cur_lum > max_lum {
                        max_lum = cur_lum;
                        best_seed_px = Some(cur_px);
                    }
                    component.push(cy * w + cx);
                    for (nx, ny) in [
                        (cx.wrapping_sub(1), cy),
                        (cx + 1, cy),
                        (cx, cy.wrapping_sub(1)),
                        (cx, cy + 1),
                    ] {
                        if nx < w && ny < h && !visited[ny * w + nx] && bright(nx, ny) {
                            visited[ny * w + nx] = true;
                            queue.push_back((nx, ny));
                        }
                    }
                }
                if component.len() >= 16 {
                    component.into_iter().for_each(|i| mask[i] = true);
                }
            }
        }
        if mask.iter().filter(|&&m| m).count() < 16 {
            return None;
        }

        let filled = fill_enclosed_holes(&mask, w, h);
        let row_spans: Vec<Option<(u32, u32)>> = filled.chunks(w).map(longest_run).collect();
        let total_min_y = row_spans.iter().position(Option::is_some)? as u32;
        let total_max_y = row_spans.iter().rposition(Option::is_some)? as u32;
        let seed_color = best_seed_px.unwrap_or(Rgba([255, 255, 255, 255]));
        Some(Self {
            img_w: image.width(),
            img_h: image.height(),
            box_x,
            box_y,
            box_w,
            box_h,
            min_y: total_min_y,
            max_y: total_max_y,
            row_spans,
            seed_color,
        })
    }

    pub fn smart_inpaint(
        &self,
        image: &mut RgbaImage,
        inset: u32,
        fill: Option<Rgba<u8>>,
    ) -> usize {
        let color = fill.unwrap_or(self.seed_color);
        let mut filled = 0;
        for y in (self.min_y + 2).min(self.max_y)..=self.max_y.saturating_sub(2) {
            if let Some(&(rmin, rmax)) = self.row_spans.get(y as usize).and_then(Option::as_ref) {
                if rmax > rmin + 2 * inset {
                    for x in (rmin + inset)..=(rmax - inset) {
                        let (px, py) = (self.box_x + x, self.box_y + y);
                        if px < image.width() && py < image.height() {
                            image.put_pixel(px, py, color);
                            filled += 1;
                        }
                    }
                }
            }
        }
        filled
    }

    pub fn compute_line_slices(
        &self,
        height: f32,
        min_w: f32,
        h_pad: f32,
    ) -> Vec<crate::layout::TextLineSlice> {
        crate::layout::TextLineSlice::compute_slices(
            self.box_x,
            self.box_y,
            self.min_y,
            self.max_y,
            &self.row_spans,
            height,
            min_w,
            h_pad,
        )
    }

    pub fn fit_text_lines_centered(
        &self,
        text: &str,
        all_slices: &[crate::layout::TextLineSlice],
        measure_fn: impl Fn(&str) -> f32,
    ) -> Option<Vec<crate::layout::FittedLine>> {
        crate::layout::TextLineSlice::fit_text_lines_centered(text, all_slices, measure_fn)
    }

    pub fn fit_text_lines(
        &self,
        text: &str,
        slices: &[crate::layout::TextLineSlice],
        measure_fn: impl Fn(&str) -> f32,
    ) -> Vec<crate::layout::FittedLine> {
        crate::layout::TextLineSlice::fit_text_lines(text, slices, measure_fn)
    }
}
