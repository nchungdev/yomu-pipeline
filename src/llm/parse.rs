//! Parsing of the vision model's JSON answer into [`LlmRegion`]s. Pure and panic-free on
//! arbitrary input: no `unwrap`/`expect`, no `f32::clamp` on unchecked values.

use std::collections::HashSet;

use serde::Deserialize;
use serde_json::Value;

use crate::contract::v1::{ReadingDirection, RegionKind};
use crate::types::NormalizedRect;

use super::reading_order::sort_regions_by_reading_order;
use super::region::LlmRegion;

const MAX_SPEAKER_CHARS: usize = 64;

/// Why an answer could not be turned into regions. Messages never contain the model output.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("empty response from translation provider")]
    Empty,
    #[error("no JSON payload found in translation provider response")]
    NoPayload,
    #[error("translation provider response is not valid JSON (line {line}, column {column})")]
    InvalidJson { line: usize, column: usize },
    #[error("translation provider response has an unexpected JSON shape")]
    UnexpectedShape,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawRegion {
    id: Option<String>,
    x: Option<f32>,
    y: Option<f32>,
    width: Option<f32>,
    height: Option<f32>,
    box_2d: Option<Vec<f32>>,
    text: String,
    translated_text: String,
    k: Option<String>,
    kind: Option<String>,
    spk: Option<String>,
    speaker: Option<String>,
    g: Option<String>,
    age: Option<String>,
    emo: Option<String>,
    int: Option<f32>,
}

/// Parses `{"regions":[...]}` (or a bare array), tolerating fences/prose around it.
///
/// Regions with empty source or translated text, no usable box, or non-finite coordinates are
/// dropped; individual malformed items are skipped rather than failing the page. The result is
/// sorted by reading order.
pub fn parse_vision_regions(
    raw: &str,
    reading_direction: ReadingDirection,
) -> Result<Vec<LlmRegion>, ParseError> {
    if raw.trim().is_empty() {
        return Err(ParseError::Empty);
    }
    let payload = extract_json_payload(raw);
    if payload.trim().is_empty() {
        return Err(ParseError::NoPayload);
    }
    let value: Value = serde_json::from_str(payload).map_err(|e| ParseError::InvalidJson {
        line: e.line(),
        column: e.column(),
    })?;
    let items = match value {
        Value::Array(items) => items,
        Value::Object(mut map) => match map.remove("regions") {
            Some(Value::Array(items)) => items,
            _ => return Err(ParseError::UnexpectedShape),
        },
        _ => return Err(ParseError::UnexpectedShape),
    };

    let mut seen = HashSet::new();
    let mut regions = Vec::new();
    for (idx, item) in items.into_iter().enumerate() {
        let Ok(raw_region) = serde_json::from_value::<RawRegion>(item) else {
            continue;
        };
        if let Some(region) = convert(raw_region, idx, &mut seen) {
            regions.push(region);
        }
    }
    sort_regions_by_reading_order(&mut regions, reading_direction);
    Ok(regions)
}

fn convert(raw: RawRegion, idx: usize, seen: &mut HashSet<String>) -> Option<LlmRegion> {
    let text = raw.text.trim().to_owned();
    let translated = raw.translated_text.trim().to_owned();
    if text.is_empty() || translated.is_empty() {
        return None;
    }
    let bounds = bounds_from(&raw)?;
    let mut id = raw
        .id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map_or_else(|| format!("r-{}", idx + 1), str::to_owned);
    if !seen.insert(id.clone()) {
        id = format!("{id}-{}", idx + 1);
        seen.insert(id.clone());
    }
    let kind = raw
        .k
        .as_deref()
        .and_then(map_kind)
        .or_else(|| raw.kind.as_deref().and_then(map_kind))
        .unwrap_or_default();
    let speaker = raw
        .spk
        .or(raw.speaker)
        .map(|s| s.trim().chars().take(MAX_SPEAKER_CHARS).collect::<String>())
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("null"));
    Some(LlmRegion {
        id,
        bounds,
        text,
        translated,
        kind,
        speaker,
        gender: canonical(
            raw.g.as_deref(),
            &[
                ("m", "m"),
                ("male", "m"),
                ("f", "f"),
                ("female", "f"),
                ("u", "u"),
                ("unknown", "u"),
            ],
        ),
        age: canonical(
            raw.age.as_deref(),
            &[
                ("child", "child"),
                ("teen", "teen"),
                ("adult", "adult"),
                ("elder", "elder"),
            ],
        ),
        emotion: canonical(raw.emo.as_deref(), &EMOTIONS),
        intensity: raw
            .int
            .filter(|i| i.is_finite() && *i >= 0.0)
            .map(|i| i.min(1.0)),
    })
}

const EMOTIONS: [(&str, &str); 8] = [
    ("neutral", "neutral"),
    ("happy", "happy"),
    ("sad", "sad"),
    ("angry", "angry"),
    ("surprised", "surprised"),
    ("serious", "serious"),
    ("whisper", "whisper"),
    ("fear", "fear"),
];

fn canonical(value: Option<&str>, table: &[(&str, &str)]) -> Option<String> {
    let v = value?.trim().to_lowercase();
    table
        .iter()
        .find(|(k, _)| *k == v)
        .map(|(_, c)| (*c).to_owned())
}

fn map_kind(k: &str) -> Option<RegionKind> {
    match k.trim().to_lowercase().as_str() {
        "dialogue" | "speech" | "thought" | "shout" | "balloon" => Some(RegionKind::Balloon),
        "narration" | "narrative" => Some(RegionKind::Narration),
        "sfx" => Some(RegionKind::Sfx),
        "sign" => Some(RegionKind::Sign),
        "other" => Some(RegionKind::Other),
        _ => None,
    }
}

/// Builds clamped page-normalized bounds; `None` when there is no usable box.
fn bounds_from(raw: &RawRegion) -> Option<NormalizedRect> {
    let (mut x, mut y, mut w, mut h) = if let Some(b) = raw.box_2d.as_deref() {
        let [ymin, xmin, ymax, xmax]: [f32; 4] = b.try_into().ok()?;
        if ![ymin, xmin, ymax, xmax].iter().all(|v| v.is_finite()) {
            return None;
        }
        // Already-normalized boxes never exceed 1.0; a real 0-1000 box that small is degenerate.
        let scale = if [ymin, xmin, ymax, xmax].iter().all(|v| v.abs() <= 1.0) {
            1.0
        } else {
            1000.0
        };
        let (x0, y0, x1, y1) = (xmin / scale, ymin / scale, xmax / scale, ymax / scale);
        (x0.min(x1), y0.min(y1), (x1 - x0).abs(), (y1 - y0).abs())
    } else {
        let v = [raw.x?, raw.y?, raw.width?, raw.height?];
        if !v.iter().all(|n| n.is_finite()) {
            return None;
        }
        let scale = if v.iter().any(|n| *n > 1.05) {
            1000.0
        } else {
            1.0
        };
        (v[0] / scale, v[1] / scale, v[2] / scale, v[3] / scale)
    };
    x = x.clamp(0.0, 0.98);
    y = y.clamp(0.0, 0.98);
    // Bounds are finite and ordered (x <= 0.98), so `clamp` cannot panic.
    w = w.clamp(0.01, (1.0 - x).max(0.01));
    h = h.clamp(0.01, (1.0 - y).max(0.01));
    NormalizedRect::new(x, y, w, h).ok()
}

/// Returns the JSON part of a model answer: strips `<think>` blocks, takes the first fenced
/// block, otherwise the span from the first `{`/`[` to the last matching `}`/`]`.
pub fn extract_json_payload(raw: &str) -> &str {
    let mut text = raw.trim();
    if let Some(end) = text.find("</think>") {
        text = text[end + "</think>".len()..].trim();
    }
    for marker in ["```json", "```JSON", "```"] {
        let Some(start) = text.find(marker) else {
            continue;
        };
        let after = &text[start + marker.len()..];
        let Some(end) = after.find("```") else {
            continue;
        };
        let body = after[..end].trim();
        // A bare ``` fence may carry an unknown language tag on its first line.
        let body = match body.split_once('\n') {
            Some((tag, rest))
                if !tag.is_empty() && tag.chars().all(|c| c.is_ascii_alphabetic()) =>
            {
                rest.trim()
            }
            _ => body,
        };
        return body;
    }
    let Some(start) = text.find(['{', '[']) else {
        return text;
    };
    if let Some(end) = balanced_end(&text[start..]) {
        return &text[start..start + end];
    }
    // Unbalanced (e.g. truncated): fall back to the last matching closer.
    let closer = if text[start..].starts_with('{') {
        '}'
    } else {
        ']'
    };
    match text.rfind(closer) {
        Some(end) if end > start => &text[start..=end],
        _ => text,
    }
}

/// Byte length of the leading balanced `{..}`/`[..]` value of `s` (string-aware), if closed.
fn balanced_end(s: &str) -> Option<usize> {
    let (mut depth, mut in_str, mut esc) = (0usize, false, false);
    for (i, c) in s.char_indices() {
        if in_str {
            match (esc, c) {
                (true, _) => esc = false,
                (false, '\\') => esc = true,
                (false, '"') => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' | '[' => depth += 1,
            '}' | ']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}
