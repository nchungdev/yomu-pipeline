//! Contract output of the pipeline: [`process_page_result`] turns the staged page processing into
//! a [`PageResult`] with per-region erase patches (ADR-0002 sections 7 and 7.1).

use std::io::Cursor;

use image::{ImageFormat as Codec, RgbaImage};

use crate::contour::BubbleContour;
use crate::contract::v1::{
    EraseSpec, ImageBlob, ImageFormat, PagePx, PageResult, PatchFormat, RegionResult, RegionStatus,
    RenderMode, SCHEMA_VERSION,
};
use crate::inpaint::extent::{contour_box, PixelBox, PATCH_MARGIN_PX};
use crate::pipeline::{plan_page, PagePlan, PipelineOptions, Planned};
use crate::render::FontRenderer;
use crate::types::{NormalizedRect, PxRect, RegionInput};

/// Placeholder confidence: `RegionInput` carries no detector score, so every region reports 1.0.
pub const PLACEHOLDER_CONFIDENCE: f32 = 1.0;

/// Maximum polygon vertices per side of a detected contour.
const POLYGON_ROWS_PER_SIDE: usize = 32;

/// Result for a page plus the clean (text-free) image it was derived from.
#[derive(Debug, Clone)]
pub struct PageOutcome {
    pub result: PageResult,
    /// Original with the source text erased and no translated text drawn; the patches in
    /// `result` are crops of this image.
    pub clean: RgbaImage,
}

/// Runs the pipeline and returns the contract result.
///
/// * `source_bytes`: the undecoded file bytes. When given, `source_hash` is the lowercase hex
///   blake3 of those bytes; otherwise it is the blake3 of the little-endian width, height and the
///   decoded RGBA pixels.
/// * `RegionInput` carries only the text to draw, so `translated_text` is that text and
///   `source_text` is always empty. `confidence` is [`PLACEHOLDER_CONFIDENCE`].
/// * `cache_key` is left empty; the engine fills it.
/// * `Overlay`: nothing is drawn; `composited` is `None`. `Composited`: the translated text is
///   also baked into a PNG in `composited`. Patches never contain drawn text in either mode.
/// * A region with invalid input bounds reports the full page as placeholder `bounds`, so the
///   result still validates.
pub fn process_page_result(
    page_id: &str,
    image: &RgbaImage,
    source_bytes: Option<&[u8]>,
    regions: &[RegionInput],
    renderer: &FontRenderer,
    options: PipelineOptions,
    render_mode: RenderMode,
) -> PageOutcome {
    let PagePlan {
        clean,
        planned,
        statuses,
    } = plan_page(image, regions, renderer, options);
    let (pw, ph) = clean.dimensions();

    let results = regions
        .iter()
        .zip(planned.iter().zip(statuses))
        .enumerate()
        .map(|(i, (input, (plan, status)))| {
            region_result(i, input, plan.as_ref(), status, &clean, renderer)
        })
        .collect();

    let composited = (render_mode == RenderMode::Composited).then(|| {
        let mut baked = clean.clone();
        for p in planned.iter().flatten() {
            renderer.render_layout(&mut baked, &p.layout, &p.typography);
        }
        encode_png(&baked)
    });

    PageOutcome {
        result: PageResult {
            schema_version: SCHEMA_VERSION,
            page_id: page_id.to_string(),
            source_hash: source_hash(image, source_bytes),
            cache_key: String::new(),
            page_px: PagePx {
                width: pw,
                height: ph,
            },
            regions: results,
            composited: composited.flatten(),
        },
        clean,
    }
}

fn region_result(
    index: usize,
    input: &RegionInput,
    plan: Option<&Planned>,
    status: RegionStatus,
    clean: &RgbaImage,
    renderer: &FontRenderer,
) -> RegionResult {
    let (pw, ph) = clean.dimensions();
    let full = NormalizedRect {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    };
    let base = RegionResult {
        region_id: input.id.clone(),
        kind: input.kind,
        reading_order: u32::try_from(index).unwrap_or(u32::MAX),
        source_text: String::new(),
        translated_text: input.text.clone(),
        confidence: PLACEHOLDER_CONFIDENCE,
        bounds: if input.bounds.validate().is_ok() {
            input.bounds
        } else {
            full
        },
        polygon: Vec::new(),
        typography: input.typography.unwrap_or_default(),
        layout: None,
        erase: EraseSpec::None,
        status,
        speaker: None,
        gender: None,
        age: None,
        emotion: None,
        intensity: None,
    };
    let Some(p) = plan else { return base };
    let cb = contour_box(&p.contour);
    RegionResult {
        bounds: norm_rect(cb, pw, ph).unwrap_or(base.bounds),
        polygon: polygon(&p.contour),
        typography: p.typography,
        layout: Some(renderer.to_layout_v1(&p.layout)),
        erase: p
            .erased
            .map_or(EraseSpec::None, |e| patch(clean, e.union(cb))),
        ..base
    }
}

fn norm_rect(b: PixelBox, pw: u32, ph: u32) -> Option<NormalizedRect> {
    let (w, h) = (pw as f32, ph as f32);
    let r = NormalizedRect {
        x: b.x as f32 / w,
        y: b.y as f32 / h,
        width: (b.w as f32 / w).min(1.0),
        height: (b.h as f32 / h).min(1.0),
    };
    r.validate().ok().map(|()| r)
}

fn patch(clean: &RgbaImage, area: PixelBox) -> EraseSpec {
    let (pw, ph) = clean.dimensions();
    let b = area.expand_clamped(PATCH_MARGIN_PX, pw, ph);
    let (x1, y1) = ((b.x + b.w).min(pw), (b.y + b.h).min(ph));
    let (w, h) = (x1 - b.x, y1 - b.y);
    let mut data = Vec::with_capacity(w as usize * h as usize * 4);
    for y in b.y..y1 {
        for x in b.x..x1 {
            data.extend_from_slice(&clean.get_pixel(x, y).0);
        }
    }
    let rect_norm = NormalizedRect {
        x: b.x as f32 / pw as f32,
        y: b.y as f32 / ph as f32,
        width: w as f32 / pw as f32,
        height: h as f32 / ph as f32,
    };
    EraseSpec::Patch {
        rect_norm,
        rect_px: PxRect::new(b.x as f32, b.y as f32, w as f32, h as f32),
        format: PatchFormat::RawRgba,
        data,
    }
}

/// Outline of the detected rows, normalized to the page: down the left edge, up the right edge.
/// A rectangle contour yields its four corners.
fn polygon(c: &BubbleContour) -> Vec<[f32; 2]> {
    let (iw, ih) = (c.img_w.max(1) as f32, c.img_h.max(1) as f32);
    let rows: Vec<(u32, u32, u32)> = (c.min_y..=c.max_y)
        .filter_map(|y| {
            let (a, b) = (*c.row_spans.get(y as usize)?)?;
            Some((y, a, b))
        })
        .collect();
    let step = rows.len().div_ceil(POLYGON_ROWS_PER_SIDE).max(1);
    let pick: Vec<_> = rows.iter().step_by(step).chain(rows.last()).collect();
    let pt = |x: u32, y: u32| [(c.box_x + x) as f32 / iw, (c.box_y + y) as f32 / ih];
    let left = pick.iter().map(|&&(y, a, _)| pt(a, y));
    let right = pick.iter().rev().map(|&&(y, _, b)| pt(b + 1, y + 1));
    let poly: Vec<[f32; 2]> = left.chain(right).collect();
    if poly.len() < 3 {
        let b = contour_box(c);
        let (x0, y0) = (b.x as f32 / iw, b.y as f32 / ih);
        let (x1, y1) = ((b.x + b.w) as f32 / iw, (b.y + b.h) as f32 / ih);
        return vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    }
    poly
}

fn source_hash(image: &RgbaImage, bytes: Option<&[u8]>) -> String {
    let mut h = blake3::Hasher::new();
    match bytes {
        Some(b) => {
            h.update(b);
        }
        None => {
            h.update(&image.width().to_le_bytes());
            h.update(&image.height().to_le_bytes());
            h.update(image.as_raw());
        }
    }
    h.finalize().to_hex().to_string()
}

fn encode_png(image: &RgbaImage) -> Option<ImageBlob> {
    let mut buf = Cursor::new(Vec::new());
    image.write_to(&mut buf, Codec::Png).ok()?;
    Some(ImageBlob {
        format: ImageFormat::Png,
        data: buf.into_inner(),
    })
}
