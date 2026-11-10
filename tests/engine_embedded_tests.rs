use std::io::Cursor;

use image::{ImageBuffer, Rgb};
use yomu_pipeline::contract::v1::{
    CancelToken, EngineError, GlossaryCategory, GlossaryEntry, ImageSource, Limits, PageInput,
    PriorPage, RegionKind, RegionResult, RegionStatus, RenderMode, TranslateBatchRequest,
    TranslateOptions, TranslationEngine, SCHEMA_VERSION,
};
use yomu_pipeline::engine::{cache_key, event_channel, glossary_hash, EmbeddedEngine};
use yomu_pipeline::types::NormalizedRect;

fn create_test_jpeg(width: u32, height: u32) -> Vec<u8> {
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(width, height, Rgb([255, 255, 255]));
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Jpeg)
        .expect("encode jpeg");
    buf.into_inner()
}

fn create_test_jpeg_with_ink(width: u32, height: u32) -> Vec<u8> {
    let mut img: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(width, height, Rgb([255, 255, 255]));
    // Draw some black ink pixels inside the bubble area (x: 50..120, y: 50..100)
    for y in 50..100 {
        for x in 50..120 {
            img.put_pixel(x, y, Rgb([0, 0, 0]));
        }
    }
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Jpeg)
        .expect("encode jpeg");
    buf.into_inner()
}

#[test]
fn test_glossary_hash_determinism_and_order_independence() {
    let entries_a = vec![
        GlossaryEntry {
            source: "Naruto".into(),
            target: "Naruto".into(),
            category: Some(GlossaryCategory::Character),
            notes: Some("Protagonist".into()),
        },
        GlossaryEntry {
            source: "Sasuke".into(),
            target: "Sasuke".into(),
            category: Some(GlossaryCategory::Character),
            notes: None,
        },
    ];

    let entries_b = vec![entries_a[1].clone(), entries_a[0].clone()];

    let hash_a = glossary_hash(&entries_a);
    let hash_b = glossary_hash(&entries_b);
    assert_eq!(
        hash_a, hash_b,
        "glossary_hash must sort entries for stability"
    );

    let empty_hash = glossary_hash(&[]);
    assert_ne!(hash_a, empty_hash);
}

#[test]
fn test_cache_key_sensitivity() {
    let mut options = TranslateOptions {
        provider: "gemini".into(),
        chain: vec![],
        credentials: Default::default(),
        source_lang: "ja".into(),
        target_lang: "vi".into(),
        glossary: vec![],
        reading_direction: Default::default(),
        render_mode: RenderMode::Overlay,
        font_id: None,
        limits: Limits::default(),
    };

    let key1 = cache_key("hash_1", &options);
    let key2 = cache_key("hash_2", &options);
    assert_ne!(
        key1, key2,
        "different source hash should produce different cache keys"
    );

    options.target_lang = "en".into();
    let key3 = cache_key("hash_1", &options);
    assert_ne!(key1, key3, "different target lang should change cache key");

    options.glossary.push(GlossaryEntry {
        source: "Chakra".into(),
        target: "Chakra".into(),
        category: Some(GlossaryCategory::General),
        notes: None,
    });
    let key4 = cache_key("hash_1", &options);
    assert_ne!(key3, key4, "adding glossary entry should change cache key");
}

#[test]
fn test_submit_validation_failures() {
    let engine = EmbeddedEngine::default();
    let (sink, _rx) = event_channel();

    // 1. Invalid schema version
    let mut req = TranslateBatchRequest::new(
        "req_1".into(),
        vec![],
        TranslateOptions {
            provider: "gemini".into(),
            chain: vec![],
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
    req.schema_version = 999;
    let err = engine.submit(req.clone(), sink.clone(), CancelToken::default());
    assert!(matches!(err, Err(EngineError::InvalidRequest(_))));

    // 2. Max pages exceeded
    req.schema_version = SCHEMA_VERSION;
    req.options.limits.max_pages = 1;
    let jpeg = create_test_jpeg(100, 100);
    req.pages = vec![
        PageInput {
            page_id: "p1".into(),
            image: ImageSource::Bytes(jpeg.clone()),
            known_source_hash: None,
            prior: None,
            context: vec![],
        },
        PageInput {
            page_id: "p2".into(),
            image: ImageSource::Bytes(jpeg),
            known_source_hash: None,
            prior: None,
            context: vec![],
        },
    ];
    let err = engine.submit(req, sink.clone(), CancelToken::default());
    assert!(matches!(err, Err(EngineError::InvalidRequest(_))));

    // 3. Already cancelled
    let cancel = CancelToken::default();
    cancel.cancel();
    let mut valid_req = TranslateBatchRequest::new(
        "req_2".into(),
        vec![],
        TranslateOptions {
            provider: "gemini".into(),
            chain: vec![],
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
    valid_req.options.limits.max_pages = 10;
    let err = engine.submit(valid_req, sink, cancel);
    assert!(matches!(err, Err(EngineError::Cancelled)));
}

#[test]
fn test_relayout_region_updates_text() {
    let engine = EmbeddedEngine::default();
    let jpeg = create_test_jpeg_with_ink(400, 400);

    let prior_region = RegionResult {
        region_id: "b1".into(),
        kind: RegionKind::Balloon,
        reading_order: 0,
        source_text: "こんにちは".into(),
        translated_text: "Xin chào".into(),
        confidence: 1.0,
        bounds: NormalizedRect::new(0.1, 0.1, 0.4, 0.3).unwrap(),
        polygon: vec![],
        typography: Default::default(),
        layout: None,
        erase: yomu_pipeline::contract::v1::EraseSpec::None,
        status: RegionStatus::Rendered,
        speaker: Some("Naruto".into()),
        gender: None,
        age: None,
        emotion: Some("happy".into()),
        intensity: Some(0.8),
    };

    let page = PageInput {
        page_id: "p1".into(),
        image: ImageSource::Bytes(jpeg),
        known_source_hash: None,
        prior: Some(PriorPage {
            cache_key: "k1".into(),
            regions: vec![prior_region],
        }),
        context: vec![],
    };

    let updated = engine
        .relayout_region(&page, "b1", "Chào buổi sáng bạn nhé!")
        .expect("relayout region should succeed");

    assert_eq!(updated.region_id, "b1");
    assert_eq!(updated.source_text, "こんにちは");
    assert_eq!(updated.translated_text, "Chào buổi sáng bạn nhé!");
    assert_eq!(updated.speaker, Some("Naruto".into()));
    assert_eq!(updated.emotion, Some("happy".into()));
    assert!(updated.status.is_rendered());
    assert!(updated.layout.is_some());
    assert!(matches!(
        updated.erase,
        yomu_pipeline::contract::v1::EraseSpec::Patch { .. }
    ));
}
