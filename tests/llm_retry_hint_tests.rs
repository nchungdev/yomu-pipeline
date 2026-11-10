#![cfg(feature = "llm")]

use serde_json::json;
use std::time::{Duration, SystemTime};
use yomu_pipeline::llm::retry_hint::{
    best_hint, gemini_retry_delay, header_hint, message_hint, parse_go_duration,
    parse_retry_after_header, parse_retry_after_ms, MAX_HINT,
};

fn ms(s: &str) -> Option<u128> {
    parse_go_duration(s).map(|d| d.as_millis())
}

fn hdrs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

fn t0() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(784_111_777) // Sun, 06 Nov 1994 08:49:37 GMT
}

#[test]
fn go_duration_table() {
    let cases: &[(&str, Option<u128>)] = &[
        ("1m6.3s", Some(66_300)),
        ("6m0s", Some(360_000)),
        ("20ms", Some(20)),
        ("1h2m3s", Some(3_723_000)),
        ("36s", Some(36_000)),
        ("1.5s", Some(1_500)),
        ("5", Some(5_000)),
        ("2.5", Some(2_500)),
        ("500us", Some(0)),
        ("2000\u{b5}s", Some(2)),
        ("3000000ns", Some(3)),
        ("1h", Some(3_600_000)),
        (" 7s ", Some(7_000)),
        ("", None),
        (" ", None),
        ("-5s", None),
        ("+5s", None),
        ("abc", None),
        ("5x", None),
        ("1h2", None),
        ("s", None),
        (".", None),
        ("1..2s", None),
        ("1e3", None),
        ("99999999999999999999999999999999h", None),
        ("18446744073709551616", None),
    ];
    for (input, want) in cases {
        assert_eq!(ms(input), *want, "input {input:?}");
    }
}

#[test]
fn go_duration_exact_66_3_seconds() {
    assert_eq!(
        parse_go_duration("1m6.3s"),
        Some(Duration::new(66, 300_000_000))
    );
}

#[test]
fn retry_after_numeric_rounds_up() {
    let now = t0();
    assert_eq!(
        parse_retry_after_header("5", now),
        Some(Duration::from_secs(5))
    );
    assert_eq!(
        parse_retry_after_header("2.5", now),
        Some(Duration::from_secs(3))
    );
    assert_eq!(parse_retry_after_header("0", now), Some(Duration::ZERO));
    assert_eq!(
        parse_retry_after_header("3600", now),
        Some(Duration::from_secs(3600))
    );
    assert_eq!(parse_retry_after_header("-1", now), None);
    assert_eq!(parse_retry_after_header("", now), None);
    assert_eq!(parse_retry_after_header("soon", now), None);
}

#[test]
fn retry_after_http_date_future_and_past() {
    let now = t0();
    assert_eq!(
        parse_retry_after_header("Sun, 06 Nov 1994 08:50:37 GMT", now),
        Some(Duration::from_secs(60))
    );
    assert_eq!(
        parse_retry_after_header("Sun, 06 Nov 1994 08:00:00 GMT", now),
        Some(Duration::ZERO)
    );
}

#[test]
fn retry_after_ms_header() {
    assert_eq!(
        parse_retry_after_ms("1500"),
        Some(Duration::from_millis(1500))
    );
    assert_eq!(
        parse_retry_after_ms("0.5"),
        Some(Duration::from_micros(500))
    );
    assert_eq!(parse_retry_after_ms("1s"), None);
}

#[test]
fn header_names_are_case_insensitive_and_max_wins() {
    let h = hdrs(&[("RETRY-AFTER", "5"), ("Retry-After-Ms", "9000")]);
    assert_eq!(header_hint(&h, t0()), Some(Duration::from_secs(9)));
}

#[test]
fn ratelimit_reset_rule() {
    let now = t0();
    // remaining 0 -> reset used (and not clamped)
    let h = hdrs(&[
        ("x-ratelimit-remaining-requests", "0"),
        ("x-ratelimit-reset-requests", "6m0s"),
    ]);
    assert_eq!(header_hint(&h, now), Some(Duration::from_secs(360)));
    // quota remains -> reset ignored
    let h = hdrs(&[
        ("x-ratelimit-remaining-tokens", "1200"),
        ("x-ratelimit-reset-tokens", "20ms"),
    ]);
    assert_eq!(header_hint(&h, now), None);
    // remaining absent -> reset used
    let h = hdrs(&[("x-ratelimit-reset-tokens", "20ms")]);
    assert_eq!(header_hint(&h, now), Some(Duration::from_millis(20)));
    // unparseable remaining -> ignored
    let h = hdrs(&[
        ("x-ratelimit-remaining-tokens", "many"),
        ("x-ratelimit-reset-tokens", "5s"),
    ]);
    assert_eq!(header_hint(&h, now), None);
}

#[test]
fn gemini_retry_info() {
    let body = json!({"error": {"details": [
        {"@type": "type.googleapis.com/google.rpc.QuotaFailure"},
        {"@type": "type.googleapis.com/google.rpc.RetryInfo", "retryDelay": "36s"}
    ]}});
    assert_eq!(gemini_retry_delay(&body), Some(Duration::from_secs(36)));
    let body = json!({"error": {"details": [
        {"@type": "type.googleapis.com/google.rpc.RetryInfo", "retryDelay": "1.5s"}]}});
    assert_eq!(gemini_retry_delay(&body), Some(Duration::from_millis(1500)));
    assert_eq!(gemini_retry_delay(&json!({"error": {}})), None);
    assert_eq!(gemini_retry_delay(&json!([1, 2])), None);
}

#[test]
fn message_scan_is_conservative() {
    let m = |s: &str| message_hint(&json!({"error": {"message": s}}));
    assert_eq!(
        m("Please retry in 36.5s."),
        Some(Duration::from_millis(36_500))
    );
    assert_eq!(
        m("Rate limit. Try again in 1m6.3s."),
        Some(Duration::from_millis(66_300))
    );
    assert_eq!(m("try again in 20ms"), Some(Duration::from_millis(20)));
    assert_eq!(m("Please wait 30 seconds"), None);
    assert_eq!(m("try again in 2 minutes"), None);
    assert_eq!(m("retry in soon"), None);
    assert_eq!(
        m("\u{130}\u{130} retry in 5s"),
        Some(Duration::from_secs(5))
    );
}

#[test]
fn best_hint_is_max_unclamped_with_24h_cap() {
    let now = t0();
    let h = hdrs(&[("Retry-After", "3600")]);
    let body = json!({"error": {"details": [
        {"@type": "x.RetryInfo", "retryDelay": "36s"}]}});
    assert_eq!(
        best_hint(&h, Some(&body), now),
        Some(Duration::from_secs(3600))
    );
    // > 30 s preserved
    assert_eq!(
        best_hint(&hdrs(&[("retry-after", "45")]), None, now),
        Some(Duration::from_secs(45))
    );
    // absurd value capped
    assert_eq!(
        best_hint(&hdrs(&[("retry-after", "999999999")]), None, now),
        Some(MAX_HINT)
    );
    // message scan only as last resort
    let msg = json!({"error": {"message": "retry in 100s"}});
    assert_eq!(
        best_hint(&[], Some(&msg), now),
        Some(Duration::from_secs(100))
    );
    assert_eq!(
        best_hint(&hdrs(&[("retry-after", "5")]), Some(&msg), now),
        Some(Duration::from_secs(5))
    );
    assert_eq!(best_hint(&[], None, now), None);
}

#[test]
fn arbitrary_garbage_never_panics() {
    for s in [
        "\0",
        "\u{ffff}",
        "9".repeat(500).as_str(),
        "..",
        "1m.s",
        "µ",
        "ms",
        "1µ",
        "\n",
    ] {
        let _ = parse_go_duration(s);
        let _ = parse_retry_after_header(s, t0());
        let _ = parse_retry_after_ms(s);
    }
}
