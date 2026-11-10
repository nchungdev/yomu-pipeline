//! Golden JSON fixtures and invariants of `contract::v1`.

use serde::de::DeserializeOwned;
use serde::Serialize;
use yomu_pipeline::contract::v1::*;
use yomu_pipeline::types::{InferredTypography, NormalizedRect};

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/contract_v1/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

/// Parses the fixture, checks that re-serialization equals the golden JSON, and round-trips.
fn golden<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(name: &str) -> T {
    let text = fixture(name);
    let golden: serde_json::Value = serde_json::from_str(&text).expect("fixture is JSON");
    let parsed: T = serde_json::from_str(&text).expect("fixture matches the type");
    assert_eq!(serde_json::to_value(&parsed).expect("serialize"), golden);
    let again: T = serde_json::from_str(&serde_json::to_string(&parsed).expect("to string"))
        .expect("round trip");
    assert_eq!(again, parsed);
    parsed
}

fn region(status: RegionStatus, erase: EraseSpec, layout: Option<TextLayoutV1>) -> RegionResult {
    RegionResult {
        region_id: "r".into(),
        kind: RegionKind::Balloon,
        reading_order: 0,
        source_text: "a".into(),
        translated_text: "b".into(),
        confidence: 1.0,
        bounds: NormalizedRect::new(0.1, 0.1, 0.2, 0.2).expect("valid"),
        polygon: vec![],
        typography: InferredTypography::default(),
        layout,
        erase,
        status,
        speaker: None,
        gender: None,
        age: None,
        emotion: None,
        intensity: None,
    }
}

fn patch() -> EraseSpec {
    EraseSpec::Patch {
        rect_norm: NormalizedRect::new(0.1, 0.1, 0.2, 0.2).expect("valid"),
        rect_px: yomu_pipeline::types::PxRect::new(0.0, 0.0, 1.0, 1.0),
        format: PatchFormat::RawRgba,
        data: vec![0; 4],
    }
}

fn some_layout() -> Option<TextLayoutV1> {
    Some(TextLayoutV1 {
        font_px: 12.0,
        lines: vec![],
    })
}

#[test]
fn schema_version_is_one() {
    assert_eq!(SCHEMA_VERSION, 1);
}

#[test]
fn request_fixture_round_trips() {
    let req: TranslateBatchRequest = golden("request.json");
    assert_eq!(req.schema_version, SCHEMA_VERSION);
    assert_eq!(req.pages.len(), 2);
    assert_eq!(req.options.render_mode, RenderMode::Overlay);
}

#[test]
fn credentials_are_redacted_and_never_serialized() {
    let mut req: TranslateBatchRequest = golden("request.json");
    req.options.credentials = Credentials {
        api_key: Some("sk-secret-123".into()),
    };
    let debug = format!("{req:?}");
    assert!(!debug.contains("sk-secret-123"));
    assert!(debug.contains("redacted"));
    let json = serde_json::to_string(&req).expect("serialize");
    assert!(!json.contains("sk-secret-123"));
    assert!(!json.contains("credentials"));
}

#[test]
fn request_defaults_apply_when_optional_fields_missing() {
    let json = r#"{"schema_version":1,"request_id":"r","pages":[],
        "options":{"provider":"p","source_lang":"ja","target_lang":"vi"}}"#;
    let req: TranslateBatchRequest = serde_json::from_str(json).expect("parse");
    assert_eq!(req.options.render_mode, RenderMode::Overlay);
    assert_eq!(req.options.reading_direction, ReadingDirection::Rtl);
    assert_eq!(req.options.limits, Limits::default());
}

#[test]
fn page_result_fixture_round_trips_and_validates() {
    let page: PageResult = golden("page_result.json");
    page.validate().expect("fixture satisfies invariants");
    assert_eq!(page.regions.len(), 3);
    assert!(page.regions[0].status.is_rendered());
    assert_eq!(
        page.regions[1].status,
        RegionStatus::Skipped {
            reason: SkipReason::SfxPolicy
        }
    );
    assert_eq!(
        page.regions[0].layout.as_ref().map(|l| l.lines.len()),
        Some(1)
    );
}

#[test]
fn erase_specs_fixture_round_trips() {
    let specs: Vec<EraseSpec> = golden("erase_specs.json");
    assert_eq!(specs.len(), 3);
    assert_eq!(specs[2], EraseSpec::None);
}

#[test]
fn events_fixture_round_trips() {
    let events: Vec<PipelineEvent> = golden("events.json");
    assert_eq!(events.len(), 6);
}

#[test]
fn page_done_event_round_trips() {
    let page: PageResult = golden("page_result.json");
    let ev = PipelineEvent::PageDone(page);
    let back: PipelineEvent =
        serde_json::from_str(&serde_json::to_string(&ev).expect("ser")).expect("de");
    assert_eq!(back, ev);
}

#[test]
fn provider_errors_fixture_round_trips() {
    let errs: Vec<ProviderError> = golden("provider_errors.json");
    assert_eq!(errs.len(), 11);
    assert_eq!(errs[6], ProviderError::BadRequest("too large".into()));
}

#[test]
fn rendered_region_with_layout_is_valid() {
    let r = region(RegionStatus::Rendered, patch(), some_layout());
    assert_eq!(r.validate(), Ok(()));
}

#[test]
fn skipped_or_failed_region_with_erase_is_rejected() {
    for status in [
        RegionStatus::Skipped {
            reason: SkipReason::EmptyText,
        },
        RegionStatus::Failed {
            reason: RegionFailure::LayoutDoesNotFit,
        },
    ] {
        let r = region(status.clone(), patch(), None);
        assert_eq!(
            r.validate(),
            Err(ContractError::EraseWithoutRender("r".into()))
        );
        assert_eq!(region(status, EraseSpec::None, None).validate(), Ok(()));
    }
}

#[test]
fn patch_is_rejected_without_render() {
    let status = RegionStatus::Skipped {
        reason: SkipReason::ComplexBackground,
    };
    assert_eq!(
        region(status, patch(), None).validate(),
        Err(ContractError::EraseWithoutRender("r".into()))
    );
}

#[test]
fn rendered_region_may_have_no_erase() {
    let r = region(RegionStatus::Rendered, EraseSpec::None, some_layout());
    assert_eq!(r.validate(), Ok(()));
}

#[test]
fn rendered_region_without_layout_is_rejected() {
    let r = region(RegionStatus::Rendered, EraseSpec::None, None);
    assert_eq!(
        r.validate(),
        Err(ContractError::RenderedWithoutLayout("r".into()))
    );
}

#[test]
fn invalid_bounds_and_schema_version_are_rejected() {
    let mut r = region(RegionStatus::Rendered, patch(), some_layout());
    r.bounds = NormalizedRect {
        x: 0.9,
        y: 0.0,
        width: 0.5,
        height: 0.1,
    };
    assert_eq!(r.validate(), Err(ContractError::InvalidBounds("r".into())));

    let mut page: PageResult = golden("page_result.json");
    page.schema_version = 2;
    assert_eq!(
        page.validate(),
        Err(ContractError::SchemaVersion {
            found: 2,
            expected: 1
        })
    );
}

#[test]
fn cancel_token_is_shared_between_clones() {
    let a = CancelToken::default();
    let b = a.clone();
    assert!(!b.is_cancelled());
    a.cancel();
    assert!(b.is_cancelled());
}

#[test]
fn request_fixture_carries_chain_context_and_glossary_meta() {
    let req: TranslateBatchRequest = golden("request.json");
    assert_eq!(req.options.chain.len(), 2);
    assert_eq!(req.options.chain[1].kind, ProviderKind::OpenAiCompatible);
    assert_eq!(req.pages[0].context[0].speaker.as_deref(), Some("Aoi"));
    assert_eq!(
        req.options.glossary[0].category,
        Some(GlossaryCategory::Honorific)
    );
}

#[test]
fn legacy_request_without_new_fields_still_deserializes() {
    let req: TranslateBatchRequest =
        serde_json::from_str(&fixture("request_legacy.json")).expect("legacy request");
    assert_eq!(req.options.provider, "gemini");
    assert!(req.options.chain.is_empty());
    assert!(req.pages[0].context.is_empty());
    assert_eq!(req.options.glossary[0].category, None);
    assert_eq!(req.options.glossary[0].notes, None);
}

#[test]
fn page_result_fixture_exposes_speaker_and_emotion() {
    let page: PageResult = golden("page_result.json");
    let r = &page.regions[0];
    assert_eq!(r.speaker.as_deref(), Some("Aoi"));
    assert_eq!(r.emotion.as_deref(), Some("happy"));
    assert_eq!(r.intensity, Some(0.5));
    assert_eq!(page.regions[1].speaker, None);
}

#[test]
fn legacy_page_result_without_new_fields_deserializes_and_omits_them() {
    let text = fixture("page_result_legacy.json");
    let page: PageResult = serde_json::from_str(&text).expect("legacy page");
    page.validate().expect("valid");
    assert!(page.regions.iter().all(|r| r.speaker.is_none()));
    let out = serde_json::to_string(&page).expect("ser");
    assert!(!out.contains("speaker") && !out.contains("intensity"));
}

fn every_error() -> Vec<ProviderError> {
    vec![
        ProviderError::RateLimited {
            retry_after_ms: Some(5),
        },
        ProviderError::RateLimited {
            retry_after_ms: None,
        },
        ProviderError::QuotaExhausted {
            reset_hint_ms: Some(9),
        },
        ProviderError::QuotaExhausted {
            reset_hint_ms: None,
        },
        ProviderError::Auth { status: 403 },
        ProviderError::ContentBlocked {
            reason: "safety".into(),
        },
        ProviderError::BadRequest("x".into()),
        ProviderError::Server(500),
        ProviderError::Network,
        ProviderError::Timeout,
        ProviderError::InvalidResponse("y".into()),
    ]
}

#[test]
fn every_provider_error_round_trips_with_matching_kind_name() {
    for e in every_error() {
        let json = serde_json::to_string(&e).expect("ser");
        let back: ProviderError = serde_json::from_str(&json).expect("de");
        assert_eq!(back, e, "{json}");
        assert!(!e.to_string().is_empty());
        let v = serde_json::to_value(&e).expect("value");
        let tag = v
            .as_str()
            .map(str::to_owned)
            .or_else(|| v.as_object().and_then(|o| o.keys().next().cloned()));
        assert_eq!(tag.as_deref(), Some(e.kind_name()));
    }
}

#[test]
fn legacy_provider_error_forms_still_deserialize() {
    let parse = |s: &str| serde_json::from_str::<ProviderError>(s).expect(s);
    assert_eq!(parse(r#""auth""#), ProviderError::Auth { status: 401 });
    assert_eq!(
        parse(r#"{"auth":null}"#),
        ProviderError::Auth { status: 401 }
    );
    assert_eq!(
        parse(r#"{"auth":403}"#),
        ProviderError::Auth { status: 403 }
    );
    assert_eq!(
        parse(r#"{"rate_limited":1500}"#),
        ProviderError::RateLimited {
            retry_after_ms: Some(1500)
        }
    );
    assert_eq!(
        parse(r#"{"rate_limited":{"retry_after_ms":7}}"#),
        ProviderError::RateLimited {
            retry_after_ms: Some(7)
        }
    );
    assert_eq!(
        parse(r#"{"quota_exhausted":60000}"#),
        ProviderError::QuotaExhausted {
            reset_hint_ms: Some(60000)
        }
    );
}

#[test]
fn unknown_provider_error_is_rejected() {
    assert!(serde_json::from_str::<ProviderError>(r#""boom""#).is_err());
    assert!(serde_json::from_str::<ProviderError>(r#"{"nope":1}"#).is_err());
}

#[test]
fn retry_hint_is_not_clamped() {
    let hour = ProviderError::QuotaExhausted {
        reset_hint_ms: Some(3_600_000),
    };
    assert_eq!(
        hour.retry_hint(),
        Some(std::time::Duration::from_secs(3600))
    );
    let rl = ProviderError::RateLimited {
        retry_after_ms: Some(u64::MAX),
    };
    assert_eq!(
        rl.retry_hint(),
        Some(std::time::Duration::from_millis(u64::MAX))
    );
    assert_eq!(ProviderError::Timeout.retry_hint(), None);
    assert_eq!(
        ProviderError::RateLimited {
            retry_after_ms: None
        }
        .retry_hint(),
        None
    );
}

#[test]
fn provider_spec_never_leaks_credentials() {
    let spec = ProviderSpec {
        kind: ProviderKind::Gemini,
        endpoint: None,
        model: "m".into(),
        languages: vec!["ja".into()],
        credentials: Credentials {
            api_key: Some("sk-secret-999".into()),
        },
    };
    assert!(!format!("{spec:?}").contains("sk-secret-999"));
    assert!(format!("{spec:?}").contains("redacted"));
    let json = serde_json::to_string(&spec).expect("ser");
    assert!(!json.contains("sk-secret-999") && !json.contains("credentials"));
    let back: ProviderSpec = serde_json::from_str(&json).expect("de");
    assert_eq!(back.credentials, Credentials::default());
}
