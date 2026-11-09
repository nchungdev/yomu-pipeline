//! Dynamic programming text wrapping and Vietnamese penalty calculations.

use super::{FittedLine, TextLineSlice};

/// Calculates the Vietnamese break penalty between `prev_word` and `next_word`.
pub fn vietnamese_break_penalty(prev: &str, next: &str) -> f64 {
    let p = prev.trim();
    if p.ends_with(['.', '!', '?', '…']) || p.ends_with("...") {
        return 0.0;
    }
    if p.ends_with([',', ';', ':', '—', '–']) {
        return 20.0;
    }
    if p.ends_with('-') {
        return 2000.0;
    }

    let last_p = p.split_whitespace().last().unwrap_or(p);
    let p_clean: String = last_p
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase();
    const PREPOSITIONS: &[&str] = &[
        "của", "cho", "với", "ở", "trong", "những", "các", "một", "từ", "đến",
    ];
    if PREPOSITIONS.contains(&p_clean.as_str()) {
        return 300.0;
    }

    let first_n = next.split_whitespace().next().unwrap_or(next.trim());
    let n_clean: String = first_n
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase();
    const CONJUNCTIONS: &[&str] = &[
        "và", "nhưng", "hay", "thì", "rồi", "vì", "nên", "mà", "hoặc",
    ];
    if CONJUNCTIONS.contains(&n_clean.as_str()) {
        return 40.0;
    }

    100.0
}

#[inline]
pub fn line_badness(avail_w: f32, line_w: f32) -> f64 {
    if line_w <= avail_w {
        let diff = (avail_w - line_w) as f64;
        diff * diff * diff
    } else {
        let diff = (line_w - avail_w) as f64;
        diff * diff * diff * 10_000.0
    }
}

pub fn dp_wrap_slices(
    words: &[&str],
    slices: &[TextLineSlice],
    measure_fn: &dyn Fn(&str) -> f32,
) -> Option<(Vec<FittedLine>, f64)> {
    let (n, k) = (words.len(), slices.len());
    if n == 0 || k == 0 || k > n {
        return None;
    }

    let mut dp = vec![vec![(f64::INFINITY, 0usize); n + 1]; k];

    for i in 1..=(n - (k - 1)) {
        let text = words[0..i].join(" ");
        let bad = line_badness(slices[0].available_width, measure_fn(&text));
        let pen = if i < n {
            vietnamese_break_penalty(words[i - 1], words[i])
        } else {
            0.0
        };
        dp[0][i] = (bad + pen, 0);
    }

    for l in 1..k {
        for i in (l + 1)..=(n - (k - 1 - l)) {
            let (avail, mut best_cost, mut best_prev) =
                (slices[l].available_width, f64::INFINITY, 0);
            for prev_i in l..i {
                let prev_cost = dp[l - 1][prev_i].0;
                if prev_cost.is_infinite() {
                    continue;
                }
                let text = words[prev_i..i].join(" ");
                let bad = line_badness(avail, measure_fn(&text));
                let pen = if i < n {
                    vietnamese_break_penalty(words[i - 1], words[i])
                } else {
                    0.0
                };
                let orphan = if l == k - 1 && (i - prev_i) == 1 && n >= 3 {
                    500.0
                } else {
                    0.0
                };
                let cost = prev_cost + bad + pen + orphan;
                if cost < best_cost {
                    (best_cost, best_prev) = (cost, prev_i);
                }
            }
            dp[l][i] = (best_cost, best_prev);
        }
    }

    let (final_cost, _) = dp[k - 1][n];
    if final_cost.is_infinite() {
        return None;
    }

    let (mut curr_end, mut ranges) = (n, Vec::with_capacity(k));
    for l in (0..k).rev() {
        let prev_i = dp[l][curr_end].1;
        ranges.push((prev_i, curr_end));
        curr_end = prev_i;
    }
    ranges.reverse();

    let mut fitted = Vec::with_capacity(k);
    for (l, &(start, end)) in ranges.iter().enumerate() {
        let text = words[start..end].join(" ");
        let line_width = measure_fn(&text);
        if line_width > slices[l].available_width + 0.5 {
            return None;
        }
        let start_x = slices[l].center_x - line_width * 0.5;
        fitted.push(FittedLine {
            text,
            slice: slices[l],
            line_width,
            start_x,
        });
    }

    Some((fitted, final_cost + (k as f64) * 8.0))
}
