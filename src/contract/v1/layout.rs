use serde::{Deserialize, Serialize};

/// A point in pixels of the source image.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PxPoint {
    pub x: f32,
    pub y: f32,
}

/// One laid-out line of translated text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutLine {
    pub text: String,
    /// Pen origin of the line: the left end of the **alphabetic baseline**, in pixels of the
    /// source image (not the top of the line box and not the slice centre). A consumer draws the
    /// first glyph with its origin exactly here.
    pub origin_px: PxPoint,
    /// Advance width of the line in pixels at `TextLayoutV1::font_px`.
    pub width_px: f32,
}

/// Resolved text layout of one region (version 1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextLayoutV1 {
    pub font_px: f32,
    pub lines: Vec<LayoutLine>,
}
