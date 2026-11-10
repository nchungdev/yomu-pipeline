//! Shared helpers for the provider adapter tests (all traffic goes through `MockTransport`).
#![allow(dead_code)]

use std::time::SystemTime;
use yomu_pipeline::contract::v1::{
    CancelToken, Credentials, ProviderKind, ProviderSpec, ReadingDirection,
};
use yomu_pipeline::llm::transport_mock::{fixed_jitter, MockTransport, RecordingSleeper};
use yomu_pipeline::llm::{
    translate_tile, CallEnv, CallError, HttpRequest, HttpResponse, LlmRegion, RetryPolicy,
    TileInput, TransportError,
};

pub const KEY: &str = "KEY-SECRET-123";
pub const PROMPT: &str = "PROMPT-TEXT";
pub const IMAGE: &[u8] = b"\xff\xd8JPEGDATA\x00\x01";

pub fn resp(
    status: u16,
    headers: &[(&str, &str)],
    body: &str,
) -> Result<HttpResponse, TransportError> {
    Ok(HttpResponse {
        status,
        headers: headers
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect(),
        body: body.as_bytes().to_vec(),
    })
}

pub fn ok(body: &str) -> Result<HttpResponse, TransportError> {
    resp(200, &[], body)
}

pub fn spec(
    kind: ProviderKind,
    endpoint: Option<&str>,
    model: &str,
    key: Option<&str>,
) -> ProviderSpec {
    ProviderSpec {
        kind,
        endpoint: endpoint.map(str::to_owned),
        model: model.to_owned(),
        languages: Vec::new(),
        credentials: Credentials {
            api_key: key.map(str::to_owned),
        },
    }
}

pub fn tile() -> TileInput<'static> {
    TileInput {
        prompt: PROMPT,
        jpeg: IMAGE,
        mime: "image/jpeg",
        reading_direction: ReadingDirection::Rtl,
        page_id: "page-7",
        source_lang: "ja",
        target_lang: "vi",
    }
}

pub fn policy() -> RetryPolicy {
    RetryPolicy {
        jitter: fixed_jitter,
        ..RetryPolicy::default()
    }
}

pub struct Run {
    pub result: Result<Vec<LlmRegion>, CallError>,
    pub requests: Vec<HttpRequest>,
}

pub async fn run_with(
    script: Vec<Result<HttpResponse, TransportError>>,
    spec: &ProviderSpec,
    cancel: &CancelToken,
) -> Run {
    let transport = MockTransport::new(script);
    let sleeper = RecordingSleeper::new();
    let policy = policy();
    let env = CallEnv {
        transport: &transport,
        sleeper: &sleeper,
        policy: &policy,
        cancel,
        now: &SystemTime::now,
    };
    let result = translate_tile(&env, spec, &tile()).await;
    Run {
        result,
        requests: transport.requests(),
    }
}

pub async fn run(script: Vec<Result<HttpResponse, TransportError>>, spec: &ProviderSpec) -> Run {
    run_with(script, spec, &CancelToken::default()).await
}

pub fn header<'a>(req: &'a HttpRequest, name: &str) -> Option<&'a yomu_pipeline::llm::Header> {
    req.headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
}

pub fn body_json(req: &HttpRequest) -> serde_json::Value {
    serde_json::from_slice(&req.body).unwrap_or(serde_json::Value::Null)
}

/// Two valid regions in the model's `box_2d` form.
pub const REGIONS_JSON: &str = r#"{"regions":[
 {"id":"b1","box_2d":[100,600,300,900],"text":"こんにちは","translated_text":"Xin chào","k":"dialogue","spk":"Aki","g":"f","age":"teen","emo":"happy","int":0.8},
 {"id":"b2","box_2d":[500,100,700,400],"text":"はい","translated_text":"Vâng","k":"thought"}]}"#;
