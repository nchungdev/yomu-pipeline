#![cfg(feature = "test-util")]

use std::time::{Duration, SystemTime};
use yomu_pipeline::contract::v1::{CancelToken, ProviderError as E, ProviderKind as K};
use yomu_pipeline::llm::transport_mock::{fixed_jitter, MockTransport, RecordingSleeper};
use yomu_pipeline::llm::{
    call_with_retry, CallError, Header, HttpRequest, HttpResponse, Method, RetryPolicy,
    TransportError,
};

type Script = Vec<Result<HttpResponse, TransportError>>;

fn resp(status: u16, headers: &[(&str, &str)], body: &str) -> Result<HttpResponse, TransportError> {
    Ok(HttpResponse {
        status,
        headers: headers
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect(),
        body: body.as_bytes().to_vec(),
    })
}

fn request() -> HttpRequest {
    HttpRequest {
        method: Method::Post,
        url: "https://api.example.test/v1/chat?key=URLSECRET".into(),
        headers: vec![Header::secret("authorization", "Bearer SECRETKEY")],
        body: b"PRIVATE-BODY".to_vec(),
        timeout: Duration::from_secs(5),
    }
}

fn policy() -> RetryPolicy {
    RetryPolicy {
        jitter: fixed_jitter,
        ..RetryPolicy::default()
    }
}

struct Outcome {
    result: Result<HttpResponse, CallError>,
    calls: usize,
    sleeps: Vec<Duration>,
}

async fn run_with(script: Script, policy: RetryPolicy, kind: K) -> Outcome {
    let t = MockTransport::new(script);
    let s = RecordingSleeper::new();
    let result = call_with_retry(
        &t,
        &s,
        kind,
        &request(),
        &policy,
        &CancelToken::default(),
        &SystemTime::now,
    )
    .await;
    Outcome {
        result,
        calls: t.call_count(),
        sleeps: s.sleeps(),
    }
}

async fn run(script: Script) -> Outcome {
    run_with(script, policy(), K::OpenAiCompatible).await
}

fn provider(o: &Outcome) -> &E {
    match &o.result {
        Err(CallError::Provider(e)) => e,
        other => panic!("expected provider error, got {other:?}"),
    }
}

const OK_BODY: &str = r#"{"choices":[{"finish_reason":"stop"}]}"#;

#[tokio::test]
async fn persistent_503_makes_three_calls() {
    let o = run(vec![
        resp(503, &[], ""),
        resp(503, &[], ""),
        resp(503, &[], ""),
        resp(200, &[], OK_BODY),
    ])
    .await;
    assert_eq!(o.calls, 3);
    assert_eq!(provider(&o), &E::Server(503));
    assert_eq!(
        o.sleeps,
        vec![Duration::from_millis(900), Duration::from_millis(1700)]
    );
    assert!(o.sleeps.iter().sum::<Duration>() <= Duration::from_secs(30));
}

#[tokio::test]
async fn persistent_timeout_and_connect_make_three_calls() {
    let o = run(vec![Err(TransportError::Timeout); 5]).await;
    assert_eq!((o.calls, provider(&o)), (3, &E::Timeout));
    let o = run(vec![
        Err(TransportError::Connect),
        Err(TransportError::Other),
        Err(TransportError::Connect),
    ])
    .await;
    assert_eq!((o.calls, provider(&o)), (3, &E::Network));
    let o = run(vec![
        Err(TransportError::BodyTooLarge),
        resp(200, &[], OK_BODY),
    ])
    .await;
    assert_eq!(o.calls, 1);
    assert!(matches!(provider(&o), E::InvalidResponse(_)));
}

#[tokio::test]
async fn transient_then_success() {
    let o = run(vec![
        resp(500, &[], ""),
        Err(TransportError::Timeout),
        resp(200, &[], OK_BODY),
    ])
    .await;
    assert!(o.result.is_ok());
    assert_eq!(o.calls, 3);
}

#[tokio::test]
async fn non_retryable_errors_make_one_call() {
    let quota = r#"{"error":{"code":"insufficient_quota"}}"#;
    let cases = [
        resp(401, &[], ""),
        resp(402, &[], ""),
        resp(400, &[], "{}"),
        resp(404, &[], "{}"),
        resp(429, &[], quota),
        resp(
            200,
            &[],
            r#"{"choices":[{"finish_reason":"content_filter"}]}"#,
        ),
        resp(200, &[], "not json {"),
    ];
    for case in cases {
        let o = run(vec![case, resp(200, &[], OK_BODY)]).await;
        assert_eq!(o.calls, 1, "{:?}", o.result);
        assert!(o.sleeps.is_empty());
        assert!(o.result.is_err());
    }
}

#[tokio::test]
async fn long_rate_limit_hint_returned_unclamped() {
    let o = run(vec![
        resp(429, &[("retry-after", "60")], "{}"),
        resp(200, &[], OK_BODY),
    ])
    .await;
    assert_eq!(o.calls, 1);
    assert_eq!(
        provider(&o),
        &E::RateLimited {
            retry_after_ms: Some(60_000)
        }
    );
    assert!(o.sleeps.is_empty());
}

#[tokio::test]
async fn short_rate_limit_hint_waits_then_succeeds() {
    let o = run(vec![
        resp(429, &[("retry-after", "5")], "{}"),
        resp(200, &[], OK_BODY),
    ])
    .await;
    assert!(o.result.is_ok());
    assert_eq!(o.calls, 2);
    assert_eq!(o.sleeps, vec![Duration::from_secs(5)]);
}

#[tokio::test]
async fn rate_limit_without_hint_is_not_retried() {
    let o = run(vec![resp(429, &[], "{}"), resp(200, &[], OK_BODY)]).await;
    assert_eq!(o.calls, 1);
    assert_eq!(
        provider(&o),
        &E::RateLimited {
            retry_after_ms: None
        }
    );
}

#[tokio::test]
async fn hint_exceeding_remaining_budget_returns_immediately() {
    let p = RetryPolicy {
        total_budget: Duration::from_secs(4),
        ..policy()
    };
    let o = run_with(
        vec![
            resp(429, &[("retry-after", "5")], "{}"),
            resp(200, &[], OK_BODY),
        ],
        p,
        K::OpenAiCompatible,
    )
    .await;
    assert_eq!(o.calls, 1);
    assert_eq!(
        provider(&o),
        &E::RateLimited {
            retry_after_ms: Some(5000)
        }
    );
}

#[tokio::test]
async fn total_sleep_never_exceeds_budget() {
    let p = RetryPolicy {
        max_attempts: 10,
        total_budget: Duration::from_secs(4),
        ..policy()
    };
    let o = run_with(vec![resp(503, &[], ""); 10], p, K::Gemini).await;
    let total: Duration = o.sleeps.iter().sum();
    assert!(total <= Duration::from_secs(4), "{:?}", o.sleeps);
    assert!(o.calls < 10);
    assert_eq!(provider(&o), &E::Server(503));
}

#[tokio::test]
async fn cancel_before_first_attempt_makes_no_call() {
    let t = MockTransport::new(vec![resp(200, &[], OK_BODY)]);
    let cancel = CancelToken::default();
    cancel.cancel();
    let r = call_with_retry(
        &t,
        &RecordingSleeper::new(),
        K::Gemini,
        &request(),
        &policy(),
        &cancel,
        &SystemTime::now,
    )
    .await;
    assert_eq!(r.unwrap_err(), CallError::Cancelled);
    assert_eq!(t.call_count(), 0);
}

#[tokio::test]
async fn cancel_during_sleep_stops_further_calls() {
    let t = MockTransport::new(vec![resp(503, &[], ""), resp(200, &[], OK_BODY)]);
    let cancel = CancelToken::default();
    let s = RecordingSleeper::cancelling(cancel.clone());
    let r = call_with_retry(
        &t,
        &s,
        K::Gemini,
        &request(),
        &policy(),
        &cancel,
        &SystemTime::now,
    )
    .await;
    assert_eq!(r.unwrap_err(), CallError::Cancelled);
    assert_eq!(t.call_count(), 1);
}

#[test]
fn request_debug_hides_secrets_body_and_query() {
    let r = request();
    for s in [
        format!("{r:?}"),
        format!("{r}"),
        format!("{:#?}", r.headers),
    ] {
        for leak in ["SECRETKEY", "PRIVATE-BODY", "URLSECRET"] {
            assert!(!s.contains(leak), "{s}");
        }
    }
    assert!(format!("{r:?}").contains("<redacted>"));
}

#[tokio::test]
async fn error_strings_never_contain_url() {
    let o = run(vec![Err(TransportError::Connect); 3]).await;
    let mut all = format!("{:?} {}", o.result, provider(&o));
    for e in [
        TransportError::Timeout,
        TransportError::Connect,
        TransportError::BodyTooLarge,
        TransportError::Other,
    ] {
        all.push_str(&format!("{e:?} {e}"));
    }
    assert!(!all.contains("example.test") && !all.contains("URLSECRET"));
}
