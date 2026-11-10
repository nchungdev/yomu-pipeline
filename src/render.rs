//! Headless font rasterization, text measurement, and canvas typesetting.

use ab_glyph::{point, Font, FontArc, Glyph, Point, PxScale, ScaleFont};
use image::{Rgba, RgbaImage};
use std::path::Path;

use crate::contour::BubbleContour;
use crate::contract::v1::{LayoutLine, PxPoint, TextLayoutV1};
use crate::layout::{layout_region, FontRange, TextLayout};
use crate::types::InferredTypography;

/// Default embedded font bytes (BeVietnamPro-Regular) supporting Latin and Vietnamese diacritics.
pub const DEFAULT_FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/BeVietnamPro-Regular.ttf");

/// Font renderer providing exact glyph measurement and pixel rasterization onto RGBA images.
#[derive(Clone)]
pub struct FontRenderer {
    font: FontArc,
}

#[derive(Debug, thiserror::Error)]
pub enum FontError {
    #[error("Failed to parse font data: {0}")]
    InvalidFont(String),
    #[error("Failed to read font file from disk: {0}")]
    Io(#[from] std::io::Error),
}

impl Default for FontRenderer {
    fn default() -> Self {
        Self::from_bytes(DEFAULT_FONT_BYTES).expect("bundled default font is valid")
    }
}

impl FontRenderer {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FontError> {
        let font = FontArc::try_from_vec(bytes.to_vec())
            .map_err(|e| FontError::InvalidFont(e.to_string()))?;
        Ok(Self { font })
    }

    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, FontError> {
        let bytes = std::fs::read(path)?;
        Self::from_bytes(&bytes)
    }

    pub fn font(&self) -> &FontArc {
        &self.font
    }

    /// Measures the total advance width of a string in pixels at `font_px`.
    pub fn measure_width(&self, text: &str, font_px: f32) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let scale = PxScale::from(font_px);
        let scaled_font = self.font.as_scaled(scale);
        let mut width = 0.0f32;
        let mut last_glyph = None;

        for ch in text.chars() {
            let id = scaled_font.glyph_id(ch);
            if let Some(last) = last_glyph {
                width += scaled_font.kern(last, id);
            }
            width += scaled_font.h_advance(id);
            last_glyph = Some(id);
        }
        width
    }

    /// Layouts text into a detected bubble contour and renders it onto the image.
    pub fn render_bubble(
        &self,
        image: &mut RgbaImage,
        contour: &BubbleContour,
        text: &str,
        typo: &InferredTypography,
        font_range: FontRange,
    ) -> Option<TextLayout> {
        let measure = |s: &str, px: f32| self.measure_width(s, px);
        let layout = layout_region(contour, text, &measure, font_range)?;
        self.render_layout(image, &layout, typo);
        Some(layout)
    }

    /// Converts a layout to the contract form: `origin_px` is the pen origin on the alphabetic
    /// baseline in source-image pixels, exactly where `render_layout` starts each line.
    pub fn to_layout_v1(&self, layout: &TextLayout) -> TextLayoutV1 {
        let ascent = self.font.as_scaled(PxScale::from(layout.font_px)).ascent();
        let lines = layout
            .lines
            .iter()
            .map(|line| {
                let width_px = self.measure_width(&line.text, layout.font_px);
                LayoutLine {
                    text: line.text.clone(),
                    origin_px: PxPoint {
                        x: line.slice_center_x - width_px * 0.5,
                        y: line.slice_y + ascent,
                    },
                    width_px,
                }
            })
            .collect();
        TextLayoutV1 {
            font_px: layout.font_px,
            lines,
        }
    }

    /// Renders lines of a computed `TextLayout` onto an RGBA canvas.
    pub fn render_layout(
        &self,
        image: &mut RgbaImage,
        layout: &TextLayout,
        typo: &InferredTypography,
    ) {
        let scale = PxScale::from(layout.font_px);
        let scaled_font = self.font.as_scaled(scale);
        let v_metrics = scaled_font.ascent();

        for line in &layout.lines {
            let line_w = self.measure_width(&line.text, layout.font_px);
            let start_x = line.slice_center_x - (line_w * 0.5);
            let baseline_y = line.slice_y + v_metrics;

            if let Some(stroke_c) = typo.stroke_color {
                let max_sw = (layout.font_px * 0.15).max(1.0);
                let sw = if typo.stroke_width.is_finite() {
                    typo.stroke_width.round().clamp(1.0, max_sw) as i32
                } else {
                    1
                };
                for dy in -sw..=sw {
                    for dx in -sw..=sw {
                        if dx * dx + dy * dy <= sw * sw && (dx != 0 || dy != 0) {
                            self.draw_string(
                                image,
                                &line.text,
                                scale,
                                point(start_x + dx as f32, baseline_y + dy as f32),
                                stroke_c,
                            );
                        }
                    }
                }
            }

            self.draw_string(
                image,
                &line.text,
                scale,
                point(start_x, baseline_y),
                typo.text_color,
            );
        }
    }

    fn draw_string(
        &self,
        image: &mut RgbaImage,
        text: &str,
        scale: PxScale,
        start: Point,
        color: [u8; 4],
    ) {
        let scaled_font = self.font.as_scaled(scale);
        let mut caret = start;
        let mut last_glyph = None;

        for ch in text.chars() {
            let id = scaled_font.glyph_id(ch);
            if let Some(last) = last_glyph {
                caret.x += scaled_font.kern(last, id);
            }
            let glyph: Glyph = id.with_scale_and_position(scale, caret);
            if let Some(outlined) = self.font.outline_glyph(glyph) {
                let bounds = outlined.px_bounds();
                outlined.draw(|gx, gy, coverage| {
                    if coverage <= 0.005 {
                        return;
                    }
                    let px = (bounds.min.x as i32 + gx as i32) as u32;
                    let py = (bounds.min.y as i32 + gy as i32) as u32;
                    if px < image.width() && py < image.height() {
                        blend_pixel(image.get_pixel_mut(px, py), color, coverage);
                    }
                });
            }
            caret.x += scaled_font.h_advance(id);
            last_glyph = Some(id);
        }
    }
}

#[inline]
fn blend_pixel(dst: &mut Rgba<u8>, src: [u8; 4], coverage: f32) {
    let alpha = (src[3] as f32 / 255.0) * coverage;
    let inv_alpha = 1.0 - alpha;
    dst[0] = ((src[0] as f32 * alpha) + (dst[0] as f32 * inv_alpha)).round() as u8;
    dst[1] = ((src[1] as f32 * alpha) + (dst[1] as f32 * inv_alpha)).round() as u8;
    dst[2] = ((src[2] as f32 * alpha) + (dst[2] as f32 * inv_alpha)).round() as u8;
    dst[3] = ((alpha + (dst[3] as f32 / 255.0) * inv_alpha) * 255.0).round() as u8;
}
