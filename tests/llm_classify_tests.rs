#![cfg(feature = "llm")]

use serde_json::{json, Value};
use std::time::{Duration, SystemTime};
use yomu_pipeline::contract::v1::{ProviderError as E, ProviderKind as K};
use yomu_pipeline::llm::{classify_response, ResponseView};

fn now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(784_111_777) // Sun, 06 Nov 1994 08:49:37 GMT
}

fn run(kind: K, status: u16, headers: &[(&str, &str)], body: &[u8]) -> Option<E> {
    let h: Vec<(String, String)> = headers
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    classify_response(
        kind,
        &ResponseView {
            status,
            headers: &h,
            body,
        },
        now(),
    )
}

fn jrun(kind: K, status: u16, headers: &[(&str, &str)], body: Value) -> Option<E> {
    run(kind, status, headers, body.to_string().as_bytes())
}

fn gemini_429(quota_id: Option<&str>, message: &str, delay: Option<&str>) -> Value {
    let mut details = vec![];
    if let Some(id) = quota_id {
        details.push(
            json!({"@type": "type.googleapis.com/google.rpc.QuotaFailure",
            "violations": [{"quotaMetric": "m", "quotaId": id}]}),
        );
    }
    if let Some(d) = delay {
        details.push(json!({"@type": "type.googleapis.com/google.rpc.RetryInfo", "retryDelay": d}));
    }
    json!({"error": {"code": 429, "message": message, "status": "RESOURCE_EXHAUSTED",
        "details": details}})
}

#[test]
fn status_matrix_without_body() {
    let cases: &[(K, u16, E)] = &[
        (K::Gemini, 401, E::Auth { status: 401 }),
        (K::OpenAiCompatible, 403, E::Auth { status: 403 }),
        (K::CustomHttp, 401, E::Auth { status: 401 }),
        (
            K::OpenAiCompatible,
            402,
            E::QuotaExhausted {
                reset_hint_ms: None,
            },
        ),
        (K::OpenAiCompatible, 408, E::Timeout),
        (K::Gemini, 504, E::Timeout),
        (K::Gemini, 500, E::Server(500)),
        (K::OpenAiCompatible, 502, E::Server(502)),
        (K::OpenAiCompatible, 529, E::Server(529)),
        (
            K::Gemini,
            429,
            E::RateLimited {
                retry_after_ms: None,
            },
        ),
        (
            K::OpenAiCompatible,
            429,
            E::RateLimited {
                retry_after_ms: None,
            },
        ),
    ];
    for (kind, status, want) in cases {
        assert_eq!(
            run(*kind, *status, &[], b""),
            Some(want.clone()),
            "{kind:?} {status}"
        );
    }
}

#[test]
fn bad_request_statuses_keep_the_error_message() {
    for status in [400, 404, 413, 422] {
        let body = json!({"error": {"message": "model `x` not found"}});
        match jrun(K::OpenAiCompatible, status, &[], body) {
            Some(E::BadRequest(m)) => assert!(m.contains("model `x` not found"), "{m}"),
            other => panic!("status {status}: {other:?}"),
        }
    }
    // plain-text 4xx body is not lost either
    match run(K::CustomHttp, 404, &[], b"no such route") {
        Some(E::BadRequest(m)) => assert!(m.contains("no such route")),
        other => panic!("{other:?}"),
    }
    // empty body -> status only
    assert_eq!(
        run(K::Gemini, 404, &[], b""),
        Some(E::BadRequest("HTTP 404".into()))
    );
}

#[test]
fn gemini_400_api_key_invalid_is_auth() {
    let body = json!({"error": {"code": 400, "message": "API key not valid.",
        "details": [{"@type": "type.googleapis.com/google.rpc.ErrorInfo",
                     "reason": "API_KEY_INVALID"}]}});
    assert_eq!(
        jrun(K::Gemini, 400, &[], body.clone()),
        Some(E::Auth { status: 400 })
    );
    // same body from a non-Gemini provider stays a bad request
    assert!(matches!(
        jrun(K::OpenAiCompatible, 400, &[], body),
        Some(E::BadRequest(_))
    ));
}

#[test]
fn gemini_429_per_day_vs_per_minute_ignores_billing_word() {
    let quota_id_day = "GenerateRequestsPerDayPerProjectPerModel-FreeTier";
    let quota_id_min = "GenerateRequestsPerMinutePerProjectPerModel-FreeTier";
    for msg in ["Quota exceeded.", "Check your plan and billing details."] {
        let day = gemini_429(Some(quota_id_day), msg, Some("36s"));
        assert_eq!(
            jrun(K::Gemini, 429, &[], day),
            Some(E::QuotaExhausted {
                reset_hint_ms: Some(36_000)
            }),
            "{msg}"
        );
        let min = gemini_429(Some(quota_id_min), msg, Some("1.5s"));
        assert_eq!(
            jrun(K::Gemini, 429, &[], min),
            Some(E::RateLimited {
                retry_after_ms: Some(1_500)
            }),
            "{msg}"
        );
    }
    // no violations -> rate limited, even with "billing"
    let none = gemini_429(None, "see billing", None);
    assert_eq!(
        jrun(K::Gemini, 429, &[], none),
        Some(E::RateLimited {
            retry_after_ms: None
        })
    );
}

#[test]
fn gemini_per_day_falls_back_to_retry_after_header_and_keeps_long_values() {
    let day = gemini_429(Some("RequestsPerDay"), "x", None);
    assert_eq!(
        jrun(K::Gemini, 429, &[("retry-after", "3600")], day),
        Some(E::QuotaExhausted {
            reset_hint_ms: Some(3_600_000)
        })
    );
    let min = gemini_429(Some("RequestsPerMinute"), "x", Some("1m6.3s"));
    assert_eq!(
        jrun(K::Gemini, 429, &[], min),
        Some(E::RateLimited {
            retry_after_ms: Some(66_300)
        })
    );
}

#[test]
fn openai_429_quota_vs_rate_limit() {
    let q = json!({"error": {"code": "insufficient_quota", "type": "x", "message": "m"}});
    assert!(matches!(
        jrun(K::OpenAiCompatible, 429, &[], q),
        Some(E::QuotaExhausted { .. })
    ));
    let q2 = json!({"error": {"type": "insufficient_quota", "code": null, "message": "m"}});
    assert!(matches!(
        jrun(K::CustomHttp, 429, &[], q2),
        Some(E::QuotaExhausted { .. })
    ));
    let rl = json!({"error": {"code": "rate_limit_exceeded", "type": "requests",
        "message": "Rate limit reached. Please add billing details; try again in 6m0s."}});
    assert_eq!(
        jrun(K::OpenAiCompatible, 429, &[], rl),
        Some(E::RateLimited {
            retry_after_ms: Some(360_000)
        })
    );
    let unknown = json!({"error": {"message": "billing issue"}});
    assert!(matches!(
        jrun(K::OpenAiCompatible, 429, &[], unknown),
        Some(E::RateLimited { .. })
    ));
}

#[test]
fn retry_after_header_variants_on_429() {
    let b = json!({});
    let cases: &[(&str, u64)] = &[
        ("5", 5_000),
        ("2.5", 3_000),
        ("Sun, 06 Nov 1994 08:50:37 GMT", 60_000),
        ("Sun, 06 Nov 1994 08:00:00 GMT", 0),
    ];
    for (v, want) in cases {
        assert_eq!(
            jrun(K::OpenAiCompatible, 429, &[("Retry-After", v)], b.clone()),
            Some(E::RateLimited {
                retry_after_ms: Some(*want)
            }),
            "{v}"
        );
    }
    assert_eq!(
        jrun(
            K::OpenAiCompatible,
            429,
            &[
                ("x-ratelimit-remaining-requests", "0"),
                ("X-RateLimit-Reset-Requests", "1m6.3s")
            ],
            b
        ),
        Some(E::RateLimited {
            retry_after_ms: Some(66_300)
        })
    );
}

#[test]
fn success_with_content_block() {
    let g_prompt = json!({"promptFeedback": {"blockReason": "SAFETY"}});
    let g_prohibited = json!({"candidates": [{"finishReason": "STOP"},
        {"finishReason": "PROHIBITED_CONTENT", "content": {"parts": [{"text": "user text"}]}}]});
    let o =
        json!({"choices": [{"finish_reason": "content_filter", "message": {"content": "secret"}}]});
    for (b, reason) in [
        (g_prompt, "SAFETY"),
        (g_prohibited, "PROHIBITED_CONTENT"),
        (o, "content_filter"),
    ] {
        assert_eq!(
            jrun(K::Gemini, 200, &[], b),
            Some(E::ContentBlocked {
                reason: reason.into()
            })
        );
    }
    for r in ["SAFETY", "BLOCKLIST", "PROHIBITED_CONTENT", "SPII"] {
        let b = json!({"candidates": [{"finishReason": r}]});
        assert_eq!(
            jrun(K::Gemini, 200, &[], b),
            Some(E::ContentBlocked { reason: r.into() })
        );
    }
}

#[test]
fn success_variants() {
    let ok = json!({"candidates": [{"finishReason": "STOP", "content": {"parts": []}}]});
    assert_eq!(jrun(K::Gemini, 200, &[], ok), None);
    let ok = json!({"choices": [{"finish_reason": "stop"}]});
    assert_eq!(jrun(K::OpenAiCompatible, 200, &[], ok), None);
    let unspecified = json!({"promptFeedback": {"blockReason": "BLOCK_REASON_UNSPECIFIED"}});
    assert_eq!(jrun(K::Gemini, 200, &[], unspecified), None);
    for body in [&b""[..], b"  \n", b"<html>oops</html>", b"{\"a\":"] {
        assert!(matches!(
            run(K::Gemini, 200, &[], body),
            Some(E::InvalidResponse(_))
        ));
    }
}

#[test]
fn invalid_response_does_not_echo_body() {
    match run(K::Gemini, 200, &[], b"top-secret-user-text <html>") {
        Some(E::InvalidResponse(m)) => assert!(!m.contains("top-secret"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn messages_are_sanitized() {
    let body = json!({"error": {"message":
        "bad key AIzaSyA1234567890abcdefghijklmnopqrstu at https://example.com/v1?key=abc \
         and sk-proj-abcdefghijklmnop1234 plus data:image/png;base64,AAAA end"}});
    let Some(E::BadRequest(m)) = jrun(K::Gemini, 400, &[], body) else {
        panic!("not bad request")
    };
    for leaked in ["AIza", "example.com", "sk-proj", "base64", "https"] {
        assert!(!m.contains(leaked), "leaked {leaked}: {m}");
    }
    assert!(
        m.contains("[redacted]") && m.contains("[url]") && m.contains("[data]"),
        "{m}"
    );
    assert!(m.ends_with("end"));
}

#[test]
fn long_messages_are_truncated_to_512_chars() {
    let long = "word ".repeat(1000);
    let Some(E::BadRequest(m)) = jrun(K::Gemini, 400, &[], json!({"error": {"message": long}}))
    else {
        panic!()
    };
    // "HTTP 400: " prefix plus at most 512 chars of provider text
    assert!(
        m.chars().count() <= "HTTP 400: ".len() + 512,
        "{}",
        m.chars().count()
    );
}

#[test]
fn arbitrary_bytes_never_panic() {
    let junk: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
    for kind in [K::Gemini, K::OpenAiCompatible, K::CustomHttp] {
        for status in [
            0u16, 100, 200, 204, 301, 400, 402, 418, 429, 500, 529, 599, 999,
        ] {
            let _ = run(kind, status, &[("Retry-After", "\u{0}")], &junk);
            let _ = run(kind, status, &[], b"");
            let _ = run(kind, status, &[], b"null");
            let _ = run(kind, status, &[], b"[1,2,3]");
            let _ = run(kind, status, &[], br#"{"error": 5}"#);
        }
    }
}

#[test]
fn unexpected_statuses() {
    assert!(matches!(
        run(K::Gemini, 301, &[], b""),
        Some(E::InvalidResponse(_))
    ));
    assert!(matches!(
        run(K::Gemini, 418, &[], b""),
        Some(E::BadRequest(_))
    ));
}
