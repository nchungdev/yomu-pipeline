//! Gemini `generateContent` adapter. The API key travels only in the `x-goog-api-key` header.

use super::endpoint::base_url;
use super::provider::{api_key, encode_image, invalid, json_post, TileInput};
use super::schemas::{gemini_response_schema, GEMINI_RESPONSE_MIME_TYPE};
use super::transport::{Header, HttpRequest};
use crate::contract::v1::{ProviderError, ProviderSpec};
use serde_json::{json, Value};
use std::time::Duration;

const DEFAULT_BASE: &str = "https://generativelanguage.googleapis.com";
const DEFAULT_MODEL: &str = "gemini-2.5-flash";
const TIMEOUT: Duration = Duration::from_secs(45);

fn model_name(spec: &ProviderSpec) -> Result<&str, ProviderError> {
    let m = spec.model.trim();
    if m.is_empty() {
        return Ok(DEFAULT_MODEL);
    }
    let ok = m
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'));
    if ok {
        Ok(m)
    } else {
        Err(ProviderError::BadRequest("invalid model".to_owned()))
    }
}

/// Thinking is switched off (budget 0) to save latency and cost, as the app does. 2.5 Pro
/// cannot disable thinking and rejects budget 0, so it is left at its default.
fn thinking_config(model: &str) -> Option<Value> {
    (!model.contains("-pro")).then(|| json!({ "thinkingBudget": 0 }))
}

pub(super) fn build_request(
    spec: &ProviderSpec,
    tile: &TileInput<'_>,
) -> Result<HttpRequest, ProviderError> {
    let key = api_key(spec, true)?.unwrap_or_default();
    let base = match spec.endpoint.as_deref() {
        Some(raw) => base_url(raw)?,
        None => DEFAULT_BASE.to_owned(),
    };
    let model = model_name(spec)?;
    let mut generation = json!({
        "responseMimeType": GEMINI_RESPONSE_MIME_TYPE,
        "responseSchema": gemini_response_schema(),
    });
    if let Some(thinking) = thinking_config(model) {
        generation["thinkingConfig"] = thinking;
    }
    let body = json!({
        "contents": [{ "parts": [
            { "text": tile.prompt },
            { "inlineData": { "mimeType": tile.mime, "data": encode_image(tile)? } },
        ]}],
        "generationConfig": generation,
    });
    json_post(
        format!("{base}/v1beta/models/{model}:generateContent"),
        vec![Header::secret("x-goog-api-key", key)],
        &body,
        TIMEOUT,
    )
}

/// Concatenated text of `candidates[0].content.parts`, skipping "thought" parts.
pub(super) fn extract_text(body: &[u8]) -> Result<String, ProviderError> {
    let v: Value = serde_json::from_slice(body).map_err(|_| invalid("response is not JSON"))?;
    let parts = v
        .pointer("/candidates/0/content/parts")
        .and_then(Value::as_array);
    let text: String = parts
        .into_iter()
        .flatten()
        .filter(|p| p.get("thought").and_then(Value::as_bool) != Some(true))
        .filter_map(|p| p.get("text")?.as_str())
        .collect();
    if text.trim().is_empty() {
        return Err(invalid("response has no text"));
    }
    Ok(text)
}
