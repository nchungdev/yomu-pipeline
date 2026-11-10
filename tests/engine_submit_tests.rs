#![cfg(feature = "test-util")]

use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use image::{ImageBuffer, Rgb};
use serde_json::json;
use yomu_pipeline::contract::v1::{
    CancelToken, EngineError, ImageSource, Limits, PageInput, PipelineEvent, PriorPage,
    ProviderKind as K, ProviderSpec, RegionKind, RegionResult, RegionStatus, RenderMode,
    TranslateBatchRequest, TranslateOptions, TranslationEngine,
};
use yomu_pipeline::engine::{cache_key, event_channel, EmbeddedEngine};
use yomu_pipeline::llm::transport_mock::{MockTransport, RecordingSleeper};
use yomu_pipeline::pipeline::PipelineOptions;
use yomu_pipeline::render::FontRenderer;
use yomu_pipeline::types::NormalizedRect;

fn create_test_jpeg(width: u32, height: u32) -> Vec<u8> {
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(width, height, Rgb([255, 255, 255]));
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Jpeg)
        .expect("encode jpeg");
    buf.into_inner()
}

fn openai_ok_regions(regions_json: &str) -> String {
    json!({"choices":[{"message":{"role":"assistant","content":regions_json},"finish_reason":"stop"}]})
        .to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn test_submit_end_to_end_with_mock_llm() {
    let jpeg = create_test_jpeg(500, 500);

    let provider = ProviderSpec {
        kind: K::OpenAiCompatible,
        endpoint: Some("https://api.openai.com/v1".into()),
        model: "gpt-4o".into(),
        languages: vec![],
        credentials: Default::default(),
    };

    let regions_payload = r#"{"regions":[
        {"id":"b1","box_2d":[100,100,300,500],"text":"おはよう","translated_text":"Chào buổi sáng","k":"dialogue"}
    ]}"#;

    let transport = Arc::new(MockTransport::new(vec![Ok(
        yomu_pipeline::llm::transport::HttpResponse {
            status: 200,
            headers: vec![],
            body: openai_ok_regions(regions_payload).into_bytes(),
        },
    )]));
    let sleeper = Arc::new(RecordingSleeper::new());

    let engine = EmbeddedEngine::with_transport(
        FontRenderer::default(),
        PipelineOptions::default(),
        transport,
        sleeper,
    );

    let (sink, rx) = event_channel();
    let cancel = CancelToken::default();

    let req = TranslateBatchRequest::new(
        "batch_1".into(),
        vec![PageInput {
            page_id: "page_1".into(),
            image: ImageSource::Bytes(jpeg),
            known_source_hash: None,
            prior: None,
            context: vec![],
        }],
        TranslateOptions {
            provider: String::new(),
            chain: vec![provider],
            credentials: Default::default(),
            source_lang: "ja".into(),
            target_lang: "vi".into(),
            glossary: vec![],
            reading_direction: Default::default(),
            render_mode: RenderMode::Overlay,
            font_id: None,
            limits: Limits::default(),
        },
    );

    let handle = engine
        .submit(req, sink, cancel)
        .expect("submit should succeed");
    assert_eq!(handle.request_id, "batch_1");

    let mut events = Vec::new();
    while let Ok(event) = rx.recv_timeout(Duration::from_secs(3)) {
        let is_finished = matches!(event, PipelineEvent::BatchFinished { .. });
        events.push(event);
        if is_finished {
            break;
        }
    }

    assert!(matches!(
        events.first(),
        Some(PipelineEvent::BatchStarted { total_pages: 1, .. })
    ));
    assert!(matches!(
        events.last(),
        Some(PipelineEvent::BatchFinished {
            ok_pages: 1,
            failed_pages: 0,
            ..
        })
    ));

    let page_done = events
        .iter()
        .find_map(|e| match e {
            PipelineEvent::PageDone(res) => Some(res),
            _ => None,
        })
        .expect("should emit PageDone event");

    assert_eq!(page_done.page_id, "page_1");
    assert_eq!(page_done.regions.len(), 1);
    let reg = &page_done.regions[0];
    assert_eq!(reg.region_id, "b1");
    assert_eq!(reg.source_text, "おはよう");
    assert_eq!(reg.translated_text, "Chào buổi sáng");
    assert!(reg.status.is_rendered());
    assert!(page_done.validate().is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn test_submit_cancelled_before_llm() {
    let jpeg = create_test_jpeg(200, 200);

    let provider = ProviderSpec {
        kind: K::OpenAiCompatible,
        endpoint: Some("https://api.openai.com/v1".into()),
        model: "gpt-4o".into(),
        languages: vec![],
        credentials: Default::default(),
    };

    let transport = Arc::new(MockTransport::new(vec![]));
    let sleeper = Arc::new(RecordingSleeper::new());

    let engine = EmbeddedEngine::with_transport(
        FontRenderer::default(),
        PipelineOptions::default(),
        transport,
        sleeper,
    );

    let (sink, _rx) = event_channel();
    let cancel = CancelToken::default();

    let req = TranslateBatchRequest::new(
        "batch_cancel".into(),
        vec![PageInput {
            page_id: "p1".into(),
            image: ImageSource::Bytes(jpeg),
            known_source_hash: None,
            prior: None,
            context: vec![],
        }],
        TranslateOptions {
            provider: String::new(),
            chain: vec![provider],
            credentials: Default::default(),
            source_lang: "ja".into(),
            target_lang: "vi".into(),
            glossary: vec![],
            reading_direction: Default::default(),
            render_mode: RenderMode::Overlay,
            font_id: None,
            limits: Limits::default(),
        },
    );

    cancel.cancel();
    let res = engine.submit(req, sink, cancel);
    assert!(matches!(res, Err(EngineError::Cancelled)));
}

#[tokio::test(flavor = "multi_thread")]
async fn test_submit_prior_cache_hit_skips_llm() {
    let jpeg = create_test_jpeg(300, 300);
    let source_hash = blake3::hash(&jpeg).to_hex().to_string();

    let provider = ProviderSpec {
        kind: K::OpenAiCompatible,
        endpoint: Some("https://api.openai.com/v1".into()),
        model: "gpt-4o".into(),
        languages: vec![],
        credentials: Default::default(),
    };

    let options = TranslateOptions {
        provider: String::new(),
        chain: vec![provider],
        credentials: Default::default(),
        source_lang: "ja".into(),
        target_lang: "vi".into(),
        glossary: vec![],
        reading_direction: Default::default(),
        render_mode: RenderMode::Overlay,
        font_id: None,
        limits: Limits::default(),
    };

    let expected_key = cache_key(&source_hash, &options);

    let prior_region = RegionResult {
        region_id: "cached_b1".into(),
        kind: RegionKind::Balloon,
        reading_order: 0,
        source_text: "cached source".into(),
        translated_text: "cached trans".into(),
        confidence: 1.0,
        bounds: NormalizedRect::new(0.1, 0.1, 0.5, 0.5).unwrap(),
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
    };

    let transport = Arc::new(MockTransport::new(vec![]));
    let sleeper = Arc::new(RecordingSleeper::new());

    let engine = EmbeddedEngine::with_transport(
        FontRenderer::default(),
        PipelineOptions::default(),
        transport,
        sleeper,
    );

    let (sink, rx) = event_channel();
    let cancel = CancelToken::default();

    let req = TranslateBatchRequest::new(
        "batch_cache_hit".into(),
        vec![PageInput {
            page_id: "cached_page".into(),
            image: ImageSource::Bytes(jpeg),
            known_source_hash: Some(source_hash),
            prior: Some(PriorPage {
                cache_key: expected_key.clone(),
                regions: vec![prior_region],
            }),
            context: vec![],
        }],
        options,
    );

    let _ = engine
        .submit(req, sink, cancel)
        .expect("submit should succeed");

    let mut events = Vec::new();
    while let Ok(event) = rx.recv_timeout(Duration::from_secs(2)) {
        let is_finished = matches!(event, PipelineEvent::BatchFinished { .. });
        events.push(event);
        if is_finished {
            break;
        }
    }

    let done = events
        .iter()
        .find_map(|e| match e {
            PipelineEvent::PageDone(res) => Some(res),
            _ => None,
        })
        .expect("should emit PageDone from cache");

    assert_eq!(done.cache_key, expected_key);
    assert_eq!(done.regions.len(), 1);
    assert_eq!(done.regions[0].region_id, "cached_b1");
}
