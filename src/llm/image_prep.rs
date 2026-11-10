//! Pure image preparation for the LLM (ADR-0002 section 8). The pipeline decodes the ORIGINAL
//! bytes once, never re-compresses an already compressed copy, hashes the original bytes and
//! either downscales the page or, for very tall pages, cuts overlapping tiles.
//!
//! The numbers in [`PrepOptions::default`] (1600 px, JPEG q85, 15% overlap, 1280x2048 tiles) are
//! design defaults that are NOT yet benchmarked; the app currently ships 1280 px / q82.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::{self, FilterType};
use image::{DynamicImage, ExtendedColorType, GrayImage, ImageReader, RgbImage};

const MAX_TILES: usize = 512;
const MIN_TILE_HEIGHT: u32 = 64;
const GRAY_SAMPLES: usize = 10_000;
const GRAY_THRESHOLD: f64 = 4.0;

/// Tunables for [`prepare_page`]. Out-of-range values are clamped, never rejected.
#[derive(Debug, Clone, PartialEq)]
pub struct PrepOptions {
    /// Longest edge (px) of a normal page sent to the LLM.
    pub max_edge: u32,
    /// JPEG quality 1..=100.
    pub jpeg_quality: u8,
    /// `height / width` at or above which the page is tiled instead of shrunk.
    pub tall_ratio: f32,
    /// Maximum encoded tile width (px); wider pages are scaled to this width.
    pub tile_width: u32,
    /// Encoded tile height (px).
    pub tile_height: u32,
    /// Fraction (0..0.9) of a tile shared with the next tile.
    pub overlap: f32,
    /// Small valid JPEGs below this size (and within `max_edge`) are passed through untouched.
    pub passthrough_max_bytes: usize,
    /// Decompression-bomb guard: images with more pixels than this are rejected.
    pub max_pixels: u64,
}

impl Default for PrepOptions {
    fn default() -> Self {
        Self {
            max_edge: 1600,
            jpeg_quality: 85,
            tall_ratio: 2.5,
            tile_width: 1280,
            tile_height: 2048,
            overlap: 0.15,
            passthrough_max_bytes: 250_000,
            max_pixels: 100_000_000,
        }
    }
}

/// Reasons a page cannot be prepared. Messages never contain image data or URLs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PrepError {
    #[error("image could not be decoded or encoded")]
    Decode,
    #[error("image is empty")]
    Empty,
    #[error("image exceeds the pixel or tile limit")]
    TooLarge,
}

/// One image to send to the LLM, with its exact position in the ORIGINAL page.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedTile {
    pub jpeg: Vec<u8>,
    pub mime: &'static str,
    /// First original-pixel row covered (inclusive).
    pub y_start_px: u32,
    /// Last original-pixel row covered (exclusive).
    pub y_end_px: u32,
    /// Encoded pixels per original pixel (1.0 when untouched).
    pub scale: f32,
}

/// Result of [`prepare_page`].
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedPage {
    /// blake3 hex of the ORIGINAL bytes; independent of options.
    pub source_hash: String,
    /// Original width in px.
    pub width: u32,
    /// Original height in px.
    pub height: u32,
    pub tiles: Vec<PreparedTile>,
}

/// Detects the MIME type from magic bytes (defaults to PNG like the app).
pub fn detect_image_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "image/png"
    }
}

/// True when the image is near-monochrome (sampled mean channel spread below a small threshold).
pub fn is_mostly_grayscale(img: &RgbImage) -> bool {
    let raw = img.as_raw();
    let pixels = raw.len() / 3;
    if pixels == 0 {
        return true;
    }
    let step = (pixels / GRAY_SAMPLES).max(1);
    let (mut sum, mut n) = (0u64, 0u64);
    for p in raw.as_chunks::<3>().0.iter().step_by(step) {
        let (r, g, b) = (p[0] as i32, p[1] as i32, p[2] as i32);
        sum += ((r - g).abs() + (g - b).abs() + (b - r).abs()) as u64;
        n += 1;
    }
    (sum as f64 / (n as f64 * 3.0)) < GRAY_THRESHOLD
}

/// Prepares one page for the LLM: hash, optional passthrough, single decode, downscale or tile.
pub fn prepare_page(bytes: &[u8], opts: &PrepOptions) -> Result<PreparedPage, PrepError> {
    if bytes.is_empty() {
        return Err(PrepError::Empty);
    }
    let source_hash = blake3::hash(bytes).to_hex().to_string();
    let (hw, hh) = header_dimensions(bytes)?;
    if hw == 0 || hh == 0 {
        return Err(PrepError::Empty);
    }
    if u64::from(hw) * u64::from(hh) > opts.max_pixels {
        return Err(PrepError::TooLarge);
    }
    let max_edge = opts.max_edge.max(1);
    let tall = f64::from(hh) / f64::from(hw) >= f64::from(opts.tall_ratio);
    let is_jpeg = bytes.starts_with(b"\xff\xd8\xff");
    if !tall
        && is_jpeg
        && bytes.len() < opts.passthrough_max_bytes
        && hw <= max_edge
        && hh <= max_edge
    {
        return Ok(page(
            source_hash,
            hw,
            hh,
            vec![tile(bytes.to_vec(), 0, hh, 1.0)],
        ));
    }

    let decoded = decode(bytes)?;
    let (w, h) = (decoded.width(), decoded.height());
    if w == 0 || h == 0 {
        return Err(PrepError::Empty);
    }
    let raster = Raster::from_decoded(&decoded);
    drop(decoded);
    let q = opts.jpeg_quality.clamp(1, 100);
    let tiles = if tall {
        tall_tiles(raster, w, h, opts, q)?
    } else {
        vec![single_tile(raster, w, h, max_edge, q)?]
    };
    Ok(page(source_hash, w, h, tiles))
}

fn page(source_hash: String, width: u32, height: u32, tiles: Vec<PreparedTile>) -> PreparedPage {
    PreparedPage {
        source_hash,
        width,
        height,
        tiles,
    }
}

fn tile(jpeg: Vec<u8>, y0: u32, y1: u32, scale: f32) -> PreparedTile {
    PreparedTile {
        jpeg,
        mime: "image/jpeg",
        y_start_px: y0,
        y_end_px: y1,
        scale,
    }
}

fn header_dimensions(bytes: &[u8]) -> Result<(u32, u32), PrepError> {
    ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| PrepError::Decode)?
        .into_dimensions()
        .map_err(|_| PrepError::Decode)
}

fn decode(bytes: &[u8]) -> Result<DynamicImage, PrepError> {
    ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| PrepError::Decode)?
        .decode()
        .map_err(|_| PrepError::Decode)
}

/// Decoded pixels in the channel layout that will be encoded.
enum Raster {
    Gray(GrayImage),
    Rgb(RgbImage),
}

impl Raster {
    fn from_decoded(img: &DynamicImage) -> Self {
        let rgb = img.to_rgb8();
        if is_mostly_grayscale(&rgb) {
            Raster::Gray(img.to_luma8())
        } else {
            Raster::Rgb(rgb)
        }
    }

    fn resized(self, w: u32, h: u32) -> Self {
        match self {
            Raster::Gray(g) => Raster::Gray(imageops::resize(&g, w, h, FilterType::Triangle)),
            Raster::Rgb(r) => Raster::Rgb(imageops::resize(&r, w, h, FilterType::Triangle)),
        }
    }

    fn height(&self) -> u32 {
        match self {
            Raster::Gray(g) => g.height(),
            Raster::Rgb(r) => r.height(),
        }
    }

    /// Encodes rows `y0..y1` as a JPEG (1 channel for gray, 3 for colour).
    fn encode_rows(&self, y0: u32, y1: u32, quality: u8) -> Result<Vec<u8>, PrepError> {
        let mut out = Vec::with_capacity(192 * 1024);
        let mut enc = JpegEncoder::new_with_quality(&mut out, quality);
        let r = match self {
            Raster::Gray(g) => {
                let s = imageops::crop_imm(g, 0, y0, g.width(), y1 - y0).to_image();
                enc.encode(s.as_raw(), s.width(), s.height(), ExtendedColorType::L8)
            }
            Raster::Rgb(g) => {
                let s = imageops::crop_imm(g, 0, y0, g.width(), y1 - y0).to_image();
                enc.encode(s.as_raw(), s.width(), s.height(), ExtendedColorType::Rgb8)
            }
        };
        r.map_err(|_| PrepError::Decode)?;
        Ok(out)
    }
}

fn scaled(v: u32, s: f64) -> u32 {
    ((f64::from(v) * s).round() as u32).max(1)
}

fn single_tile(
    raster: Raster,
    w: u32,
    h: u32,
    max_edge: u32,
    q: u8,
) -> Result<PreparedTile, PrepError> {
    let long = w.max(h);
    if long <= max_edge {
        return Ok(tile(raster.encode_rows(0, h, q)?, 0, h, 1.0));
    }
    let s = f64::from(max_edge) / f64::from(long);
    let (nw, nh) = (scaled(w, s), scaled(h, s));
    let raster = raster.resized(nw, nh);
    Ok(tile(
        raster.encode_rows(0, nh, q)?,
        0,
        h,
        (f64::from(nw) / f64::from(w)) as f32,
    ))
}

fn tall_tiles(
    raster: Raster,
    w: u32,
    h: u32,
    opts: &PrepOptions,
    q: u8,
) -> Result<Vec<PreparedTile>, PrepError> {
    let tile_w = opts.tile_width.max(1);
    let s = if w > tile_w {
        f64::from(tile_w) / f64::from(w)
    } else {
        1.0
    };
    let raster = if s < 1.0 {
        raster.resized(tile_w, scaled(h, s))
    } else {
        raster
    };
    let sh = raster.height();
    let span = ((f64::from(opts.tile_height.max(MIN_TILE_HEIGHT)) / s).floor() as u32).max(1);
    let ranges = plan_ranges(h, span, opts.overlap)?;
    let mut tiles = Vec::with_capacity(ranges.len());
    for (y0, y1) in ranges {
        let sy0 = if y0 == 0 {
            0
        } else {
            ((f64::from(y0) * s).round() as u32).min(sh - 1)
        };
        let sy1 = if y1 == h {
            sh
        } else {
            ((f64::from(y1) * s).round() as u32).clamp(sy0 + 1, sh)
        };
        tiles.push(tile(raster.encode_rows(sy0, sy1, q)?, y0, y1, s as f32));
    }
    Ok(tiles)
}

/// Overlapping `[start, end)` ranges in original px covering `0..total` with no gap.
fn plan_ranges(total: u32, span: u32, overlap: f32) -> Result<Vec<(u32, u32)>, PrepError> {
    if total <= span {
        return Ok(vec![(0, total)]);
    }
    let frac = if overlap.is_finite() {
        f64::from(overlap).clamp(0.0, 0.9)
    } else {
        0.0
    };
    let step = span
        .saturating_sub((f64::from(span) * frac).round() as u32)
        .max(1);
    let mut out = Vec::new();
    let mut y = 0u32;
    loop {
        let end = y.saturating_add(span).min(total);
        out.push((y, end));
        if end == total {
            return Ok(out);
        }
        if out.len() >= MAX_TILES {
            return Err(PrepError::TooLarge);
        }
        y += step;
    }
}
