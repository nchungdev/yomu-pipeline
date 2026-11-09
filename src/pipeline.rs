//! High-level comic page translation processing and typesetting orchestrator.

use image::RgbaImage;
use serde::{Deserialize, Serialize};

use crate::inpaint::inpaint_page;
use crate::layout::{FontRange, TextLayout};
use crate::render::FontRenderer;
use crate::types::RegionInput;
use crate::typography::infer_typography;

/// Processing output containing the inpainted & rendered image and per-region layouts.
#[derive(Debug, Clone)]
pub struct ProcessedPage {
    pub image: RgbaImage,
    pub layouts: Vec<Option<TextLayout>>,
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

/// Executes end-to-end translation pipeline on a comic page:
/// 1. Infers typography for each region from the source image.
/// 2. Performs statistical inpainting & bubble contour preservation.
/// 3. Typesets translated text into bubbles using dynamic programming.
/// 4. Renders text glyphs onto the resulting image.
pub fn process_page(
    image: &RgbaImage,
    regions: &[RegionInput],
    renderer: &FontRenderer,
    options: PipelineOptions,
) -> ProcessedPage {
    let typographies: Vec<_> = if options.auto_typography {
        regions
            .iter()
            .map(|r| infer_typography(image, r.bounds))
            .collect()
    } else {
        regions
            .iter()
            .map(|_| crate::types::InferredTypography::default())
            .collect()
    };

    let inpainted = inpaint_page(image, regions);
    let mut result_image = inpainted.image;
    let mut layouts = Vec::with_capacity(regions.len());

    for (i, region) in regions.iter().enumerate() {
        if region.text.trim().is_empty() {
            layouts.push(None);
            continue;
        }

        let typo = &typographies[i];
        let layout = if let Some(Some(contour)) = inpainted.contours.get(i) {
            renderer.render_bubble(
                &mut result_image,
                contour,
                &region.text,
                typo,
                options.font_range,
            )
        } else {
            None
        };
        layouts.push(layout);
    }

    ProcessedPage {
        image: result_image,
        layouts,
    }
}
