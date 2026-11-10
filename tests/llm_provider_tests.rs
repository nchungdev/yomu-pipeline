#![cfg(feature = "test-util")]

#[path = "common/llm_provider.rs"]
mod support;

use base64::Engine;
use serde_json::{json, Value};
use support::*;
use yomu_pipeline::contract::v1::{CancelToken, ProviderError as E, ProviderKind as K};
use yomu_pipeline::llm::{validate_endpoint, CallError};

fn gemini_spec(endpoint: Option<&str>) -> yomu_pipeline::contract::v1::ProviderSpec {
    spec(K::Gemini, endpoint, "gemini-2.5-flash", Some(KEY))
}

fn openai_spec(endpoint: &str) -> yomu_pipeline::contract::v1::ProviderSpec {
    spec(K::OpenAiCompatible, Some(endpoint), "gpt-test", Some(KEY))
}

fn gemini_ok(text: &str) -> String {
    json!({"candidates":[{"content":{"parts":[{"text":text}]},"finishReason":"STOP"}]}).to_string()
}

fn openai_ok(content: Value) -> String {
    json!({"choices":[{"message":{"role":"assistant","content":content},"finish_reason":"stop"}]})
        .to_string()
}

fn provider_err(run: Run) -> E {
    match run.result {
        Err(CallError::Provider(e)) => e,
        other => panic!("expected provider error, got {other:?}"),
    }
}

fn b64(s: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .unwrap_or_default()
}

#[tokio::test]
async fn gemini_request_shape_default_endpoint() {
    let r = run(vec![ok(&gemini_ok(REGIONS_JSON))], &gemini_spec(None)).await;
    assert_eq!(r.result.map(|v| v.len()), Ok(2));
    let req = &r.requests[0];
    assert_eq!(
        req.url,
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent"
    );
    assert!(!req.url.contains(KEY));
    let key = header(req, "x-goog-api-key").expect("key header");
    assert_eq!(key.value, KEY);
    assert!(key.sensitive);
    assert!(req.headers.iter().filter(|h| h.value.contains(KEY)).count() == 1);
    let body = body_json(req);
    let parts = &body["contents"][0]["parts"];
    assert_eq!(parts[0]["text"], PROMPT);
    assert_eq!(parts[1]["inlineData"]["mimeType"], "image/jpeg");
    let data = parts[1]["inlineData"]["data"].as_str().unwrap_or_default();
    assert_eq!(b64(data), IMAGE);
    let cfg = &body["generationConfig"];
    assert_eq!(cfg["responseMimeType"], "application/json");
    assert_eq!(cfg["responseSchema"]["type"], "OBJECT");
    assert_eq!(cfg["thinkingConfig"]["thinkingBudget"], 0);
}

#[tokio::test]
async fn gemini_overridden_endpoint_and_pro_model() {
    let s = spec(
        K::Gemini,
        Some("http://127.0.0.1:8080/proxy/"),
        "gemini-2.5-pro",
        Some(KEY),
    );
    let r = run(vec![ok(&gemini_ok(REGIONS_JSON))], &s).await;
    assert!(r.result.is_ok());
    assert_eq!(
        r.requests[0].url,
        "http://127.0.0.1:8080/proxy/v1beta/models/gemini-2.5-pro:generateContent"
    );
    assert!(body_json(&r.requests[0])["generationConfig"]
        .get("thinkingConfig")
        .is_none());
}

#[tokio::test]
async fn gemini_missing_key_and_bad_model_make_no_request() {
    let no_key = spec(K::Gemini, None, "gemini-2.5-flash", None);
    let r = run(vec![], &no_key).await;
    assert!(r.requests.is_empty());
    assert_eq!(provider_err(r), E::Auth { status: 401 });
    let bad = spec(K::Gemini, None, "x/../y?k=1", Some(KEY));
    let r = run(vec![], &bad).await;
    assert!(r.requests.is_empty());
    assert_eq!(provider_err(r), E::BadRequest("invalid model".into()));
}

#[tokio::test]
async fn gemini_success_with_fence_and_multipart() {
    let fenced = format!("Here you go:\n```json\n{REGIONS_JSON}\n```");
    let (a, b) = fenced.split_at(fenced.len() / 2);
    let body = json!({"candidates":[{"content":{"parts":[
        {"text":"thinking...","thought":true},{"text":a},{"text":b}]}}]})
    .to_string();
    let r = run(vec![ok(&body)], &gemini_spec(None)).await;
    let regions = r.result.expect("regions");
    assert_eq!(regions.len(), 2);
    assert_eq!(regions[0].translated, "Xin chào");
    assert_eq!(regions[0].speaker.as_deref(), Some("Aki"));
}

#[tokio::test]
async fn openai_request_shape() {
    let r = run(
        vec![ok(&openai_ok(json!(REGIONS_JSON)))],
        &openai_spec("https://llm.example.test/v1/"),
    )
    .await;
    assert_eq!(r.result.map(|v| v.len()), Ok(2));
    let req = &r.requests[0];
    assert_eq!(req.url, "https://llm.example.test/v1/chat/completions");
    let auth = header(req, "authorization").expect("auth header");
    assert_eq!(auth.value, format!("Bearer {KEY}"));
    assert!(auth.sensitive);
    assert!(header(req, "x-goog-api-key").is_none());
    let body = body_json(req);
    assert_eq!(body["model"], "gpt-test");
    let content = &body["messages"][0]["content"];
    assert_eq!(content[0]["text"], PROMPT);
    let uri = content[1]["image_url"]["url"].as_str().unwrap_or_default();
    let data = uri
        .strip_prefix("data:image/jpeg;base64,")
        .expect("data uri");
    assert_eq!(b64(data), IMAGE);
    assert_eq!(body["response_format"]["type"], "json_schema");
    assert_eq!(body["response_format"]["json_schema"]["strict"], true);
}

#[tokio::test]
async fn openai_endpoint_with_suffix_and_no_key() {
    let s = spec(
        K::OpenAiCompatible,
        Some("http://192.168.1.20:11434/v1/chat/completions"),
        "",
        None,
    );
    let r = run(vec![ok(&openai_ok(json!(REGIONS_JSON)))], &s).await;
    assert!(r.result.is_ok());
    assert_eq!(
        r.requests[0].url,
        "http://192.168.1.20:11434/v1/chat/completions"
    );
    assert!(header(&r.requests[0], "authorization").is_none());
    assert_eq!(body_json(&r.requests[0])["model"], "gpt-4o-mini");
}

#[tokio::test]
async fn openai_array_content_and_missing_endpoint() {
    let parts = json!([{"type":"text","text":"```json\n"},{"type":"text","text":REGIONS_JSON},{"type":"text","text":"\n```"}]);
    let r = run(
        vec![ok(&openai_ok(parts))],
        &openai_spec("https://h.example.test"),
    )
    .await;
    assert_eq!(r.result.map(|v| v.len()), Ok(2));
    let none = spec(K::OpenAiCompatible, None, "m", Some(KEY));
    let r = run(vec![], &none).await;
    assert_eq!(provider_err(r), E::BadRequest("invalid endpoint".into()));
}

#[tokio::test]
async fn auth_and_rate_limit_mapping() {
    let s = gemini_spec(None);
    for status in [401, 403] {
        let r = run(vec![resp(status, &[], "{}")], &s).await;
        assert_eq!(r.requests.len(), 1);
        assert_eq!(provider_err(r), E::Auth { status });
    }
    let per_min = r#"{"error":{"details":[{"@type":"type.googleapis.com/google.rpc.QuotaFailure","violations":[{"quotaId":"GenerateRequestsPerMinutePerProjectPerModel"}]},{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"120s"}]}}"#;
    let r = run(vec![resp(429, &[], per_min)], &s).await;
    assert_eq!(
        provider_err(r),
        E::RateLimited {
            retry_after_ms: Some(120_000)
        }
    );
    let per_day = r#"{"error":{"details":[{"@type":"type.googleapis.com/google.rpc.QuotaFailure","violations":[{"quotaId":"GenerateRequestsPerDayPerProjectPerModel"}]}]}}"#;
    let r = run(vec![resp(429, &[], per_day)], &s).await;
    assert!(matches!(provider_err(r), E::QuotaExhausted { .. }));
    let o = openai_spec("https://h.example.test");
    let rl = r#"{"error":{"code":"rate_limit_exceeded","message":"slow down"}}"#;
    let r = run(vec![resp(429, &[("retry-after", "90")], rl)], &o).await;
    assert_eq!(
        provider_err(r),
        E::RateLimited {
            retry_after_ms: Some(90_000)
        }
    );
    let q = r#"{"error":{"code":"insufficient_quota"}}"#;
    let r = run(vec![resp(429, &[], q)], &o).await;
    assert!(matches!(provider_err(r), E::QuotaExhausted { .. }));
}

#[tokio::test]
async fn content_blocked_mapping() {
    let g = r#"{"candidates":[{"finishReason":"SAFETY"}]}"#;
    let r = run(vec![ok(g)], &gemini_spec(None)).await;
    assert_eq!(
        provider_err(r),
        E::ContentBlocked {
            reason: "SAFETY".into()
        }
    );
    let pf = r#"{"promptFeedback":{"blockReason":"OTHER"}}"#;
    let r = run(vec![ok(pf)], &gemini_spec(None)).await;
    assert_eq!(
        provider_err(r),
        E::ContentBlocked {
            reason: "OTHER".into()
        }
    );
    let o = r#"{"choices":[{"message":{"content":null},"finish_reason":"content_filter"}]}"#;
    let r = run(vec![ok(o)], &openai_spec("https://h.example.test")).await;
    assert_eq!(
        provider_err(r),
        E::ContentBlocked {
            reason: "content_filter".into()
        }
    );
    let refusal =
        r#"{"choices":[{"message":{"content":null,"refusal":"no"},"finish_reason":"stop"}]}"#;
    let r = run(vec![ok(refusal)], &openai_spec("https://h.example.test")).await;
    assert!(matches!(provider_err(r), E::ContentBlocked { .. }));
}

#[tokio::test]
async fn server_error_retried_up_to_policy_then_fails() {
    let script = vec![
        resp(500, &[], "x"),
        resp(500, &[], "x"),
        resp(500, &[], "x"),
        resp(500, &[], "x"),
    ];
    let r = run(script, &gemini_spec(None)).await;
    assert_eq!(r.requests.len(), 3);
    assert_eq!(provider_err(r), E::Server(500));
    let script = vec![resp(503, &[], "x"), ok(&gemini_ok(REGIONS_JSON))];
    let r = run(script, &gemini_spec(None)).await;
    assert_eq!(r.requests.len(), 2);
    assert!(r.result.is_ok());
}

#[tokio::test]
async fn invalid_responses_are_not_retried() {
    let g = gemini_spec(None);
    let o = openai_spec("https://h.example.test");
    let cases: Vec<(&yomu_pipeline::contract::v1::ProviderSpec, String)> = vec![
        (&g, r#"{"candidates":[]}"#.into()),
        (&g, gemini_ok("   ")),
        (&g, gemini_ok("sorry, I cannot do that")),
        (
            &g,
            gemini_ok(
                r#"{"regions":[{"id":"x","text":"a","translated_text":"","box_2d":[0,0,10,10]}]}"#,
            ),
        ),
        (&o, r#"{"choices":[]}"#.into()),
        (&o, openai_ok(json!(""))),
        (&o, openai_ok(json!("not json at all"))),
        (
            &o,
            openai_ok(json!(
                r#"{"regions":[{"id":"x","text":"a","translated_text":"b"}]}"#
            )),
        ),
    ];
    for (s, body) in cases {
        let r = run(vec![ok(&body), ok(&body)], s).await;
        assert_eq!(r.requests.len(), 1, "no retry for {body}");
        assert!(matches!(provider_err(r), E::InvalidResponse(_)), "{body}");
    }
}

#[tokio::test]
async fn declared_empty_regions_is_success() {
    let r = run(
        vec![ok(&gemini_ok(r#"{"regions":[]}"#))],
        &gemini_spec(None),
    )
    .await;
    assert_eq!(r.result, Ok(vec![]));
}

#[tokio::test]
async fn secret_never_leaks() {
    let leaky =
        format!(r#"{{"error":{{"message":"bad key {KEY} at https://x.test/?key={KEY}"}}}}"#);
    let specs = [gemini_spec(None), openai_spec("https://h.example.test")];
    for s in &specs {
        for script in [
            vec![resp(400, &[], &leaky)],
            vec![resp(401, &[], &leaky)],
            vec![ok("not json")],
        ] {
            let r = run(script, s).await;
            let req_dbg = format!("{:?} {}", r.requests[0], r.requests[0]);
            assert!(!req_dbg.contains(KEY), "request debug leaks key");
            assert!(!r.requests[0].url.contains(KEY));
            let err = provider_err(r);
            let text = format!("{err:?} {err}");
            assert!(!text.contains(KEY), "error leaks key: {text}");
        }
    }
    let bad_key = spec(K::Gemini, None, "m", Some("bad key\n"));
    let err = provider_err(run(vec![], &bad_key).await);
    assert!(!format!("{err:?}").contains("bad key"));
}

#[tokio::test]
async fn cancellation_passes_through() {
    let cancel = CancelToken::default();
    cancel.cancel();
    let r = run_with(
        vec![ok(&gemini_ok(REGIONS_JSON))],
        &gemini_spec(None),
        &cancel,
    )
    .await;
    assert_eq!(r.result, Err(CallError::Cancelled));
    assert!(r.requests.is_empty());
}

#[test]
fn endpoint_validation() {
    for good in [
        "https://api.example.com/v1",
        "http://localhost:11434",
        "http://127.0.0.1:8080/x",
        "http://[::1]:8080",
        "http://192.168.0.5:5000",
        "http://10.1.2.3",
        "http://172.16.0.9",
        "http://nas.local/api",
        "http://nas:8080",
    ] {
        assert!(validate_endpoint(good).is_ok(), "{good}");
    }
    for bad in [
        "http://example.com/v1",
        "http://8.8.8.8",
        "http://[2001:db8::1]",
        "https://user:pw@example.com",
        "https://user@example.com",
        "https://example.com/?key=abc",
        "https://example.com/#frag",
        "ftp://example.com",
        "file:///etc/passwd",
        "not a url",
        "",
        "https://",
    ] {
        let err = validate_endpoint(bad).expect_err(bad);
        assert_eq!(err, E::BadRequest("invalid endpoint".into()), "{bad}");
        assert!(!format!("{err:?}{err}").contains("example.com"));
    }
}
