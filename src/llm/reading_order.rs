//! Natural manga/comic reading order (port of the app's `sort_regions_by_reading_order`).

use crate::contract::v1::ReadingDirection;

use super::region::LlmRegion;

struct Item {
    original_idx: usize,
    cx: f32,
    cy: f32,
    y: f32,
    h: f32,
}

/// Sorts regions top-to-bottom by panel row; inside a row right-to-left for
/// [`ReadingDirection::Rtl`] and left-to-right for [`ReadingDirection::Ltr`].
///
/// Rows are formed from vertical centres: a region joins the current row when its centre is
/// within `clamp(0.65 * min height, 0.06, 0.14)` of the row anchor, or when the vertical
/// overlap is at least half of the smaller height. Ties keep the input order (stable).
pub fn sort_regions_by_reading_order(regions: &mut [LlmRegion], direction: ReadingDirection) {
    if regions.len() <= 1 {
        return;
    }
    let mut items: Vec<Item> = regions
        .iter()
        .enumerate()
        .map(|(i, r)| Item {
            original_idx: i,
            cx: r.bounds.x + r.bounds.width * 0.5,
            cy: r.bounds.y + r.bounds.height * 0.5,
            y: r.bounds.y,
            h: r.bounds.height,
        })
        .collect();
    items.sort_by(|a, b| {
        a.cy.total_cmp(&b.cy)
            .then_with(|| a.original_idx.cmp(&b.original_idx))
    });

    let mut rows: Vec<Vec<Item>> = Vec::new();
    for item in items {
        if let Some(row) = rows.last_mut() {
            if same_row(&row[0], &item) {
                row.push(item);
                continue;
            }
        }
        rows.push(vec![item]);
    }

    let rtl = direction == ReadingDirection::Rtl;
    for row in &mut rows {
        row.sort_by(|a, b| {
            let (ka, kb) = (bucket(a.cx), bucket(b.cx));
            let order = if rtl { kb.cmp(&ka) } else { ka.cmp(&kb) };
            order.then_with(|| a.original_idx.cmp(&b.original_idx))
        });
    }

    let original = regions.to_vec();
    let order = rows.iter().flatten().map(|it| it.original_idx);
    for (slot, idx) in regions.iter_mut().zip(order) {
        *slot = original[idx].clone();
    }
}

fn same_row(anchor: &Item, item: &Item) -> bool {
    let min_h = item.h.min(anchor.h);
    let threshold = (min_h * 0.65).clamp(0.06, 0.14);
    if (item.cy - anchor.cy).abs() <= threshold {
        return true;
    }
    let top = item.y.max(anchor.y);
    let bottom = (item.y + item.h).min(anchor.y + anchor.h);
    min_h > 0.0 && (bottom - top).max(0.0) / min_h >= 0.5
}

/// Discretizes x to 1e-4 so negligible differences fall back to input order.
fn bucket(cx: f32) -> i64 {
    (cx * 10_000.0).round() as i64
}
