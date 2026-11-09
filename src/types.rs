//! Pure self-contained data transfer objects and geometry types for comic/manga pipelines.

use image::RgbaImage;
use serde::{Deserialize, Serialize};

use crate::contour::BubbleContour;

/// Errors associated with rectangular bounds and coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RectError {
    #[error("Invalid normalized bounding box: values must be finite, >= 0.0, and fit within 1.0")]
    InvalidBounds,
}

/// Normalized bounding box coordinates `[0.0, 1.0]` for OCR regions and speech bubbles.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NormalizedRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl NormalizedRect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Result<Self, RectError> {
        let rect = Self {
            x,
            y,
            width,
            height,
        };
        rect.validate()?;
        Ok(rect)
    }

    #[inline]
    pub fn x(&self) -> f32 {
        self.x
    }

    #[inline]
    pub fn y(&self) -> f32 {
        self.y
    }

    #[inline]
    pub fn width(&self) -> f32 {
        self.width
    }

    #[inline]
    pub fn height(&self) -> f32 {
        self.height
    }

    pub fn validate(&self) -> Result<(), RectError> {
        let values = [self.x, self.y, self.width, self.height];
        if values.iter().any(|v| !v.is_finite())
            || self.x < 0.0
            || self.y < 0.0
            || self.width <= 0.0
            || self.height <= 0.0
            || self.x + self.width > 1.0001
            || self.y + self.height > 1.0001
        {
            return Err(RectError::InvalidBounds);
        }
        Ok(())
    }
}

/// Absolute pixel rectangle coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PxRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl PxRect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    #[inline]
    pub fn min_x(&self) -> f32 {
        self.x
    }

    #[inline]
    pub fn min_y(&self) -> f32 {
        self.y
    }

    #[inline]
    pub fn max_x(&self) -> f32 {
        self.x + self.w
    }

    #[inline]
    pub fn max_y(&self) -> f32 {
        self.y + self.h
    }
}

/// Visual hint for bubble rendering and inpainting style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub enum BubbleStyleHint {
    #[default]
    Balloon,
    DarkBalloon,
    FloatingText,
}

/// Input region for bubble detection, inpainting, and text layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegionInput {
    pub id: String,
    pub bounds: NormalizedRect,
    pub text: String,
    #[serde(default)]
    pub style_hint: BubbleStyleHint,
}

/// Inpainting results including the cleaned image and detected bubble contours.
#[derive(Debug, Clone)]
pub struct InpaintedPage {
    pub image: RgbaImage,
    pub contours: Vec<Option<BubbleContour>>,
}

/// Typography styling inferred from original comic ink.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct InferredTypography {
    pub text_color: [u8; 4],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_color: Option<[u8; 4]>,
    #[serde(default)]
    pub stroke_width: f32,
    #[serde(default)]
    pub rotation_rad: f32,
    #[serde(default)]
    pub is_vertical: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_font_size: Option<f32>,
}

impl Default for InferredTypography {
    fn default() -> Self {
        Self {
            text_color: [0, 0, 0, 255],
            stroke_color: None,
            stroke_width: 0.0,
            rotation_rad: 0.0,
            is_vertical: false,
            estimated_font_size: None,
        }
    }
}

impl InferredTypography {
    pub fn new(text_color: [u8; 4]) -> Self {
        Self {
            text_color,
            ..Default::default()
        }
    }

    pub fn with_stroke(mut self, stroke_color: [u8; 4], stroke_width: f32) -> Self {
        self.stroke_color = Some(stroke_color);
        self.stroke_width = stroke_width;
        self
    }

    pub fn with_rotation(mut self, rotation_rad: f32) -> Self {
        self.rotation_rad = rotation_rad;
        self
    }

    pub fn with_vertical(mut self, is_vertical: bool) -> Self {
        self.is_vertical = is_vertical;
        self
    }

    pub fn with_estimated_font_size(mut self, size: f32) -> Self {
        self.estimated_font_size = Some(size);
        self
    }

    #[inline]
    pub fn text_color(&self) -> [u8; 4] {
        self.text_color
    }

    #[inline]
    pub fn stroke_color(&self) -> Option<[u8; 4]> {
        self.stroke_color
    }

    #[inline]
    pub fn stroke_width(&self) -> f32 {
        self.stroke_width
    }

    #[inline]
    pub fn rotation_rad(&self) -> f32 {
        self.rotation_rad
    }

    #[inline]
    pub fn is_vertical(&self) -> bool {
        self.is_vertical
    }

    #[inline]
    pub fn estimated_font_size(&self) -> Option<f32> {
        self.estimated_font_size
    }
}
