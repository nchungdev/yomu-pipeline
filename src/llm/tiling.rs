//! Coordinate remapping and de-duplication for tiled tall pages (port of the app's
//! `webtoon_slicing`). Independent of any region type: callers pass accessors.

use crate::llm::image_prep::PreparedTile;
use crate::types::NormalizedRect;

const IOU_THRESHOLD: f32 = 0.5;
const TEXT_SIMILARITY_THRESHOLD: f32 = 0.6;
const MIN_EXTENT: f32 = 1e-6;

/// Maps a tile-normalized rect (`0..1` over the tile image, tile spanning the full page width)
/// to a page-normalized rect. The rect is clamped to the page; `None` if it is not finite or
/// degenerate after clamping. `_page_w` is accepted for symmetry: x is unchanged because tiles
/// always span the full width.
pub fn remap_tile_rect(
    tile: &PreparedTile,
    page_h: u32,
    _page_w: u32,
    rect: NormalizedRect,
) -> Option<NormalizedRect> {
    let vals = [rect.x, rect.y, rect.width, rect.height];
    if page_h == 0 || tile.y_end_px <= tile.y_start_px || vals.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let ph = f64::from(page_h);
    let span = f64::from(tile.y_end_px - tile.y_start_px);
    let top = (f64::from(tile.y_start_px) + f64::from(rect.y.clamp(0.0, 1.0)) * span) / ph;
    let bottom = (f64::from(tile.y_start_px)
        + f64::from((rect.y + rect.height).clamp(0.0, 1.0)) * span)
        / ph;
    let left = rect.x.clamp(0.0, 1.0);
    let right = (rect.x + rect.width).clamp(0.0, 1.0);
    let (top, bottom) = (top.clamp(0.0, 1.0) as f32, bottom.clamp(0.0, 1.0) as f32);
    if right - left < MIN_EXTENT || bottom - top < MIN_EXTENT {
        return None;
    }
    NormalizedRect::new(left, top, right - left, bottom - top).ok()
}

/// Intersection over union of two normalized rects (0 when disjoint or degenerate).
pub fn rect_iou(a: &NormalizedRect, b: &NormalizedRect) -> f32 {
    let w = (a.x + a.width).min(b.x + b.width) - a.x.max(b.x);
    let h = (a.y + a.height).min(b.y + b.height) - a.y.max(b.y);
    if w <= 0.0 || h <= 0.0 {
        return 0.0;
    }
    let inter = w * h;
    let union = a.width * a.height + b.width * b.height - inter;
    if union <= 0.0 {
        0.0
    } else {
        inter / union
    }
}

/// Dice coefficient over character bigrams of the whitespace-stripped, lowercased texts.
/// Two empty texts count as identical; empty versus non-empty as different.
pub fn text_similarity(a: &str, b: &str) -> f32 {
    let norm = |s: &str| -> Vec<char> {
        s.chars()
            .filter(|c| !c.is_whitespace())
            .flat_map(char::to_lowercase)
            .collect()
    };
    let (a, b) = (norm(a), norm(b));
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    if a.len() < 2 || b.len() < 2 {
        return 0.0;
    }
    let mut pool: Vec<(char, char)> = b.windows(2).map(|w| (w[0], w[1])).collect();
    let mut hits = 0usize;
    for w in a.windows(2) {
        if let Some(i) = pool.iter().position(|p| *p == (w[0], w[1])) {
            pool.swap_remove(i);
            hits += 1;
        }
    }
    2.0 * hits as f32 / ((a.len() - 1) + (b.len() - 1)) as f32
}

/// Removes duplicates produced by tile overlap. Two items are duplicates when their page-level
/// boxes have IoU > 0.5 AND their text similarity (bigram Dice) is >= 0.6, so distinct regions
/// with similar boxes but different text are both kept.
///
/// Winner of a duplicate pair (deterministic): the larger box area (a region cut off by a tile
/// edge is smaller, i.e. less complete), then the longer trimmed text, then the earlier input
/// item. The survivor keeps the position of the first member of its duplicate group. Tile
/// centrality is not available through this generic signature; completeness is its proxy.
pub fn dedupe_overlapping<T>(
    items: Vec<T>,
    bounds: impl Fn(&T) -> NormalizedRect,
    text: impl Fn(&T) -> &str,
) -> Vec<T> {
    let mut kept: Vec<T> = Vec::with_capacity(items.len());
    for item in items {
        let (b, t) = (bounds(&item), text(&item));
        let dup = kept.iter().position(|k| {
            rect_iou(&b, &bounds(k)) > IOU_THRESHOLD
                && text_similarity(t, text(k)) >= TEXT_SIMILARITY_THRESHOLD
        });
        match dup {
            None => kept.push(item),
            Some(i) => {
                if better(&b, t, &bounds(&kept[i]), text(&kept[i])) {
                    kept[i] = item;
                }
            }
        }
    }
    kept
}

/// True when the candidate strictly beats the incumbent (ties keep the incumbent = earlier item).
fn better(cb: &NormalizedRect, ct: &str, ib: &NormalizedRect, it: &str) -> bool {
    let (ca, ia) = (cb.width * cb.height, ib.width * ib.height);
    if ca != ia {
        return ca > ia;
    }
    ct.trim().chars().count() > it.trim().chars().count()
}
