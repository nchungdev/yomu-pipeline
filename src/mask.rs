//! 8-connected BFS bubble mask generation and row span helpers.

use std::collections::VecDeque;

/// Adds every pixel that is enclosed by `mask` (lettering inside a bubble) to the mask.
/// Pixels connected to the box border through non-mask pixels (outline, sky between
/// tail spikes, artwork) stay excluded.
pub fn fill_enclosed_holes(mask: &[bool], w: usize, h: usize) -> Vec<bool> {
    let mut outside = vec![false; w * h];
    let mut queue = VecDeque::new();
    let border = (0..w)
        .flat_map(|x| [(x, 0), (x, h.saturating_sub(1))])
        .chain((0..h).flat_map(|y| [(0, y), (w.saturating_sub(1), y)]));
    for (x, y) in border {
        let i = y * w + x;
        if !mask[i] && !outside[i] {
            outside[i] = true;
            queue.push_back((x, y));
        }
    }
    while let Some((x, y)) = queue.pop_front() {
        let neighbors = [
            (x.wrapping_sub(1), y),
            (x + 1, y),
            (x, y.wrapping_sub(1)),
            (x, y + 1),
        ];
        for (nx, ny) in neighbors {
            if nx < w && ny < h {
                let i = ny * w + nx;
                if !mask[i] && !outside[i] {
                    outside[i] = true;
                    queue.push_back((nx, ny));
                }
            }
        }
    }
    mask.iter().zip(&outside).map(|(&m, &o)| m || !o).collect()
}

/// Longest contiguous `true` run in a row, as inclusive `(start, end)`.
pub fn longest_run(row: &[bool]) -> Option<(u32, u32)> {
    let (mut best, mut start) = (None::<(usize, usize)>, None);
    for (x, &on) in row.iter().chain(std::iter::once(&false)).enumerate() {
        match (on, start) {
            (true, None) => start = Some(x),
            (false, Some(s)) => {
                if best.is_none_or(|(bs, be)| x - 1 - s > be - bs) {
                    best = Some((s, x - 1));
                }
                start = None;
            }
            _ => {}
        }
    }
    best.map(|(s, e)| (s as u32, e as u32))
}

/// Keeps only rows of the bubble holding the text box. The reference is the widest run within
/// `center_y ± half_window`; a row survives when it lies in `[y0, y1]`, is at most 15% wider
/// than the reference and its centre is within 15% of the reference width. Wider rows are page
/// gutter or background the flood fill leaked into; shifted rows belong to a touching bubble.
pub fn anchor_spans(
    spans: &mut [Option<(u32, u32)>],
    (y0, y1): (usize, usize),
    center_y: usize,
    half_window: usize,
) {
    let width = |(l, r): (u32, u32)| (r - l + 1) as f32;
    let centre = |(l, r): (u32, u32)| (l + r) as f32 * 0.5;
    let window = center_y.saturating_sub(half_window).max(y0)..=(center_y + half_window).min(y1);
    let reference = spans
        .get(window)
        .into_iter()
        .flatten()
        .flatten()
        .copied()
        .max_by(|a, b| width(*a).total_cmp(&width(*b)));
    for (y, span) in spans.iter_mut().enumerate() {
        let keep = match (*span, reference) {
            (Some(s), Some(r)) => {
                (y0..=y1).contains(&y)
                    && width(s) <= width(r) * 1.15
                    && (centre(s) - centre(r)).abs() <= width(r) * 0.15
            }
            _ => false,
        };
        if !keep {
            *span = None;
        }
    }
}
