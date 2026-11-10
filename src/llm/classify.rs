//! Typed classification of a provider HTTP response into `Option<ProviderError>`.
//!
//! Priority: HTTP status, then structured JSON body, then headers. Free text is only ever used
//! for the retry hint (see `retry_hint::message_hint`), never to pick the error kind.

use super::retry_hint::{best_hint, cap_hint, gemini_retry_delay, retry_after_header};
use super::sanitize::sanitize_message;
use crate::contract::v1::{ProviderError, ProviderKind};
use serde_json::Value;
use std::time::{Duration, SystemTime};

/// Borrowed view of a received response. Header names are matched case-insensitively.
#[derive(Debug, Clone, Copy)]
pub struct ResponseView<'a> {
    pub status: u16,
    pub headers: &'a [(String, String)],
    pub body: &'a [u8],
}

const BLOCKING_FINISH_REASONS: [&str; 4] = ["SAFETY", "BLOCKLIST", "PROHIBITED_CONTENT", "SPII"];
const MAX_BODY_ECHO_BYTES: usize = 2048;

/// `None` means the response is a usable success.
pub fn classify_response(
    kind: ProviderKind,
    view: &ResponseView,
    now: SystemTime,
) -> Option<ProviderError> {
    let json: Option<Value> = serde_json::from_slice(view.body).ok();
    let body = json.as_ref();
    let hint_ms = || best_hint(view.headers, body, now).map(to_ms);
    let gemini = kind == ProviderKind::Gemini;
    Some(match view.status {
        200..=299 => return classify_success(view, body),
        401 | 403 => ProviderError::Auth {
            status: view.status,
        },
        400 if gemini && has_error_reason(body, "API_KEY_INVALID") => {
            ProviderError::Auth { status: 400 }
        }
        402 => ProviderError::QuotaExhausted {
            reset_hint_ms: hint_ms(),
        },
        429 if gemini => classify_gemini_429(view, body, now),
        429 => classify_openai_429(body, hint_ms()),
        408 | 504 => ProviderError::Timeout,
        500..=599 => ProviderError::Server(view.status),
        400..=499 => ProviderError::BadRequest(error_message(view, body)),
        _ => ProviderError::InvalidResponse(format!("unexpected HTTP status {}", view.status)),
    })
}

fn to_ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

fn classify_openai_429(body: Option<&Value>, hint_ms: Option<u64>) -> ProviderError {
    let is_quota = ["/error/code", "/error/type"].iter().any(|p| {
        body.and_then(|b| b.pointer(p))
            .and_then(Value::as_str)
            .is_some_and(|s| s == "insufficient_quota")
    });
    if is_quota {
        ProviderError::QuotaExhausted {
            reset_hint_ms: hint_ms,
        }
    } else {
        ProviderError::RateLimited {
            retry_after_ms: hint_ms,
        }
    }
}

fn classify_gemini_429(
    view: &ResponseView,
    body: Option<&Value>,
    now: SystemTime,
) -> ProviderError {
    let daily = body.is_some_and(has_daily_violation);
    if daily {
        let reset = body
            .and_then(gemini_retry_delay)
            .or_else(|| retry_after_header(view.headers, now))
            .or_else(|| best_hint(view.headers, body, now))
            .map(|d| to_ms(cap_hint(d)));
        return ProviderError::QuotaExhausted {
            reset_hint_ms: reset,
        };
    }
    ProviderError::RateLimited {
        retry_after_ms: best_hint(view.headers, body, now).map(to_ms),
    }
}

fn details(body: &Value) -> impl Iterator<Item = &Value> {
    body.pointer("/error/details")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn has_daily_violation(body: &Value) -> bool {
    details(body)
        .filter(|d| {
            d.get("@type")
                .and_then(Value::as_str)
                .is_some_and(|t| t.ends_with("QuotaFailure"))
        })
        .filter_map(|d| d.get("violations")?.as_array())
        .flatten()
        .filter_map(|v| v.get("quotaId")?.as_str())
        .any(|id| id.contains("PerDay"))
}

fn has_error_reason(body: Option<&Value>, reason: &str) -> bool {
    body.is_some_and(|b| {
        details(b).any(|d| d.get("reason").and_then(Value::as_str) == Some(reason))
    })
}

/// Short sanitized message: structured `error.message` first, else the sanitized start of the
/// body, else just the status. Never the full body.
fn error_message(view: &ResponseView, body: Option<&Value>) -> String {
    let structured = body.and_then(|j| {
        j.pointer("/error/message")
            .or_else(|| j.get("error"))
            .or_else(|| j.get("message"))
            .and_then(Value::as_str)
    });
    let text = match structured {
        Some(m) => m.to_owned(),
        None => {
            let head = &view.body[..view.body.len().min(MAX_BODY_ECHO_BYTES)];
            String::from_utf8_lossy(head).into_owned()
        }
    };
    let clean = sanitize_message(&text);
    if clean.is_empty() {
        format!("HTTP {}", view.status)
    } else {
        format!("HTTP {}: {clean}", view.status)
    }
}

fn classify_success(view: &ResponseView, body: Option<&Value>) -> Option<ProviderError> {
    if view.body.iter().all(u8::is_ascii_whitespace) {
        return Some(ProviderError::InvalidResponse(
            "empty response body".to_owned(),
        ));
    }
    let Some(json) = body else {
        return Some(ProviderError::InvalidResponse(
            "response body is not valid JSON".to_owned(),
        ));
    };
    blocked_reason(json).map(|reason| ProviderError::ContentBlocked { reason })
}

/// Enum strings only (Gemini `blockReason` / `finishReason`, OpenAI `content_filter`); never
/// any model or user text. Both shapes are checked for every provider kind.
fn blocked_reason(json: &Value) -> Option<String> {
    if let Some(r) = json
        .pointer("/promptFeedback/blockReason")
        .and_then(Value::as_str)
        .filter(|r| *r != "BLOCK_REASON_UNSPECIFIED")
    {
        return Some(enum_string(r));
    }
    let gemini = json
        .get("candidates")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| c.get("finishReason")?.as_str())
        .find(|r| BLOCKING_FINISH_REASONS.contains(r));
    if let Some(r) = gemini {
        return Some(enum_string(r));
    }
    json.get("choices")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|c| c.get("finish_reason").and_then(Value::as_str) == Some("content_filter"))
        .then(|| "content_filter".to_owned())
}

fn enum_string(raw: &str) -> String {
    let s: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .take(64)
        .collect();
    if s.is_empty() {
        "UNKNOWN".to_owned()
    } else {
        s
    }
}
