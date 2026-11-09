//! Color, stroke width, and rotation inference from bubble pixels.

pub mod analyze;
pub use analyze::*;

use image::RgbaImage;

use crate::types::{InferredTypography, NormalizedRect};

const SNAP_ANGLE_DEG: f32 = 3.0;

pub fn detect_rotation(w: usize, h: usize, ink: &[bool]) -> f32 {
    let pts: Vec<(f32, f32)> = (0..h * w)
        .filter(|&i| ink[i])
        .map(|i| ((i % w) as f32, (i / w) as f32))
        .collect();
    if pts.len() < 10 {
        return 0.0;
    }
    let (cx, cy, d) = (
        w as f32 * 0.5,
        h as f32 * 0.5,
        ((w * w + h * h) as f32).sqrt(),
    );
    let (k, step) = ((d.ceil() as usize) + 3, (pts.len() / 1000).max(1));
    let sample: Vec<(f32, f32)> = pts.into_iter().step_by(step).collect();

    let (mut best_deg, mut max_var, mut deg) = (0.0f32, 0u64, -45.0f32);
    while deg <= 45.0 {
        let (cos_a, sin_a) = (deg.to_radians().cos(), deg.to_radians().sin());
        let mut bins = vec![0u32; k];
        for &(px, py) in &sample {
            let u = -(px - cx) * sin_a + (py - cy) * cos_a;
            let ub = ((u + d * 0.5 + 1.0).round() as usize).clamp(0, k - 1);
            bins[ub] += 1;
        }
        let var: u64 = bins.iter().map(|&c| (c as u64) * (c as u64)).sum();
        if var > max_var {
            max_var = var;
            best_deg = deg;
        }
        deg += 0.5;
    }
    if best_deg.abs() < SNAP_ANGLE_DEG {
        0.0
    } else {
        best_deg.to_radians()
    }
}

pub fn detect_orientation(w: usize, h: usize, ink: &[bool]) -> bool {
    if (h as f32) <= 1.15 * (w as f32) {
        return false;
    }
    let th = (0..w * h)
        .filter(|&i| i % w + 1 < w && ink[i] != ink[i + 1])
        .count();
    let tv = (0..w * (h - 1)).filter(|&i| ink[i] != ink[i + w]).count();
    (tv as f32) > 1.35 * (th as f32)
}

pub fn estimate_font_size(w: usize, h: usize, ink: &[bool]) -> Option<f32> {
    let (mut hs, mut run) = (Vec::new(), 0);
    for y in 0..=h {
        if y < h && (0..w).any(|x| ink[y * w + x]) {
            run += 1;
        } else if run >= 4 {
            hs.push(run as f32);
            run = 0;
        } else {
            run = 0;
        }
    }
    hs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    hs.get(hs.len() / 2).copied()
}

pub fn infer_typography(image: &RgbaImage, norm_box: NormalizedRect) -> InferredTypography {
    let (iw, ih) = (image.width() as usize, image.height() as usize);
    if iw < 2 || ih < 2 {
        return InferredTypography::default();
    }
    let to_px = |v: f32, max: usize| ((v * max as f32) as i32).clamp(0, max as i32);
    let (x0, y0) = (to_px(norm_box.x(), iw), to_px(norm_box.y(), ih));
    let x1 = to_px(norm_box.x() + norm_box.width(), iw);
    let y1 = to_px(norm_box.y() + norm_box.height(), ih);
    let (w, h) = ((x1 - x0).max(0) as usize, (y1 - y0).max(0) as usize);
    if w < 2 || h < 2 {
        return InferredTypography::default();
    }
    let region = BoxRegion { x0, y0, w, h };
    let bg = sample_bg_median(image, region);
    let ink = extract_ink_mask(image, region, bg);
    let (tc, sc, sw) = analyze_colors_and_stroke(image, region, &ink);
    let mut t = InferredTypography::new(tc)
        .with_rotation(detect_rotation(w, h, &ink))
        .with_vertical(detect_orientation(w, h, &ink));
    t = sc.map_or(t, |c| t.with_stroke(c, sw));
    estimate_font_size(w, h, &ink).map_or(t, |sz| t.with_estimated_font_size(sz))
}
