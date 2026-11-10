//! Provider adapters entry point (ADR-0002 section 4): translate ONE prepared image tile through
//! the provider named by a [`ProviderSpec`]. The fallback chain and multi-tile orchestration
//! belong to the caller.
//!
//! SSRF: in server (BE) mode `ProviderSpec::endpoint` must come from server configuration,
//! never from a client request. See [`super::endpoint::validate_endpoint`].

use super::call::{call_with_retry, CallError, RetryPolicy};
use super::parse::{extract_json_payload, parse_vision_regions};
use super::region::LlmRegion;
use super::transport::{Header, HttpRequest, HttpTransport, Method, Sleeper};
use super::{custom_http, gemini, openai_compat};
use crate::contract::v1::{
    CancelToken, ProviderError, ProviderKind, ProviderSpec, ReadingDirection,
};
use base64::Engine;
use serde_json::Value;
use std::time::{Duration, SystemTime};

/// Everything a call needs besides the provider and the tile. Cheap to copy.
#[derive(Clone, Copy)]
pub struct CallEnv<'a> {
    pub transport: &'a dyn HttpTransport,
    pub sleeper: &'a dyn Sleeper,
    pub policy: &'a RetryPolicy,
    pub cancel: &'a CancelToken,
    pub now: &'a dyn Fn() -> SystemTime,
}

/// One prepared image tile plus the text that travels with it.
#[derive(Debug, Clone, Copy)]
pub struct TileInput<'a> {
    pub prompt: &'a str,
    /// Encoded image bytes (JPEG after `prepare_page`, or the passthrough original).
    pub jpeg: &'a [u8],
    /// `image/jpeg`, `image/png` or `image/webp`.
    pub mime: &'a str,
    pub reading_direction: ReadingDirection,
    /// Only sent to custom HTTP endpoints.
    pub page_id: &'a str,
    pub source_lang: &'a str,
    pub target_lang: &'a str,
}

/// Sends one tile to `spec` (with the short retry of `env.policy`) and parses the answer.
///
/// An explicit `{"regions":[]}` answer is a success with no regions; any other answer that
/// yields no valid region is `InvalidResponse`. Errors never contain URLs, keys, headers,
/// image bytes or model output.
pub async fn translate_tile(
    env: &CallEnv<'_>,
    spec: &ProviderSpec,
    tile: &TileInput<'_>,
) -> Result<Vec<LlmRegion>, CallError> {
    let req = match spec.kind {
        ProviderKind::Gemini => gemini::build_request(spec, tile)?,
        ProviderKind::OpenAiCompatible => openai_compat::build_request(spec, tile)?,
        ProviderKind::CustomHttp => custom_http::build_request(spec, tile)?,
    };
    let resp = call_with_retry(
        env.transport,
        env.sleeper,
        spec.kind,
        &req,
        env.policy,
        env.cancel,
        env.now,
    )
    .await
    .map_err(|e| scrub_key(e, spec))?;
    let text = match spec.kind {
        ProviderKind::Gemini => gemini::extract_text(&resp.body)?,
        ProviderKind::OpenAiCompatible => openai_compat::extract_text(&resp.body)?,
        ProviderKind::CustomHttp => custom_http::extract_text(&resp.body)?,
    };
    Ok(parse_regions(&text, tile.reading_direction)?)
}

/// A provider may echo the key it was sent inside an error message; drop it from the text.
fn scrub_key(err: CallError, spec: &ProviderSpec) -> CallError {
    let key = spec.credentials.api_key.as_deref().map(str::trim);
    let (Some(key), CallError::Provider(e)) = (key.filter(|k| !k.is_empty()), &err) else {
        return err;
    };
    let hide = |m: &str| m.replace(key, "[redacted]");
    CallError::Provider(match e {
        ProviderError::BadRequest(m) => ProviderError::BadRequest(hide(m)),
        ProviderError::InvalidResponse(m) => ProviderError::InvalidResponse(hide(m)),
        other => other.clone(),
    })
}

pub(super) fn invalid(msg: &str) -> ProviderError {
    ProviderError::InvalidResponse(msg.to_owned())
}

fn parse_regions(text: &str, dir: ReadingDirection) -> Result<Vec<LlmRegion>, ProviderError> {
    let regions =
        parse_vision_regions(text, dir).map_err(|_| invalid("unparseable model output"))?;
    if regions.is_empty() && !declared_empty(text) {
        return Err(invalid("no valid regions in model output"));
    }
    Ok(regions)
}

/// True when the payload is `{"regions":[]}` (or `[]`): the model saw no text.
fn declared_empty(text: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(extract_json_payload(text)) else {
        return false;
    };
    let items = match &value {
        Value::Array(items) => Some(items),
        other => other.get("regions").and_then(Value::as_array),
    };
    items.is_some_and(Vec::is_empty)
}

pub(super) fn encode_image(tile: &TileInput<'_>) -> Result<String, ProviderError> {
    if !["image/jpeg", "image/png", "image/webp"].contains(&tile.mime) {
        return Err(ProviderError::BadRequest(
            "unsupported image type".to_owned(),
        ));
    }
    Ok(base64::engine::general_purpose::STANDARD.encode(tile.jpeg))
}

/// Trimmed API key. `required` keys that are missing or not a valid header value are an auth
/// failure (no secret is echoed); optional keys that are missing give `None`.
pub(super) fn api_key(
    spec: &ProviderSpec,
    required: bool,
) -> Result<Option<String>, ProviderError> {
    let key = spec.credentials.api_key.as_deref().map(str::trim);
    match key {
        Some(k) if !k.is_empty() => {
            if k.bytes().all(|b| b.is_ascii_graphic()) {
                Ok(Some(k.to_owned()))
            } else {
                Err(ProviderError::Auth { status: 401 })
            }
        }
        _ if required => Err(ProviderError::Auth { status: 401 }),
        _ => Ok(None),
    }
}

pub(super) fn json_post(
    url: String,
    mut headers: Vec<Header>,
    body: &Value,
    timeout: Duration,
) -> Result<HttpRequest, ProviderError> {
    let body = serde_json::to_vec(body)
        .map_err(|_| ProviderError::BadRequest("request encoding failed".to_owned()))?;
    headers.push(Header::new("content-type", "application/json"));
    Ok(HttpRequest {
        method: Method::Post,
        url,
        headers,
        body,
        timeout,
    })
}
