#![cfg(feature = "test-util")]

#[path = "common/llm_provider.rs"]
mod support;

use serde_json::json;
use std::time::SystemTime;
use support::*;
use yomu_pipeline::contract::v1::{CancelToken, ProviderError as E, ProviderKind as K};
use yomu_pipeline::llm::chain::LlmPageRequest;
use yomu_pipeline::llm::image_prep::PrepOptions;
use yomu_pipeline::llm::transport_mock::{MockTransport, RecordingSleeper};
use yomu_pipeline::llm::{translate_page, CallEnv, CallError};

fn create_jpeg(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbImage::new(width, height);
    let mut bytes = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut bytes);
    encoder.encode_image(&img).expect("encode jpeg");
    bytes
}

fn openai_ok_regions(regions_json: &str) -> String {
    json!({"choices":[{"message":{"role":"assistant","content":regions_json},"finish_reason":"stop"}]})
        .to_string()
}

#[tokio::test]
async fn normal_page_1_tile_end_to_end() {
    let image_bytes = create_jpeg(800, 1000);
    let expected_hash = blake3::hash(&image_bytes).to_hex().to_string();

    let provider = spec(
        K::OpenAiCompatible,
        Some("https://api.openai.com/v1"),
        "gpt-4o",
        Some(KEY),
    );

    // Two regions in model output: b1 at top-right, b2 at bottom-left
    let regions_payload = r#"{"regions":[
      {"id":"b1","box_2d":[100,600,250,900],"text":"おはよう","translated_text":"Chào buổi sáng","k":"dialogue"},
      {"id":"b2","box_2d":[600,100,750,400],"text":"さようなら","translated_text":"Tạm biệt","k":"dialogue"}
    ]}"#;

    let transport = MockTransport::new(vec![ok(&openai_ok_regions(regions_payload))]);
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

    let req = LlmPageRequest {
        image: &image_bytes,
        chain: vec![provider],
        source_lang: "ja",
        target_lang: "vi",
        glossary: &[],
        context: &[],
        known_characters: &[],
        reading_direction: yomu_pipeline::contract::v1::ReadingDirection::Rtl,
        prep: PrepOptions::default(),
    };

    let res = translate_page(&env, &req).await.expect("page translation");
    assert_eq!(res.source_hash, expected_hash);
    assert_eq!(res.regions.len(), 2);

    // Check bounds normalization
    for r in &res.regions {
        assert!(r.bounds.x >= 0.0 && r.bounds.x <= 1.0);
        assert!(r.bounds.y >= 0.0 && r.bounds.y <= 1.0);
        assert!(r.bounds.width > 0.0 && r.bounds.width <= 1.0);
        assert!(r.bounds.height > 0.0 && r.bounds.height <= 1.0);
    }

    // Check reading order (RTL: top row first, right-to-left)
    assert_eq!(res.regions[0].id, "b1");
    assert_eq!(res.regions[0].translated, "Chào buổi sáng");
    assert_eq!(res.regions[1].id, "b2");
    assert_eq!(res.regions[1].translated, "Tạm biệt");
}

#[tokio::test]
async fn tall_synthetic_page_overlap_dedup_and_remap() {
    // 800x12000 tall webtoon page with multiple tiles
    let image_bytes = create_jpeg(800, 12000);
    let expected_hash = blake3::hash(&image_bytes).to_hex().to_string();

    let provider = spec(
        K::OpenAiCompatible,
        Some("https://api.openai.com/v1"),
        "gpt-4o",
        Some(KEY),
    );

    // In Tile 0: region A (middle of tile 0) and region B (near bottom of tile 0, in overlap ~1843..2007 px)
    // Box 2d: [ymin, xmin, ymax, xmax] in 0-1000 scale
    let tile0_json = r#"{"regions":[
      {"id":"t0_a","box_2d":[200,200,300,500],"text":"Region A","translated_text":"Vùng A","k":"dialogue"},
      {"id":"t0_b","box_2d":[900,200,980,600],"text":"Overlap Text","translated_text":"Chữ trùng","k":"dialogue"}
    ]}"#;

    // In Tile 1: region B' (near top of tile 1, matching ~1843..2007 px in the 1741..3789 px tile span)
    // and region C (middle of tile 1)
    let tile1_json = r#"{"regions":[
      {"id":"t1_b","box_2d":[50,200,130,600],"text":"Overlap Text","translated_text":"Chữ trùng","k":"dialogue"},
      {"id":"t1_c","box_2d":[500,200,600,500],"text":"Region C","translated_text":"Vùng C","k":"dialogue"}
    ]}"#;

    let empty_json = r#"{"regions":[]}"#;

    // With 800x12000 and default tile height 2048 and overlap 0.15, step is ~1740 px.
    // 12000 / 1740 ~= 7 tiles.
    let mut script = vec![
        ok(&openai_ok_regions(tile0_json)),
        ok(&openai_ok_regions(tile1_json)),
    ];
    for _ in 0..10 {
        script.push(ok(&openai_ok_regions(empty_json)));
    }

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

    let req = LlmPageRequest {
        image: &image_bytes,
        chain: vec![provider],
        source_lang: "ja",
        target_lang: "vi",
        glossary: &[],
        context: &[],
        known_characters: &[],
        reading_direction: yomu_pipeline::contract::v1::ReadingDirection::Ltr,
        prep: PrepOptions::default(),
    };

    let res = translate_page(&env, &req)
        .await
        .expect("translate tall page");
    assert_eq!(res.source_hash, expected_hash);

    // The overlapping region should be deduplicated: exactly 3 regions survive (A, B, C)
    let texts: Vec<&str> = res.regions.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(texts.len(), 3, "regions: {texts:?}");
    assert!(texts.contains(&"Region A"));
    assert!(texts.contains(&"Overlap Text"));
    assert!(texts.contains(&"Region C"));

    // Verify all regions are mapped to global [0..1] range
    for r in &res.regions {
        assert!(r.bounds.y >= 0.0 && r.bounds.y + r.bounds.height <= 1.0);
    }
}

#[tokio::test]
async fn atomic_page_failure_policy_on_tile_error() {
    let image_bytes = create_jpeg(800, 12000);
    let provider = spec(
        K::OpenAiCompatible,
        Some("https://api.openai.com/v1"),
        "gpt-4o",
        Some(KEY),
    );

    // Tile 0 succeeds, but Tile 1 fails with 429
    let script = vec![
        ok(&openai_ok_regions(r#"{"regions":[]}"#)),
        resp(429, &[("retry-after", "15")], "Rate limit hit"),
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

    let req = LlmPageRequest {
        image: &image_bytes,
        chain: vec![provider],
        source_lang: "ja",
        target_lang: "vi",
        glossary: &[],
        context: &[],
        known_characters: &[],
        reading_direction: yomu_pipeline::contract::v1::ReadingDirection::Ltr,
        prep: PrepOptions::default(),
    };

    let res = translate_page(&env, &req).await;
    match res {
        Err(CallError::Provider(E::RateLimited { retry_after_ms })) => {
            assert_eq!(retry_after_ms, Some(15_000));
        }
        other => panic!("expected atomic RateLimited failure, got {other:?}"),
    }
}
