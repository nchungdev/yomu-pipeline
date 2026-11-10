//! Headless comic & manga speech bubble processing and typesetting pipeline.
//! Independent of UI/rendering frameworks (zero egui/eframe dependencies).

pub mod contour;
pub mod contract;
pub mod inpaint;
pub mod layout;
#[cfg(feature = "llm")]
pub mod llm;
pub mod mask;
pub mod pipeline;
pub mod pipeline_result;
pub mod render;
pub mod sidecar;
pub mod types;
pub mod typography;

pub use contour::{BubbleContour, ContourDetectionOptions};
pub use inpaint::{
    clamp_rect_against_neighbors, inpaint_page, smart_inpaint, InpaintResult, StatisticalFlatFill,
};
pub use layout::{
    clamp_target_font_size, dp_wrap_slices, estimate_target_font_size,
    is_character_length_balanced, layout_region, vietnamese_break_penalty, FittedLine, FontRange,
    TextLayout, TextLayoutLine, TextLineSlice,
};
pub use mask::{anchor_spans, fill_enclosed_holes, longest_run};
pub use pipeline::{process_page, PipelineOptions, ProcessedPage, RegionReport};
pub use pipeline_result::{process_page_result, PageOutcome};
pub use render::{FontError, FontRenderer, DEFAULT_FONT_BYTES};
pub use sidecar::{parse_page, ParsedPage, SidecarError};
pub use types::{
    BubbleStyleHint, InferredTypography, InpaintedPage, NormalizedRect, PxRect, RectError,
    RegionInput,
};
pub use typography::infer_typography;
