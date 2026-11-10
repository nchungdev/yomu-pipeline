//! OpenAI-compatible `chat/completions` adapter (OpenAI, OpenRouter, Ollama, LM Studio, ...).
//! The key, when present, travels only in the `Authorization` header.

use super::endpoint::base_url;
use super::provider::{api_key, encode_image, invalid, json_post, TileInput};
use super::schemas::openai_response_format;
use super::transport::{Header, HttpRequest};
use crate::contract::v1::{ProviderError, ProviderSpec};
use serde_json::{json, Value};
use std::time::Duration;

const DEFAULT_MODEL: &str = "gpt-4o-mini";
const SUFFIX: &str = "/chat/completions";
const TIMEOUT: Duration = Duration::from_secs(60);

pub(super) fn build_request(
    spec: &ProviderSpec,
    tile: &TileInput<'_>,
) -> Result<HttpRequest, ProviderError> {
    let raw = spec
        .endpoint
        .as_deref()
        .ok_or_else(|| ProviderError::BadRequest("invalid endpoint".to_owned()))?;
    let base = base_url(raw)?;
    let url = if base.ends_with(SUFFIX) {
        base
    } else {
        format!("{base}{SUFFIX}")
    };
    let model = match spec.model.trim() {
        "" => DEFAULT_MODEL,
        m => m,
    };
    let data_uri = format!("data:{};base64,{}", tile.mime, encode_image(tile)?);
    let body = json!({
        "model": model,
        "messages": [{ "role": "user", "content": [
            { "type": "text", "text": tile.prompt },
            { "type": "image_url", "image_url": { "url": data_uri } },
        ]}],
        "reasoning": { "effort": "none" },
        "response_format": openai_response_format(),
    });
    let headers = api_key(spec, false)?
        .map(|k| Header::secret("authorization", format!("Bearer {k}")))
        .into_iter()
        .collect();
    json_post(url, headers, &body, TIMEOUT)
}

/// `choices[0].message.content` as a string or an array of text parts.
pub(super) fn extract_text(body: &[u8]) -> Result<String, ProviderError> {
    let v: Value = serde_json::from_slice(body).map_err(|_| invalid("response is not JSON"))?;
    let message = v
        .pointer("/choices/0/message")
        .ok_or_else(|| invalid("response has no choices"))?;
    if message.get("refusal").is_some_and(|r| !r.is_null()) {
        return Err(ProviderError::ContentBlocked {
            reason: "refusal".to_owned(),
        });
    }
    let text = match message.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| p.get("text")?.as_str())
            .collect(),
        _ => String::new(),
    };
    if text.trim().is_empty() {
        return Err(invalid("response has no text"));
    }
    Ok(text)
}
