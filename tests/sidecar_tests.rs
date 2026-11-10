use serde_json::json;
use yomu_pipeline::contract::v1::{RegionFailure, RegionKind, RegionStatus, SkipReason};
use yomu_pipeline::{parse_page, SidecarError};

fn item(id: &str, translated: Option<&str>) -> serde_json::Value {
    let mut v = json!({
        "recognized": {
            "id": id,
            "text": "原文",
            "bounds": { "x": 0.1, "y": 0.1, "width": 0.3, "height": 0.2 }
        }
    });
    if let Some(t) = translated {
        v["translated_text"] = json!(t);
    }
    v
}

#[test]
fn sidecar_regions_use_translated_text_kind_and_typography() {
    let mut a = item("a", Some("Xin chào"));
    a["kind"] = json!("narration");
    a["typography"] = json!({ "text_color": [1, 2, 3, 255], "is_vertical": true });
    let page = parse_page(&json!({ "status": "ok", "regions": [a] })).expect("parse");
    assert_eq!(page.regions.len(), 1);
    let r = &page.regions[0];
    assert_eq!(r.text, "Xin chào");
    assert_eq!(r.kind, RegionKind::Narration);
    let t = r.typography.expect("typography kept");
    assert_eq!(t.text_color, [1, 2, 3, 255]);
    assert!(t.is_vertical);
}

#[test]
fn kind_may_live_inside_recognized_and_unknown_kind_is_other() {
    let mut a = item("a", Some("x"));
    a["recognized"]["kind"] = json!("Sfx");
    let mut b = item("b", Some("y"));
    b["kind"] = json!("mystery");
    let page = parse_page(&json!({ "regions": [a, b] })).expect("parse");
    assert_eq!(page.regions[0].kind, RegionKind::Sfx);
    assert_eq!(page.regions[1].kind, RegionKind::Other);
}

#[test]
fn missing_translation_is_rejected_never_replaced_by_source_text() {
    let page =
        parse_page(&json!({ "regions": [item("a", None), item("b", Some("ok"))] })).expect("parse");
    assert_eq!(page.regions.len(), 1);
    assert_eq!(page.regions[0].id, "b");
    assert_eq!(page.rejected.len(), 1);
    assert_eq!(page.rejected[0].id, "a");
    assert_eq!(
        page.rejected[0].status,
        RegionStatus::Skipped {
            reason: SkipReason::MissingTranslation
        }
    );
    assert_eq!(page.warnings.len(), 1);
}

#[test]
fn non_ok_status_skips_the_page() {
    for status in ["error", "pending"] {
        let err = parse_page(&json!({ "status": status, "regions": [] })).expect_err("skipped");
        assert_eq!(err, SidecarError::NotOk(status.into()));
    }
    let err = parse_page(&json!({ "status": null, "regions": [] })).expect_err("skipped");
    assert!(matches!(err, SidecarError::NotOk(_)));
}

#[test]
fn invalid_bounds_drop_only_that_region() {
    let mut bad = item("bad", Some("x"));
    bad["recognized"]["bounds"] = json!({ "x": 0.9, "y": 0.9, "width": 0.5, "height": 0.5 });
    let mut missing = item("nobounds", Some("x"));
    missing["recognized"]
        .as_object_mut()
        .expect("obj")
        .remove("bounds");
    let mut zero = item("zero", Some("x"));
    zero["recognized"]["bounds"] = json!({ "x": 0.1, "y": 0.1, "width": 0.0, "height": 0.2 });
    let page = parse_page(&json!({ "regions": [bad, missing, zero, item("ok", Some("x"))] }))
        .expect("parse");
    assert_eq!(page.regions.len(), 1);
    assert_eq!(page.rejected.len(), 3);
    assert!(page.rejected.iter().all(|r| r.status
        == RegionStatus::Failed {
            reason: RegionFailure::InvalidBounds
        }));
}

#[test]
fn plain_region_array_is_accepted_with_per_item_validation() {
    let good = json!({ "id": "g", "bounds": { "x": 0.1, "y": 0.1, "width": 0.2, "height": 0.2 }, "text": "hi", "kind": "sfx" });
    let bad_bounds = json!({ "id": "b", "bounds": { "x": 2.0, "y": 0.1, "width": 0.2, "height": 0.2 }, "text": "hi" });
    let malformed = json!({ "id": "m", "text": "hi" });
    let page = parse_page(&json!([good, bad_bounds, malformed])).expect("parse");
    assert_eq!(page.regions.len(), 1);
    assert_eq!(page.regions[0].kind, RegionKind::Sfx);
    assert_eq!(page.rejected.len(), 2);
    assert_eq!(
        page.rejected[1].status,
        RegionStatus::Failed {
            reason: RegionFailure::Malformed
        }
    );
}

#[test]
fn unrecognized_shapes_are_format_errors() {
    for v in [
        json!(5),
        json!("x"),
        json!({ "nope": [] }),
        json!({ "regions": 3 }),
    ] {
        assert!(matches!(parse_page(&v), Err(SidecarError::Format(_))));
    }
    assert_eq!(parse_page(&json!([])).expect("empty").regions.len(), 0);
}
