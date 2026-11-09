//! Statistical flat fill inpainting for solid/uniform background regions.

use image::{Rgba, RgbaImage};
use std::collections::VecDeque;

use crate::contour::BubbleContour;
use crate::types::NormalizedRect;

pub const FLAT_FILL_EDGE_MARGIN: usize = 3;
pub const FLAT_FILL_DEFAULT_MARGIN_RATIO: f32 = 0.02;
pub const FLAT_FILL_MIN_SAMPLE_PIXELS: usize = 16;
pub const FLAT_FILL_MAX_STD: f32 = 10.0;
pub const FLAT_FILL_SATURATED_STD: f32 = 7.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InpaintResult {
    FlatFilled,
    ComplexBackground,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatisticalFlatFill {
    pub margin_ratio: f32,
    pub edge_margin: usize,
    pub min_samples: usize,
    pub max_std: f32,
    pub saturated_std: f32,
}

impl Default for StatisticalFlatFill {
    fn default() -> Self {
        Self {
            margin_ratio: FLAT_FILL_DEFAULT_MARGIN_RATIO,
            edge_margin: FLAT_FILL_EDGE_MARGIN,
            min_samples: FLAT_FILL_MIN_SAMPLE_PIXELS,
            max_std: FLAT_FILL_MAX_STD,
            saturated_std: FLAT_FILL_SATURATED_STD,
        }
    }
}

impl StatisticalFlatFill {
    pub fn inpaint_rgba_image(
        &self,
        img: &mut RgbaImage,
        norm_box: NormalizedRect,
    ) -> InpaintResult {
        let (w, h) = (img.width(), img.height());
        let Some((bx, by, bw, bh)) =
            BubbleContour::pixel_rect_coords(w, h, norm_box, self.margin_ratio, 5)
        else {
            return InpaintResult::ComplexBackground;
        };

        let total = (bw * bh) as usize;
        let mut rgb_buf = Vec::with_capacity(total);
        let mut lum_buf = Vec::with_capacity(total);
        let (mut min_l, mut max_l) = (255u8, 0u8);

        for cy in 0..bh {
            for cx in 0..bw {
                let p = img.get_pixel(bx + cx, by + cy);
                let rgb = [p[0], p[1], p[2]];
                let lum =
                    (0.299 * rgb[0] as f32 + 0.587 * rgb[1] as f32 + 0.114 * rgb[2] as f32) as u8;
                min_l = min_l.min(lum);
                max_l = max_l.max(lum);
                rgb_buf.push(rgb);
                lum_buf.push(lum);
            }
        }

        let otsu_t = compute_otsu_threshold(&lum_buf);
        let has_contrast = max_l.saturating_sub(min_l) >= 15;
        let dark_count = lum_buf.iter().filter(|&&l| l <= otsu_t).count();
        let dark_is_fg = dark_count < total / 2;

        let cand_ink: Vec<bool> = if has_contrast {
            lum_buf
                .iter()
                .map(|&l| if dark_is_fg { l <= otsu_t } else { l > otsu_t })
                .collect()
        } else {
            vec![false; total]
        };

        let mut border_mask = vec![false; total];
        let mut q = VecDeque::new();
        for cy in 0..bh {
            for cx in 0..bw {
                if cx == 0 || cy == 0 || cx == bw - 1 || cy == bh - 1 {
                    let idx = (cy * bw + cx) as usize;
                    if cand_ink[idx] && !border_mask[idx] {
                        border_mask[idx] = true;
                        q.push_back((cx, cy));
                    }
                }
            }
        }

        let n8 = [
            (-1, 0),
            (1, 0),
            (0, -1),
            (0, 1),
            (-1, -1),
            (1, -1),
            (-1, 1),
            (1, 1),
        ];
        while let Some((x, y)) = q.pop_front() {
            for (dx, dy) in n8 {
                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                if nx >= 0 && nx < bw as i32 && ny >= 0 && ny < bh as i32 {
                    let n_idx = (ny as u32 * bw + nx as u32) as usize;
                    if cand_ink[n_idx] && !border_mask[n_idx] {
                        border_mask[n_idx] = true;
                        q.push_back((nx as u32, ny as u32));
                    }
                }
            }
        }

        let is_ink: Vec<bool> = (0..total).map(|i| cand_ink[i] && !border_mask[i]).collect();
        let em = self.edge_margin as i32;
        let mut bg_samples = Vec::new();

        if (bw as i32) > em * 2 && (bh as i32) > em * 2 {
            for cy in em..(bh as i32 - em) {
                for cx in em..(bw as i32 - em) {
                    let is_bg = (-em..=em).all(|dy| {
                        (-em..=em).all(|dx| {
                            !cand_ink[((cy + dy) as u32 * bw + (cx + dx) as u32) as usize]
                        })
                    });
                    if is_bg {
                        bg_samples.push(rgb_buf[(cy as u32 * bw + cx as u32) as usize]);
                    }
                }
            }
        }

        let n = bg_samples.len();
        if n < self.min_samples {
            return InpaintResult::ComplexBackground;
        }

        let (mut r_v, mut g_v, mut b_v) = (
            bg_samples.iter().map(|p| p[0]).collect::<Vec<_>>(),
            bg_samples.iter().map(|p| p[1]).collect::<Vec<_>>(),
            bg_samples.iter().map(|p| p[2]).collect::<Vec<_>>(),
        );
        r_v.sort_unstable();
        g_v.sort_unstable();
        b_v.sort_unstable();

        let (mr, mg, mb) = (r_v[n / 2], g_v[n / 2], b_v[n / 2]);
        let std_fn = |v: &[u8], m: u8| -> f32 {
            (v.iter()
                .map(|&x| (x as f32 - m as f32).powi(2))
                .sum::<f32>()
                / n as f32)
                .sqrt()
        };
        let (sr, sg, sb) = (std_fn(&r_v, mr), std_fn(&g_v, mg), std_fn(&b_v, mb));
        let max_s = sr.max(sg).max(sb);
        let min_s = sr.min(sg).min(sb);
        let thresh = if max_s - min_s > 1.0 {
            self.saturated_std
        } else {
            self.max_std
        };
        if max_s > thresh {
            return InpaintResult::ComplexBackground;
        }

        let fill = [mr, mg, mb];
        for cy in 0..bh {
            for cx in 0..bw {
                let idx = (cy * bw + cx) as usize;
                if is_ink[idx] {
                    img.put_pixel(bx + cx, by + cy, Rgba([fill[0], fill[1], fill[2], 255]));
                } else if !border_mask[idx] && has_ink_neighbor(&is_ink, cx, cy, bw, bh) {
                    let c = rgb_buf[idx];
                    let diff = (c[0] as i32 - mr as i32)
                        .abs()
                        .max((c[1] as i32 - mg as i32).abs())
                        .max((c[2] as i32 - mb as i32).abs());
                    if diff > 20 {
                        img.put_pixel(bx + cx, by + cy, Rgba([fill[0], fill[1], fill[2], 255]));
                    }
                }
            }
        }
        InpaintResult::FlatFilled
    }
}

fn has_ink_neighbor(is_ink: &[bool], cx: u32, cy: u32, bw: u32, bh: u32) -> bool {
    let (cx, cy, bw, bh) = (cx as i32, cy as i32, bw as i32, bh as i32);
    [(-1, 0), (1, 0), (0, -1), (0, 1)]
        .into_iter()
        .any(|(dx, dy)| {
            let (nx, ny) = (cx + dx, cy + dy);
            nx >= 0
                && nx < bw
                && ny >= 0
                && ny < bh
                && is_ink[(ny as u32 * bw as u32 + nx as u32) as usize]
        })
}

pub fn compute_otsu_threshold(lum: &[u8]) -> u8 {
    let mut hist = [0u32; 256];
    for &l in lum {
        hist[l as usize] += 1;
    }
    let total = lum.len() as f32;
    if total == 0.0 {
        return 128;
    }
    let sum_all: f32 = hist
        .iter()
        .enumerate()
        .map(|(i, &c)| (i as f32) * (c as f32))
        .sum();
    let (mut sum_b, mut w_b, mut max_var, mut best_t) = (0.0, 0.0, 0.0, 128u8);

    for (t, &count) in hist.iter().enumerate() {
        let count_f = count as f32;
        w_b += count_f;
        if w_b == 0.0 {
            continue;
        }
        let w_f = total - w_b;
        if w_f == 0.0 {
            break;
        }
        sum_b += (t as f32) * count_f;
        let (m_b, m_f) = (sum_b / w_b, (sum_all - sum_b) / w_f);
        let var = w_b * w_f * (m_b - m_f).powi(2);
        if var > max_var {
            max_var = var;
            best_t = t as u8;
        }
    }
    best_t
}
