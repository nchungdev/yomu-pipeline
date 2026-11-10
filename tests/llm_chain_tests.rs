#![cfg(feature = "test-util")]

#[path = "common/llm_provider.rs"]
mod support;

use serde_json::json;
use std::time::SystemTime;
use support::*;
use yomu_pipeline::contract::v1::{
    CancelToken, ProviderError as E, ProviderKind as K, ProviderSpec,
};
use yomu_pipeline::llm::chain::{matches_languages, merge_provider_errors, translate_tile_chain};
use yomu_pipeline::llm::transport_mock::{MockTransport, RecordingSleeper};
use yomu_pipeline::llm::{CallEnv, CallError};

fn gemini_spec(endpoint: Option<&str>, key: &str) -> ProviderSpec {
    spec(K::Gemini, endpoint, "gemini-2.5-flash", Some(key))
}

fn openai_spec(endpoint: &str, key: &str) -> ProviderSpec {
    spec(K::OpenAiCompatible, Some(endpoint), "gpt-test", Some(key))
}

#[allow(dead_code)]
fn gemini_ok(text: &str) -> String {
    json!({"candidates":[{"content":{"parts":[{"text":text}]},"finishReason":"STOP"}]}).to_string()
}

fn openai_ok(text: &str) -> String {
    json!({"choices":[{"message":{"role":"assistant","content":text},"finish_reason":"stop"}]})
        .to_string()
}

#[tokio::test]
async fn provider_order_and_language_filter() {
    let mut spec1 = openai_spec("https://api.openai.com/v1", "KEY-1");
    spec1.languages = vec!["en".to_string()]; // Request is ja -> vi, so this will be skipped!

    let mut spec2 = openai_spec("https://api.openai.com/v1", "KEY-2");
    spec2.languages = vec!["vi".to_string()]; // Matches target language

    let spec3 = gemini_spec(None, "KEY-3"); // Empty languages matches any

    let chain = vec![spec1, spec2, spec3];

    let transport = MockTransport::new(vec![ok(&openai_ok(REGIONS_JSON))]);
    let sleeper = RecordingSleeper::new();
    let pol = policy();
    let cancel = CancelToken::default();
    let env = CallEnv {
        transport: &transport,
        sleeper: &sleeper,
        policy: &pol,
        cancel: &cancel,
        now: &SystemTime::now,
    };

    let res = translate_tile_chain(&env, &chain, &tile()).await;
    assert!(res.is_ok(), "expected success from spec2");
    let reqs = transport.requests();
    assert_eq!(reqs.len(), 1);
    // Request must use KEY-2 from spec2 (spec1 was skipped due to language filter)
    let auth = header(&reqs[0], "authorization").expect("auth header");
    assert_eq!(auth.value, "Bearer KEY-2");
}

#[tokio::test]
async fn gemini_429_then_openai_success() {
    let spec_gemini = gemini_spec(None, "KEY-GEMINI");
    let spec_openai = openai_spec("https://api.openai.com/v1", "KEY-OPENAI");
    let chain = vec![spec_gemini, spec_openai];

    // Gemini returns 429 Rate limit, then OpenAI returns 200 OK
    let gemini_429_body = json!({
        "error": {
            "code": 429,
            "message": "Resource has been exhausted (e.g. check quota).",
            "status": "RESOURCE_EXHAUSTED",
            "details": [{"@type": "type.googleapis.com/google.rpc.QuotaFailure"}]
        }
    })
    .to_string();

    let script = vec![
        resp(
            429,
            &[("retry-after", "12"), ("content-type", "application/json")],
            &gemini_429_body,
        ),
        ok(&openai_ok(REGIONS_JSON)),
    ];

    let transport = MockTransport::new(script);
    let sleeper = RecordingSleeper::new();
    let pol = policy();
    let cancel = CancelToken::default();
    let env = CallEnv {
        transport: &transport,
        sleeper: &sleeper,
        policy: &pol,
        cancel: &cancel,
        now: &SystemTime::now,
    };

    let res = translate_tile_chain(&env, &chain, &tile()).await;
    assert!(res.is_ok());
    let regions = res.expect("regions");
    assert_eq!(regions.len(), 2);
    assert_eq!(regions[0].translated, "Xin chào");

    let reqs = transport.requests();
    assert_eq!(reqs.len(), 2);
    assert!(reqs[0].url.contains("googleapis.com"));
    assert_eq!(reqs[1].url, "https://api.openai.com/v1/chat/completions");
}

#[tokio::test]
async fn fall_through_per_error_kind() {
    // Test fall-through on various error kinds
    let error_responses = vec![
        // RateLimited
        resp(429, &[("retry-after", "5")], "rate limited"),
        // QuotaExhausted
        resp(
            429,
            &[("content-type", "application/json")],
            &json!({"error":{"code":429,"message":"Per-day limit exceeded","status":"RESOURCE_EXHAUSTED"}}).to_string(),
        ),
        // Auth
        resp(401, &[], "Unauthorized"),
        // Server
        resp(500, &[], "Internal Server Error"),
        // ContentBlocked
        ok(&json!({"candidates":[{"finishReason":"SAFETY"}]}).to_string()),
        // BadRequest
        resp(400, &[], "Bad Request"),
        // InvalidResponse
        ok("invalid unparseable json response"),
    ];

    for err_resp in error_responses {
        let spec1 = gemini_spec(None, "KEY-1");
        let spec2 = openai_spec("https://api.openai.com/v1", "KEY-2");
        let chain = vec![spec1, spec2];

        let script = vec![err_resp, ok(&openai_ok(REGIONS_JSON))];
        let transport = MockTransport::new(script);
        let sleeper = RecordingSleeper::new();
        let pol = yomu_pipeline::llm::RetryPolicy {
            max_attempts: 1,
            max_rate_limit_wait: std::time::Duration::ZERO,
            ..policy()
        };
        let cancel = CancelToken::default();
        let env = CallEnv {
            transport: &transport,
            sleeper: &sleeper,
            policy: &pol,
            cancel: &cancel,
            now: &SystemTime::now,
        };

        let res = translate_tile_chain(&env, &chain, &tile()).await;
        assert!(
            res.is_ok(),
            "expected fall-through to provider 2 and success"
        );
        let reqs = transport.requests();
        assert_eq!(reqs.len(), 2);
    }
}

#[tokio::test]
async fn cancel_mid_chain() {
    let spec1 = gemini_spec(None, "KEY-1");
    let spec2 = openai_spec("https://api.openai.com/v1", "KEY-2");
    let chain = vec![spec1, spec2];

    let cancel = CancelToken::default();
    let cancel_clone = cancel.clone();

    // After first call fails, trigger cancel
    let script = vec![resp(500, &[], "Server error")];
    let transport = MockTransport::new(script);
    let sleeper = RecordingSleeper::new();
    let pol = policy();

    // Cancel before or during
    cancel_clone.cancel();

    let env = CallEnv {
        transport: &transport,
        sleeper: &sleeper,
        policy: &pol,
        cancel: &cancel,
        now: &SystemTime::now,
    };

    let res = translate_tile_chain(&env, &chain, &tile()).await;
    assert_eq!(res, Err(CallError::Cancelled));
}

#[tokio::test]
async fn all_providers_fail_priority_merge() {
    let spec1 = gemini_spec(None, "KEY-1");
    let spec2 = openai_spec("https://api.openai.com/v1", "KEY-2");
    let chain = vec![spec1, spec2];

    // Spec1 fails with Server 500, Spec2 fails with Auth 401 -> Auth priority wins over Server
    let script = vec![
        resp(500, &[], "server error"),
        resp(401, &[], "unauthorized"),
    ];
    let transport = MockTransport::new(script);
    let sleeper = RecordingSleeper::new();
    let pol = policy();
    let cancel = CancelToken::default();
    let env = CallEnv {
        transport: &transport,
        sleeper: &sleeper,
        policy: &pol,
        cancel: &cancel,
        now: &SystemTime::now,
    };

    let res = translate_tile_chain(&env, &chain, &tile()).await;
    match res {
        Err(CallError::Provider(E::Auth { status: 401 })) => {}
        other => panic!("expected Auth 401 priority error, got {other:?}"),
    }
}

#[test]
fn priority_merge_rule_table() {
    // RateLimited picks SMALLEST hint
    let e1 = E::RateLimited {
        retry_after_ms: Some(30_000),
    };
    let e2 = E::RateLimited {
        retry_after_ms: Some(10_000),
    };
    assert_eq!(
        merge_provider_errors(&[e1, e2]),
        E::RateLimited {
            retry_after_ms: Some(10_000)
        }
    );

    // RateLimited wins over QuotaExhausted and Auth
    let rl = E::RateLimited {
        retry_after_ms: Some(5_000),
    };
    let quota = E::QuotaExhausted {
        reset_hint_ms: Some(86_400_000),
    };
    let auth = E::Auth { status: 401 };
    assert_eq!(
        merge_provider_errors(&[quota.clone(), rl.clone(), auth.clone()]),
        E::RateLimited {
            retry_after_ms: Some(5_000)
        }
    );

    // QuotaExhausted picks smallest hint and wins over Auth
    let q1 = E::QuotaExhausted {
        reset_hint_ms: Some(60_000),
    };
    let q2 = E::QuotaExhausted {
        reset_hint_ms: Some(15_000),
    };
    assert_eq!(
        merge_provider_errors(&[auth.clone(), q1, q2]),
        E::QuotaExhausted {
            reset_hint_ms: Some(15_000)
        }
    );

    // Auth wins over Server / Network / Timeout
    let srv = E::Server(500);
    let net = E::Network;
    assert_eq!(
        merge_provider_errors(&[srv, net.clone(), auth]),
        E::Auth { status: 401 }
    );

    // Last error when no priority error exists
    assert_eq!(merge_provider_errors(&[E::Server(502), net.clone()]), net);
}

#[test]
fn language_filter_matching() {
    let mut spec = gemini_spec(None, "KEY");
    spec.languages = vec![];
    assert!(matches_languages(&spec, "ja", "vi"));

    spec.languages = vec!["vi".into()];
    assert!(matches_languages(&spec, "ja", "vi"));
    assert!(!matches_languages(&spec, "ja", "en"));

    spec.languages = vec!["ja->en".into()];
    assert!(matches_languages(&spec, "ja", "en"));
    assert!(!matches_languages(&spec, "ja", "vi"));
}

#[tokio::test]
async fn api_key_never_appears_in_error_or_recorded_url() {
    let secret = "SECRET-SUPER-KEY-999";
    let spec1 = openai_spec("https://api.openai.com/v1", secret);
    let chain = vec![spec1];

    // Provider returns 400 echoing the key in message
    let echo_err = format!("Error: key {secret} was malformed");
    let script = vec![resp(400, &[], &echo_err)];

    let transport = MockTransport::new(script);
    let sleeper = RecordingSleeper::new();
    let pol = policy();
    let cancel = CancelToken::default();
    let env = CallEnv {
        transport: &transport,
        sleeper: &sleeper,
        policy: &pol,
        cancel: &cancel,
        now: &SystemTime::now,
    };

    let res = translate_tile_chain(&env, &chain, &tile()).await;
    match res {
        Err(CallError::Provider(err)) => {
            let debug_str = format!("{err:?}");
            let display_str = format!("{err}");
            assert!(
                !debug_str.contains(secret),
                "secret found in Debug: {debug_str}"
            );
            assert!(
                !display_str.contains(secret),
                "secret found in Display: {display_str}"
            );
        }
        other => panic!("expected provider error, got {other:?}"),
    }

    for req in transport.requests() {
        assert!(
            !req.url.contains(secret),
            "secret found in URL: {}",
            req.url
        );
    }
}
