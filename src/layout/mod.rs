//! Dynamic programming text wrapping and layout within contour line slices.

pub mod dp;
pub use dp::*;

use serde::{Deserialize, Serialize};

use crate::contour::BubbleContour;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TextLineSlice {
    pub y: f32,
    pub center_x: f32,
    pub left_x: f32,
    pub right_x: f32,
    pub available_width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FittedLine {
    pub text: String,
    pub slice: TextLineSlice,
    pub line_width: f32,
    pub start_x: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextLayoutLine {
    pub text: String,
    pub slice_center_x: f32,
    pub slice_y: f32,
    pub slice_width: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextLayout {
    pub font_px: f32,
    pub lines: Vec<TextLayoutLine>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FontRange {
    pub min_px: f32,
    pub max_px: f32,
    pub step_px: f32,
}

impl Default for FontRange {
    fn default() -> Self {
        Self {
            min_px: 6.0,
            max_px: 48.0,
            step_px: 0.5,
        }
    }
}

impl TextLineSlice {
    #[allow(clippy::too_many_arguments)]
    pub fn compute_slices(
        box_x: u32,
        box_y: u32,
        min_y: u32,
        max_y: u32,
        row_spans: &[Option<(u32, u32)>],
        height: f32,
        min_w: f32,
        h_pad: f32,
    ) -> Vec<Self> {
        let mut slices = Vec::new();
        if height <= 0.0 || max_y <= min_y {
            return slices;
        }
        let (mut curr_y, max_y_f) = (min_y as f32 + height * 0.80, max_y as f32 - height * 0.60);
        while curr_y <= max_y_f {
            if let Some(&(rmin, rmax)) = row_spans
                .get(curr_y.round() as usize)
                .and_then(Option::as_ref)
            {
                let (abs_l, abs_r) = (box_x as f32 + rmin as f32, box_x as f32 + rmax as f32);
                let edge_dist = (curr_y - min_y as f32).min(max_y as f32 - curr_y);
                let curve_pad = if edge_dist < height * 1.5 {
                    (h_pad * 0.35).max(3.0)
                } else {
                    0.0
                };
                let effective_pad = (h_pad + curve_pad).max(4.0);
                let available_width = (abs_r - abs_l) - 2.0 * effective_pad;
                if available_width >= min_w {
                    let (y, center_x) = (box_y as f32 + curr_y, (abs_l + abs_r) * 0.5);
                    let (left_x, right_x) = (abs_l + effective_pad, abs_r - effective_pad);
                    slices.push(Self {
                        y,
                        center_x,
                        left_x,
                        right_x,
                        available_width,
                        height,
                    });
                }
            }
            curr_y += height;
        }
        slices
    }

    pub fn fit_text_lines_centered(
        text: &str,
        all_slices: &[Self],
        measure_fn: impl Fn(&str) -> f32,
    ) -> Option<Vec<FittedLine>> {
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.is_empty() || all_slices.is_empty() {
            return None;
        }

        let mut best_fitted: Option<(Vec<FittedLine>, f64)> = None;
        for k in 1..=all_slices.len().min(words.len()) {
            let start = (all_slices.len() - k) / 2;
            if let Some((fitted, cost)) =
                dp_wrap_slices(&words, &all_slices[start..start + k], &measure_fn)
            {
                let update = match &best_fitted {
                    Some((_, best_cost)) => cost < *best_cost,
                    None => true,
                };
                if update {
                    best_fitted = Some((fitted, cost));
                }
            }
        }
        if let Some((fitted, _)) = best_fitted {
            return Some(fitted);
        }

        for k in 1..=all_slices.len() {
            let start = (all_slices.len() - k) / 2;
            let (fitted, used) =
                Self::wrap_words(&words, &all_slices[start..start + k], false, &measure_fn);
            if used == words.len() {
                return Some(fitted);
            }
        }
        None
    }

    pub fn fit_text_lines(
        text: &str,
        slices: &[Self],
        measure_fn: impl Fn(&str) -> f32,
    ) -> Vec<FittedLine> {
        let words: Vec<&str> = text.split_whitespace().collect();
        if words.is_empty() || slices.is_empty() {
            return Vec::new();
        }

        let mut best_fitted: Option<(Vec<FittedLine>, f64)> = None;
        for k in 1..=slices.len().min(words.len()) {
            if let Some((fitted, cost)) = dp_wrap_slices(&words, &slices[0..k], &measure_fn) {
                let update = match &best_fitted {
                    Some((_, best_cost)) => cost < *best_cost,
                    None => true,
                };
                if update {
                    best_fitted = Some((fitted, cost));
                }
            }
        }
        if let Some((fitted, _)) = best_fitted {
            return fitted;
        }

        Self::wrap_words(&words, slices, true, &measure_fn).0
    }

    fn wrap_words(
        words: &[&str],
        slices: &[Self],
        allow_overflow_single: bool,
        measure_fn: &impl Fn(&str) -> f32,
    ) -> (Vec<FittedLine>, usize) {
        let (mut fitted, mut word_idx) = (Vec::with_capacity(slices.len()), 0);
        for &slice in slices {
            if word_idx >= words.len() {
                break;
            }
            let mut text = String::new();
            while word_idx < words.len() {
                let next = words[word_idx];
                let cand = if text.is_empty() {
                    next.to_string()
                } else {
                    format!("{text} {next}")
                };
                if measure_fn(&cand) <= slice.available_width {
                    text = cand;
                    word_idx += 1;
                } else if text.is_empty() && allow_overflow_single {
                    text = next.to_string();
                    word_idx += 1;
                    break;
                } else {
                    break;
                }
            }
            if !text.is_empty() {
                let line_width = measure_fn(&text);
                let start_x = slice.center_x - line_width * 0.5;
                fitted.push(FittedLine {
                    text,
                    slice,
                    line_width,
                    start_x,
                });
            }
        }
        (fitted, word_idx)
    }
}

pub fn estimate_target_font_size(box_height_px: f32, estimated_lines: usize) -> f32 {
    let raw_h = box_height_px / (estimated_lines.max(1) as f32);
    (raw_h * 0.75).clamp(9.0, 36.0)
}

pub fn clamp_target_font_size(estimated_orig: f32, max_fitting_size: f32) -> f32 {
    let upper = (estimated_orig * 1.15).max(9.0);
    max_fitting_size.min(upper).max(9.0)
}

pub fn is_character_length_balanced(original: &str, translated: &str) -> bool {
    let orig_len = original.chars().count().max(1);
    let trans_len = translated.chars().count().max(1);
    (0.65..=1.50).contains(&(trans_len as f32 / orig_len as f32))
}

/// Upper bound on font-size search iterations, whatever `FontRange` says.
const MAX_LAYOUT_ITERATIONS: usize = 512;
/// Smallest allowed font-size search step in pixels.
const MIN_STEP_PX: f32 = 0.25;

pub fn layout_region(
    c: &BubbleContour,
    text: &str,
    measure: &dyn Fn(&str, f32) -> f32,
    font: FontRange,
) -> Option<TextLayout> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let min_slice_w = (c.box_w as f32 * 0.28).clamp(18.0, 42.0);
    let h_pad = (c.box_w as f32 * 0.05).clamp(2.5, 10.0);
    if !font.min_px.is_finite() || !font.max_px.is_finite() || !font.step_px.is_finite() {
        return None;
    }
    let step = font.step_px.max(MIN_STEP_PX);
    let mut px = font.max_px;

    for _ in 0..MAX_LAYOUT_ITERATIONS {
        if px < font.min_px || px <= 0.0 {
            break;
        }
        let slices = c.compute_line_slices(px * 1.18, min_slice_w, h_pad);
        let m = |s: &str| measure(s, px);
        if let Some(fitted) = c.fit_text_lines_centered(trimmed, &slices, m) {
            return Some(TextLayout {
                font_px: px,
                lines: fitted
                    .into_iter()
                    .map(|f| TextLayoutLine {
                        text: f.text,
                        slice_center_x: f.slice.center_x,
                        slice_y: f.slice.y,
                        slice_width: f.slice.available_width,
                    })
                    .collect(),
            });
        }
        px -= step;
    }
    None
}
