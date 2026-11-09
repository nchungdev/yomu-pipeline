//! Text color, background sampling, and stroke analysis.

use image::RgbaImage;

pub const MIN_STROKE_WIDTH: f32 = 1.5;

#[derive(Clone, Copy)]
pub struct BoxRegion {
    pub x0: i32,
    pub y0: i32,
    pub w: usize,
    pub h: usize,
}

#[inline]
pub fn c_dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

pub fn snap_color(r: u8, g: u8, b: u8) -> [u8; 4] {
    let l = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
    if l < 64.0 {
        [0, 0, 0, 255]
    } else if l > 191.0 {
        [255, 255, 255, 255]
    } else {
        [r, g, b, 255]
    }
}

pub fn sample_bg_median(img: &RgbaImage, b: BoxRegion) -> [u8; 4] {
    let (iw, ih) = (img.width() as i32, img.height() as i32);
    let (x1, y1) = (b.x0 + b.w as i32, b.y0 + b.h as i32);
    let mut chs: [Vec<u8>; 3] = Default::default();
    for y in (b.y0 - 2).max(0)..(y1 + 2).min(ih) {
        for x in (b.x0 - 2).max(0)..(x1 + 2).min(iw) {
            if x < b.x0 || x >= x1 || y < b.y0 || y >= y1 {
                let p = img.get_pixel(x as u32, y as u32);
                chs[0].push(p[0]);
                chs[1].push(p[1]);
                chs[2].push(p[2]);
            }
        }
    }
    if chs[0].is_empty() {
        return [255, 255, 255, 255];
    }
    for c in &mut chs {
        c.sort_unstable();
    }
    let m = chs[0].len() / 2;
    [chs[0][m], chs[1][m], chs[2][m], 255]
}

pub fn extract_ink_mask(img: &RgbaImage, b: BoxRegion, bg: [u8; 4]) -> Vec<bool> {
    let (mut dists, mut hist) = (Vec::with_capacity(b.w * b.h), [0u32; 256]);
    let bg_rgb = [bg[0] as f32, bg[1] as f32, bg[2] as f32];
    for dy in 0..b.h {
        let py = (b.y0 + dy as i32) as u32;
        for dx in 0..b.w {
            let px = (b.x0 + dx as i32) as u32;
            let c = img.get_pixel(px, py);
            let d =
                (c_dist([c[0] as f32, c[1] as f32, c[2] as f32], bg_rgb) / 1.732).min(255.0) as u8;
            dists.push(d);
            hist[d as usize] += 1;
        }
    }
    let (mut sum_b, mut w_b, mut max_var, mut best_t) = (0.0, 0.0, 0.0, 0usize);
    let total = (b.w * b.h) as f64;
    let sum = hist
        .iter()
        .enumerate()
        .map(|(i, &c)| i as f64 * c as f64)
        .sum::<f64>();
    for (t, &c) in hist.iter().enumerate() {
        w_b += c as f64;
        if w_b > 0.0 && w_b < total {
            sum_b += t as f64 * c as f64;
            let diff = (sum_b / w_b) - ((sum - sum_b) / (total - w_b));
            let var = w_b * (total - w_b) * diff * diff;
            if var > max_var {
                max_var = var;
                best_t = t;
            }
        }
    }
    dists
        .into_iter()
        .map(|d| d >= (best_t as u8).clamp(20, 200))
        .collect()
}

pub fn nearest_cluster(cls: &[([f32; 3], usize)], rgb: [f32; 3]) -> (usize, f32) {
    let mut best = (0, f32::MAX);
    for (i, &(s, c)) in cls.iter().enumerate() {
        let d = c_dist([s[0] / c as f32, s[1] / c as f32, s[2] / c as f32], rgb);
        if d < best.1 {
            best = (i, d);
        }
    }
    best
}

pub fn analyze_colors_and_stroke(
    img: &RgbaImage,
    b: BoxRegion,
    ink: &[bool],
) -> ([u8; 4], Option<[u8; 4]>, f32) {
    let (w, h) = (b.w, b.h);
    let coords: Vec<(usize, usize)> = (0..w * h)
        .filter(|&i| ink[i])
        .map(|i| (i % w, i / w))
        .collect();
    if coords.is_empty() {
        return ([0, 0, 0, 255], None, 0.0);
    }
    let mut clusters: Vec<([f32; 3], usize)> = Vec::new();
    for &(dx, dy) in &coords {
        let c = img.get_pixel((b.x0 + dx as i32) as u32, (b.y0 + dy as i32) as u32);
        let rgb = [c[0] as f32, c[1] as f32, c[2] as f32];
        let (ci, d) = nearest_cluster(&clusters, rgb);
        if d < 32.0 || clusters.len() >= 4 {
            let (sum, count) = &mut clusters[ci];
            sum[0] += rgb[0];
            sum[1] += rgb[1];
            sum[2] += rgb[2];
            *count += 1;
        } else {
            clusters.push((rgb, 1));
        }
    }

    clusters.retain(|&(_, cnt)| cnt >= ((coords.len() * 5) / 100).max(2));
    if clusters.is_empty() {
        return ([0, 0, 0, 255], None, 0.0);
    }
    let cluster_rgb = |(s, cnt): ([f32; 3], usize)| -> [u8; 4] {
        let n = cnt.max(1) as f32;
        snap_color(
            (s[0] / n).round() as u8,
            (s[1] / n).round() as u8,
            (s[2] / n).round() as u8,
        )
    };
    if clusters.len() < 2 {
        return (cluster_rgb(clusters[0]), None, 0.0);
    }

    let (mut grid, mut bg_touches) = (vec![usize::MAX; w * h], vec![0usize; clusters.len()]);
    for &(dx, dy) in &coords {
        let c = img.get_pixel((b.x0 + dx as i32) as u32, (b.y0 + dy as i32) as u32);
        let ci = nearest_cluster(&clusters, [c[0] as f32, c[1] as f32, c[2] as f32]).0;
        grid[dy * w + dx] = ci;
        for (sx, sy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            let (nx, ny) = (dx as i32 + sx, dy as i32 + sy);
            if !(0..w as i32).contains(&nx)
                || !(0..h as i32).contains(&ny)
                || !ink[ny as usize * w + nx as usize]
            {
                bg_touches[ci] += 1;
                break;
            }
        }
    }

    let mut fracs: Vec<(f32, usize)> = clusters
        .iter()
        .enumerate()
        .map(|(i, &(_, c))| (bg_touches[i] as f32 / c as f32, i))
        .collect();
    fracs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let (core_i, stroke_i) = (fracs[0].1, fracs[fracs.len() - 1].1);
    let (core_c, stroke_c) = (
        cluster_rgb(clusters[core_i]),
        cluster_rgb(clusters[stroke_i]),
    );
    if c_dist(
        [core_c[0] as f32, core_c[1] as f32, core_c[2] as f32],
        [stroke_c[0] as f32, stroke_c[1] as f32, stroke_c[2] as f32],
    ) < 15.0
    {
        return (core_c, None, 0.0);
    }

    let mut ws = Vec::new();
    for &(x, y) in &coords {
        if grid[y * w + x] != core_i {
            continue;
        }
        for (sx, sy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            let (mut s, mut nx, mut ny) = (1, x as i32 + sx, y as i32 + sy);
            while (0..w as i32).contains(&nx)
                && (0..h as i32).contains(&ny)
                && grid[ny as usize * w + nx as usize] == stroke_i
            {
                s += 1;
                nx += sx;
                ny += sy;
            }
            if s > 1 {
                ws.push(s - 1);
            }
        }
    }
    ws.sort_unstable();
    let sw = ws.get(ws.len() / 2).copied().unwrap_or(0) as f32;
    if sw >= MIN_STROKE_WIDTH {
        (core_c, Some(stroke_c), sw)
    } else {
        (core_c, None, 0.0)
    }
}
