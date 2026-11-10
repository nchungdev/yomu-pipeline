//! Parsers for provider retry hints. Every parser returns `None` on garbage and never panics.
//!
//! Hints are NOT clamped to a short wait (the app decides how long to sleep); only values above
//! [`MAX_HINT`] (24 h) are reduced, as a guard against absurd input.

use serde_json::Value;
use std::time::{Duration, SystemTime};

/// Upper bound applied by [`cap_hint`] and [`best_hint`].
pub const MAX_HINT: Duration = Duration::from_secs(24 * 3600);

const NANOS_PER_SEC: u128 = 1_000_000_000;

pub fn cap_hint(d: Duration) -> Duration {
    d.min(MAX_HINT)
}

/// Case-insensitive header lookup; the value is trimmed.
pub fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.trim())
}

fn is_plain_number(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit() || b == b'.')
}

/// `digits[.digits]` times `scale` nanoseconds, in exact integer arithmetic (no float error).
fn decimal_to_nanos(num: &str, scale: u128) -> Option<u128> {
    let (int, frac) = num.split_once('.').unwrap_or((num, ""));
    if (int.is_empty() && frac.is_empty()) || frac.contains('.') {
        return None;
    }
    let int_v: u128 = if int.is_empty() { 0 } else { int.parse().ok()? };
    let mut total = int_v.checked_mul(scale)?;
    let digits = &frac[..frac.len().min(18)];
    if !digits.is_empty() {
        let f: u128 = digits.parse().ok()?;
        let exp = u32::try_from(digits.len()).ok()?;
        total = total.checked_add(f.checked_mul(scale)? / 10u128.pow(exp))?;
    }
    Some(total)
}

fn nanos_to_duration(nanos: u128) -> Option<Duration> {
    let secs = u64::try_from(nanos / NANOS_PER_SEC).ok()?;
    let sub = u32::try_from(nanos % NANOS_PER_SEC).ok()?;
    Some(Duration::new(secs, sub))
}

fn unit_nanos(unit: &str) -> Option<u128> {
    Some(match unit {
        "ns" => 1,
        "us" | "\u{b5}s" | "\u{3bc}s" => 1_000,
        "ms" => 1_000_000,
        "s" => NANOS_PER_SEC,
        "m" => 60 * NANOS_PER_SEC,
        "h" => 3600 * NANOS_PER_SEC,
        _ => return None,
    })
}

/// Go-style duration (`1m6.3s`, `6m0s`, `20ms`, `1h2m3s`, `36s`). A bare number means seconds.
/// Rejects empty, negative, signed, unit-less segments, unknown units and overflow.
pub fn parse_go_duration(input: &str) -> Option<Duration> {
    let s = input.trim();
    if is_plain_number(s) {
        return nanos_to_duration(decimal_to_nanos(s, NANOS_PER_SEC)?);
    }
    let mut rest = s;
    let mut total: u128 = 0;
    while !rest.is_empty() {
        let num_end = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        let (num, tail) = rest.split_at(num_end);
        let unit_end = tail
            .find(|c: char| c.is_ascii_digit() || c == '.')
            .unwrap_or(tail.len());
        let (unit, next) = tail.split_at(unit_end);
        total = total.checked_add(decimal_to_nanos(num, unit_nanos(unit)?)?)?;
        rest = next;
    }
    if s.is_empty() {
        return None;
    }
    nanos_to_duration(total)
}

/// `Retry-After`: delta-seconds (integer or decimal, rounded UP) or an HTTP-date
/// (`max(0, date - now)`; a past date gives zero).
pub fn parse_retry_after_header(value: &str, now: SystemTime) -> Option<Duration> {
    let v = value.trim();
    if is_plain_number(v) {
        let d = parse_go_duration(v)?;
        let secs = d.as_secs().saturating_add(u64::from(d.subsec_nanos() > 0));
        return Some(Duration::from_secs(secs));
    }
    let date = httpdate::parse_http_date(v).ok()?;
    Some(date.duration_since(now).unwrap_or(Duration::ZERO))
}

/// `retry-after-ms`: integer or decimal milliseconds.
pub fn parse_retry_after_ms(value: &str) -> Option<Duration> {
    let v = value.trim();
    if !is_plain_number(v) {
        return None;
    }
    nanos_to_duration(decimal_to_nanos(v, 1_000_000)?)
}

/// `x-ratelimit-reset-<resource>` (`requests` or `tokens`, Go duration). Rule: used when the
/// matching `x-ratelimit-remaining-<resource>` header is absent or parses to 0 or less; when
/// quota remains (or the remaining value is unparseable) the reset says nothing about a wait.
fn ratelimit_reset(headers: &[(String, String)], resource: &str) -> Option<Duration> {
    let reset = header_value(headers, &format!("x-ratelimit-reset-{resource}"))?;
    if let Some(rem) = header_value(headers, &format!("x-ratelimit-remaining-{resource}")) {
        if rem.parse::<f64>().ok()? > 0.0 {
            return None;
        }
    }
    parse_go_duration(reset)
}

/// Largest hint found in `Retry-After`, `retry-after-ms` and the `x-ratelimit-reset-*` headers.
pub fn header_hint(headers: &[(String, String)], now: SystemTime) -> Option<Duration> {
    [
        retry_after_header(headers, now),
        header_value(headers, "retry-after-ms").and_then(parse_retry_after_ms),
        ratelimit_reset(headers, "requests"),
        ratelimit_reset(headers, "tokens"),
    ]
    .into_iter()
    .flatten()
    .max()
}

/// Parsed `Retry-After` header alone.
pub fn retry_after_header(headers: &[(String, String)], now: SystemTime) -> Option<Duration> {
    header_value(headers, "retry-after").and_then(|v| parse_retry_after_header(v, now))
}

/// Gemini `error.details[]` entry of type `...RetryInfo` with a `retryDelay` ("36s", "1.5s").
pub fn gemini_retry_delay(body: &Value) -> Option<Duration> {
    body.pointer("/error/details")?
        .as_array()?
        .iter()
        .filter(|d| {
            d.get("@type")
                .and_then(Value::as_str)
                .is_some_and(|t| t.ends_with("RetryInfo"))
        })
        .filter_map(|d| d.get("retryDelay")?.as_str().and_then(parse_go_duration))
        .max()
}

/// Last resort: `error.message` containing "retry in <dur>" / "try again in <dur>" where `<dur>`
/// is a Go duration WITH a unit (a bare number is ambiguous in prose and ignored).
pub fn message_hint(body: &Value) -> Option<Duration> {
    let msg = body.pointer("/error/message")?.as_str()?;
    let lower = msg.to_ascii_lowercase(); // ASCII-only: byte offsets stay valid for `msg`
    ["retry in ", "try again in "]
        .iter()
        .filter_map(|marker| {
            let start = lower.find(marker)? + marker.len();
            let token: String = msg[start..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '.')
                .collect();
            let token = token.trim_end_matches('.');
            if is_plain_number(token) {
                return None;
            }
            parse_go_duration(token)
        })
        .max()
}

/// Maximum among the hints present, capped at [`MAX_HINT`]. Headers and Gemini `RetryInfo` are
/// authoritative; the prose scan only counts when neither produced anything.
pub fn best_hint(
    headers: &[(String, String)],
    body: Option<&Value>,
    now: SystemTime,
) -> Option<Duration> {
    let structured = header_hint(headers, now)
        .into_iter()
        .chain(body.and_then(gemini_retry_delay))
        .max();
    structured
        .or_else(|| body.and_then(message_hint))
        .map(cap_hint)
}
