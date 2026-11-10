//! Custom HTTP adapter for self-hosted OCR+translation endpoints (LAN / NAS).
//!
//! Request: `POST <endpoint>` with `{book_id, page_number, page_id, source_language,
//! target_language, image_base64}`; optional `Authorization: Bearer <key>`. Response:
//! `{"regions":[{id, x, y, width, height, text, translated_text}]}` (the box_2d form and the
//! optional tags are accepted too, see `parse`). `book_id` and `page_number` are legacy fields
//! the pipeline does not know; they are sent empty / 0 and `page_id` carries the real id.

use super::endpoint::base_url;
use super::provider::{api_key, invalid, json_post, TileInput};
use super::transport::{Header, HttpRequest};
use crate::contract::v1::{ProviderError, ProviderSpec};
use base64::Engine;
use serde_json::json;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(60);

pub(super) fn build_request(
    spec: &ProviderSpec,
    tile: &TileInput<'_>,
) -> Result<HttpRequest, ProviderError> {
    let raw = spec
        .endpoint
        .as_deref()
        .ok_or_else(|| ProviderError::BadRequest("invalid endpoint".to_owned()))?;
    let url = base_url(raw)?;
    let body = json!({
        "book_id": "",
        "page_number": 0,
        "page_id": tile.page_id,
        "source_language": tile.source_lang,
        "target_language": tile.target_lang,
        "image_base64": base64::engine::general_purpose::STANDARD.encode(tile.jpeg),
    });
    let headers = api_key(spec, false)?
        .map(|k| Header::secret("authorization", format!("Bearer {k}")))
        .into_iter()
        .collect();
    json_post(url, headers, &body, TIMEOUT)
}

/// The response body is the regions JSON itself.
pub(super) fn extract_text(body: &[u8]) -> Result<String, ProviderError> {
    let text = std::str::from_utf8(body).map_err(|_| invalid("response is not UTF-8"))?;
    if text.trim().is_empty() {
        return Err(invalid("response is empty"));
    }
    Ok(text.to_owned())
}
