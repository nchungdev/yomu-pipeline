#![cfg(feature = "llm")]

use std::collections::HashSet;
use std::sync::Arc;

use yomu_pipeline::contract::v1::{
    CancelToken, Credentials, EngineError, EventSink, ImageSource, JobHandle, Limits, PageInput,
    PagePx, PageResult, PipelineEvent, RegionKind, RegionResult, RegionStatus, RenderMode,
    TranslateBatchRequest, TranslateOptions, TranslationEngine,
};
use yomu_pipeline::server::{ApiStatus, RelayoutApiRequest, ServerConfig, ServerHandler};
use yomu_pipeline::NormalizedRect;

struct MockEngine;

impl TranslationEngine for MockEngine {
    fn submit(
        &self,
        request: TranslateBatchRequest,
        sink: Arc<dyn EventSink>,
        _cancel: CancelToken,
    ) -> Result<JobHandle, EngineError> {
        let req_id = request.request_id.clone();
        for page in request.pages {
            sink.emit(PipelineEvent::PageDone(PageResult {
                schema_version: 1,
                page_id: page.page_id,
                source_hash: "hash".into(),
                cache_key: "cache".into(),
                page_px: PagePx {
                    width: 100,
                    height: 100,
                },
                regions: vec![],
                composited: None,
            }));
        }
        sink.emit(PipelineEvent::BatchFinished {
            request_id: req_id,
            ok_pages: 1,
            failed_pages: 0,
        });
        Ok(JobHandle {
            request_id: "job-1".into(),
        })
    }

    fn relayout_region(
        &self,
        _page: &PageInput,
        region_id: &str,
        new_text: &str,
    ) -> Result<RegionResult, EngineError> {
        Ok(RegionResult {
            region_id: region_id.to_string(),
            kind: RegionKind::Balloon,
            reading_order: 0,
            source_text: "Old".into(),
            translated_text: new_text.to_string(),
            confidence: 1.0,
            bounds: NormalizedRect {
                x: 0.1,
                y: 0.1,
                width: 0.5,
                height: 0.5,
            },
            polygon: vec![],
            typography: Default::default(),
            layout: None,
            erase: yomu_pipeline::contract::v1::EraseSpec::None,
            status: RegionStatus::Rendered,
            speaker: None,
            gender: None,
            age: None,
            emotion: None,
            intensity: None,
        })
    }

    fn cache_key(&self, source_hash: &str, options: &TranslateOptions) -> String {
        yomu_pipeline::cache_key(source_hash, options)
    }
}

fn sample_batch_request(source: ImageSource) -> TranslateBatchRequest {
    TranslateBatchRequest {
        schema_version: 1,
        request_id: "req-123".into(),
        pages: vec![PageInput {
            page_id: "p1".into(),
            image: source,
            known_source_hash: None,
            prior: None,
            context: vec![],
        }],
        options: TranslateOptions {
            provider: "gemini".into(),
            chain: vec![],
            credentials: Credentials { api_key: None },
            source_lang: "en".into(),
            target_lang: "vi".into(),
            glossary: vec![],
            reading_direction: Default::default(),
            render_mode: RenderMode::Overlay,
            font_id: None,
            limits: Limits::default(),
        },
    }
}

#[test]
fn test_server_authentication() {
    let mut tokens = HashSet::new();
    tokens.insert("secret-token-xyz".to_string());
    let config = ServerConfig {
        auth_tokens: tokens,
        max_payload_bytes: 1024 * 1024,
    };
    let handler = ServerHandler::new(config, Arc::new(MockEngine));

    assert!(!handler.authenticate(None));
    assert!(!handler.authenticate(Some("Bearer wrong-token")));
    assert!(handler.authenticate(Some("Bearer secret-token-xyz")));
    assert!(handler.authenticate(Some("secret-token-xyz")));
}

#[test]
fn test_server_rejects_payload_too_large() {
    let config = ServerConfig {
        auth_tokens: HashSet::new(),
        max_payload_bytes: 10,
    };
    let handler = ServerHandler::new(config, Arc::new(MockEngine));
    let payload = b"this payload is definitely longer than 10 bytes";

    let res = handler.handle_translate(None, payload);
    assert_eq!(res.unwrap_err().0, ApiStatus::PayloadTooLarge);
}

#[test]
fn test_server_rejects_local_paths_in_remote_mode() {
    let config = ServerConfig::default();
    let handler = ServerHandler::new(config, Arc::new(MockEngine));
    let req = sample_batch_request(ImageSource::Path("/etc/passwd".into()));
    let payload = serde_json::to_vec(&req).unwrap();

    let res = handler.handle_translate(None, &payload);
    assert_eq!(res.unwrap_err().0, ApiStatus::BadRequest);
}

#[test]
fn test_server_handles_valid_batch_with_zero_retention() {
    let config = ServerConfig::default();
    let handler = ServerHandler::new(config, Arc::new(MockEngine));
    let req = sample_batch_request(ImageSource::Bytes(vec![1, 2, 3, 4]));
    let payload = serde_json::to_vec(&req).unwrap();

    let events = handler
        .handle_translate(None, &payload)
        .expect("should succeed");
    assert_eq!(events.len(), 2);
    assert!(matches!(&events[0], PipelineEvent::PageDone(_)));
    assert!(matches!(&events[1], PipelineEvent::BatchFinished { .. }));
}

#[test]
fn test_server_handles_relayout_request() {
    let config = ServerConfig::default();
    let handler = ServerHandler::new(config, Arc::new(MockEngine));
    let relayout_req = RelayoutApiRequest {
        page: PageInput {
            page_id: "p1".into(),
            image: ImageSource::Bytes(vec![1, 2, 3]),
            known_source_hash: None,
            prior: None,
            context: vec![],
        },
        region_id: "r1".into(),
        new_text: "Updated text".into(),
    };
    let payload = serde_json::to_vec(&relayout_req).unwrap();

    let res = handler
        .handle_relayout(None, &payload)
        .expect("should succeed");
    assert_eq!(res.region_id, "r1");
    assert_eq!(res.translated_text, "Updated text");
}
