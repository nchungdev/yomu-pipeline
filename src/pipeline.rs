//! High-level comic page translation processing and typesetting orchestrator.

use image::RgbaImage;
use serde::{Deserialize, Serialize};

use crate::contour::BubbleContour;
use crate::contract::v1::{RegionFailure, RegionKind, RegionStatus, SkipReason};
use crate::inpaint::extent::{erase_extent, PixelBox};
use crate::inpaint::{clamp_rect_against_neighbors, erase_region, region_contour};
use crate::layout::{layout_region, FontRange, TextLayout};
use crate::render::FontRenderer;
use crate::types::{InferredTypography, RegionInput};
use crate::typography::infer_typography;

/// Processing output containing the inpainted & rendered image and per-region results.
#[derive(Debug, Clone)]
pub struct ProcessedPage {
    pub image: RgbaImage,
    pub layouts: Vec<Option<TextLayout>>,
    /// One status per input region, in input order. Only `Rendered` regions were erased.
    pub statuses: Vec<RegionStatus>,
}

/// Status of one region, addressed by its id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegionReport {
    pub id: String,
    #[serde(flatten)]
    pub status: RegionStatus,
}

impl ProcessedPage {
    /// Pairs every input region id with its status.
    pub fn reports(&self, regions: &[RegionInput]) -> Vec<RegionReport> {
        regions
            .iter()
            .zip(&self.statuses)
            .map(|(r, s)| RegionReport {
                id: r.id.clone(),
                status: s.clone(),
            })
            .collect()
    }

    /// True when every region was rendered.
    pub fn all_rendered(&self) -> bool {
        self.statuses.iter().all(RegionStatus::is_rendered)
    }
}

/// Options to configure the end-to-end processing pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PipelineOptions {
    pub font_range: FontRange,
    pub auto_typography: bool,
}

impl Default for PipelineOptions {
    fn default() -> Self {
        Self {
            font_range: FontRange::default(),
            auto_typography: true,
        }
    }
}

/// A region that passed layout and erasing and only needs drawing.
pub(crate) struct Planned {
    pub layout: TextLayout,
    pub typography: InferredTypography,
    pub contour: BubbleContour,
    /// Pixels the erase changed (patch rectangle before margin); `None` when nothing changed.
    pub erased: Option<PixelBox>,
}

/// Page after layout and erasing, before any text is drawn.
pub(crate) struct PagePlan {
    /// Original with the source text of every `Rendered` region erased; no text drawn.
    pub clean: RgbaImage,
    /// One entry per input region; `Some` exactly for `Rendered` regions.
    pub planned: Vec<Option<Planned>>,
    pub statuses: Vec<RegionStatus>,
}

/// Stages 1 and 2: lays out every region and erases only those whose text fits.
pub(crate) fn plan_page(
    image: &RgbaImage,
    regions: &[RegionInput],
    renderer: &FontRenderer,
    options: PipelineOptions,
) -> PagePlan {
    let mut clean = image.clone();
    let mut statuses = Vec::with_capacity(regions.len());
    let mut planned = Vec::with_capacity(regions.len());
    for index in 0..regions.len() {
        match plan_region(image, &mut clean, regions, index, renderer, options) {
            Ok(p) => {
                planned.push(Some(p));
                statuses.push(RegionStatus::Rendered);
            }
            Err(status) => {
                planned.push(None);
                statuses.push(status);
            }
        }
    }
    PagePlan {
        clean,
        planned,
        statuses,
    }
}

/// Executes end-to-end translation pipeline on a comic page:
/// 1. Lays out the translated text of each region in its bubble (or bounds rectangle).
/// 2. Only for regions whose text fits, erases the source text.
/// 3. Draws the translated text.
///
/// A region whose text cannot be drawn is never erased; its status says why.
pub fn process_page(
    image: &RgbaImage,
    regions: &[RegionInput],
    renderer: &FontRenderer,
    options: PipelineOptions,
) -> ProcessedPage {
    let PagePlan {
        mut clean,
        planned,
        statuses,
    } = plan_page(image, regions, renderer, options);
    let mut layouts = Vec::with_capacity(regions.len());
    for p in planned {
        layouts.push(p.map(|p| {
            renderer.render_layout(&mut clean, &p.layout, &p.typography);
            p.layout
        }));
    }
    ProcessedPage {
        image: clean,
        layouts,
        statuses,
    }
}

fn plan_region(
    image: &RgbaImage,
    clean: &mut RgbaImage,
    regions: &[RegionInput],
    index: usize,
    renderer: &FontRenderer,
    options: PipelineOptions,
) -> Result<Planned, RegionStatus> {
    let region = &regions[index];
    let skip = |reason| RegionStatus::Skipped { reason };
    let fail = |reason| RegionStatus::Failed { reason };

    if region.kind == RegionKind::Sfx {
        return Err(skip(SkipReason::SfxPolicy));
    }
    if region.bounds.validate().is_err() {
        return Err(fail(RegionFailure::InvalidBounds));
    }
    if region.text.trim().is_empty() {
        return Err(skip(SkipReason::EmptyText));
    }

    let bounds = clamp_rect_against_neighbors(region.bounds, index, regions);
    let (contour, source) =
        region_contour(image, region, bounds).ok_or(fail(RegionFailure::LayoutDoesNotFit))?;
    let measure = |s: &str, px: f32| renderer.measure_width(s, px);
    let layout = layout_region(&contour, &region.text, &measure, options.font_range)
        .ok_or(fail(RegionFailure::LayoutDoesNotFit))?;
    erase_region(clean, bounds, &contour, source).map_err(skip)?;
    let erased = erase_extent(image, clean, bounds, &contour);

    let typography = region.typography.unwrap_or_else(|| {
        if options.auto_typography {
            infer_typography(image, region.bounds)
        } else {
            InferredTypography::default()
        }
    });
    Ok(Planned {
        layout,
        typography,
        contour,
        erased,
    })
}
