#![cfg(feature = "test-util")]

#[path = "common/llm_provider.rs"]
mod support;

use base64::Engine;
use support::*;
use yomu_pipeline::contract::v1::{ProviderError as E, ProviderKind as K};
use yomu_pipeline::llm::{CallError, TransportError};

const URL: &str = "http://192.168.1.50:9000/translate";

fn custom(key: Option<&str>) -> yomu_pipeline::contract::v1::ProviderSpec {
    spec(K::CustomHttp, Some(URL), "", key)
}

const APP_JSON: &str = r#"{"regions":[{"id":"r1","x":0.1,"y":0.1,"width":0.3,"height":0.2,"text":"Hello","translated_text":"Xin chào"}]}"#;

fn provider_err(run: Run) -> E {
    match run.result {
        Err(CallError::Provider(e)) => e,
        other => panic!("expected provider error, got {other:?}"),
    }
}

#[tokio::test]
async fn custom_request_shape_and_app_response() {
    let r = run(vec![ok(APP_JSON)], &custom(Some(KEY))).await;
    let regions = r.result.expect("regions");
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].translated, "Xin chào");
    let req = &r.requests[0];
    assert_eq!(req.url, URL);
    let auth = header(req, "authorization").expect("auth");
    assert_eq!(auth.value, format!("Bearer {KEY}"));
    assert!(auth.sensitive);
    assert!(!format!("{req:?}").contains(KEY));
    let body = body_json(req);
    assert_eq!(body["source_language"], "ja");
    assert_eq!(body["target_language"], "vi");
    assert_eq!(body["page_id"], "page-7");
    let data = body["image_base64"].as_str().unwrap_or_default();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(data)
        .unwrap_or_default();
    assert_eq!(decoded, IMAGE);
}

#[tokio::test]
async fn custom_without_key_has_no_auth_header() {
    let r = run(vec![ok(APP_JSON)], &custom(None)).await;
    assert!(r.result.is_ok());
    assert!(header(&r.requests[0], "authorization").is_none());
}

#[tokio::test]
async fn custom_rejects_public_http_and_missing_endpoint() {
    let public = spec(K::CustomHttp, Some("http://example.com/t"), "", None);
    assert_eq!(
        provider_err(run(vec![], &public).await),
        E::BadRequest("invalid endpoint".into())
    );
    let none = spec(K::CustomHttp, None, "", None);
    assert_eq!(
        provider_err(run(vec![], &none).await),
        E::BadRequest("invalid endpoint".into())
    );
}

#[tokio::test]
async fn custom_invalid_bodies() {
    for body in [
        "",
        "   ",
        "<html>oops</html>",
        r#"{"regions":"x"}"#,
        r#"{"regions":[{"id":"a"}]}"#,
    ] {
        let r = run(vec![ok(body), ok(body)], &custom(None)).await;
        assert_eq!(r.requests.len(), 1, "no retry for {body:?}");
        assert!(matches!(provider_err(r), E::InvalidResponse(_)), "{body:?}");
    }
}

// Ported from the app's resilience tests (chain cases are step 2g).

#[tokio::test]
async fn retry_after_zero_on_429_then_success() {
    let script = vec![
        resp(429, &[("retry-after", "0")], "Rate limited"),
        ok(APP_JSON),
    ];
    let r = run(script, &custom(None)).await;
    assert_eq!(r.requests.len(), 2);
    assert_eq!(
        r.result.map(|v| v[0].translated.clone()),
        Ok("Xin chào".to_owned())
    );
}

#[tokio::test]
async fn rate_limit_with_long_hint_is_returned_unclamped() {
    let r = run(
        vec![resp(429, &[("retry-after", "3600")], "")],
        &custom(None),
    )
    .await;
    assert_eq!(r.requests.len(), 1);
    assert_eq!(
        provider_err(r),
        E::RateLimited {
            retry_after_ms: Some(3_600_000)
        }
    );
}

#[tokio::test]
async fn backoff_on_503_then_success() {
    let r = run(
        vec![resp(503, &[], "Service Unavailable"), ok(APP_JSON)],
        &custom(None),
    )
    .await;
    assert_eq!(r.requests.len(), 2);
    assert!(r.result.is_ok());
}

#[tokio::test]
async fn transport_failures_map_to_network_and_timeout() {
    let r = run(vec![Err(TransportError::Connect); 3], &custom(None)).await;
    assert_eq!(r.requests.len(), 3);
    assert_eq!(provider_err(r), E::Network);
    let r = run(vec![Err(TransportError::Timeout); 3], &custom(None)).await;
    assert_eq!(provider_err(r), E::Timeout);
}
