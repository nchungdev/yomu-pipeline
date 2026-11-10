#![cfg(feature = "llm")]

use yomu_pipeline::contract::v1::{ReadingDirection, RegionKind};
use yomu_pipeline::llm::{
    extract_json_payload, parse_vision_regions, sort_regions_by_reading_order, LlmRegion,
    ParseError,
};
use yomu_pipeline::types::NormalizedRect;

fn parse(raw: &str) -> Vec<LlmRegion> {
    parse_vision_regions(raw, ReadingDirection::Ltr).expect("parse ok")
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn fenced_json_and_prose_are_handled() {
    let body = r#"{"regions":[{"id":"a","box_2d":[100,200,300,400],"text":"Hi","translated_text":"Chào"}]}"#;
    for raw in [
        format!("```json\n{body}\n```"),
        format!("```\n{body}\n```"),
        format!("```JSON\n{body}\n```"),
        format!("Sure! Here is the result:\n{body}\nHope it helps {{}}."),
        format!("<think>{{not json}}</think>\n{body}"),
        body.to_owned(),
    ] {
        let r = parse(&raw);
        assert_eq!(r.len(), 1, "raw: {raw}");
        assert_eq!(r[0].translated, "Chào");
    }
}

#[test]
fn bare_array_is_accepted() {
    let r = parse(r#"[{"id":"a","box_2d":[0,0,500,500],"text":"x","translated_text":"y"}]"#);
    assert_eq!(r.len(), 1);
    assert_eq!(extract_json_payload("noise [1, 2] tail"), "[1, 2]");
}

#[test]
fn box_2d_thousand_scale_is_normalized() {
    let r = parse(r#"{"regions":[{"box_2d":[100,200,400,600],"text":"a","translated_text":"b"}]}"#);
    let b = r[0].bounds;
    assert!(near(b.x, 0.2) && near(b.y, 0.1) && near(b.width, 0.4) && near(b.height, 0.3));
}

#[test]
fn already_normalized_boxes_are_accepted() {
    let r = parse(r#"{"regions":[{"box_2d":[0.1,0.2,0.4,0.6],"text":"a","translated_text":"b"}]}"#);
    let b = r[0].bounds;
    assert!(near(b.x, 0.2) && near(b.y, 0.1) && near(b.width, 0.4) && near(b.height, 0.3));
    let r = parse(
        r#"{"regions":[{"x":0.1,"y":0.2,"width":0.3,"height":0.1,"text":"a","translated_text":"b"}]}"#,
    );
    assert!(near(r[0].bounds.x, 0.1) && near(r[0].bounds.width, 0.3));
}

#[test]
fn xywh_thousand_scale_is_normalized() {
    let r = parse(
        r#"{"regions":[{"x":500,"y":600,"width":200,"height":100,"text":"a","translated_text":"b"}]}"#,
    );
    let b = r[0].bounds;
    assert!(near(b.x, 0.5) && near(b.y, 0.6) && near(b.width, 0.2) && near(b.height, 0.1));
}

#[test]
fn edge_and_out_of_range_boxes_are_clamped() {
    let r = parse(
        r#"{"regions":[{"x":0.99,"y":0.99,"width":0.05,"height":0.05,"text":"a","translated_text":"b"}]}"#,
    );
    let b = r[0].bounds;
    assert!(near(b.x, 0.98) && near(b.width, 0.02) && b.validate().is_ok());
    let r =
        parse(r#"{"regions":[{"box_2d":[-50,-20,2000,1500],"text":"a","translated_text":"b"}]}"#);
    assert!(r[0].bounds.validate().is_ok());
    let r = parse(
        r#"{"regions":[{"x":0.2,"y":0.3,"width":-0.5,"height":0.0,"text":"a","translated_text":"b"}]}"#,
    );
    let b = r[0].bounds;
    assert!(near(b.x, 0.2) && near(b.y, 0.3) && b.width > 0.0 && b.height > 0.0);
}

#[test]
fn garbage_boxes_never_panic() {
    let cases = [
        r#"{"x":1e38,"y":-1e38,"width":3e38,"height":-3e38}"#,
        r#"{"box_2d":[1e38,-1e38,3e38,-3e38]}"#,
        r#"{"box_2d":[1,2,3]}"#,
        r#"{"box_2d":["a",null,{},[]]}"#,
        r#"{"box_2d":[0,0,0,0]}"#,
        r#"{"x":null}"#,
        r#"{"x":1e999,"y":0,"width":1,"height":1}"#,
    ];
    for c in cases {
        let raw = format!(
            r#"{{"regions":[{},{{"box_2d":[0,0,100,100],"text":"ok","translated_text":"ok"}}]}}"#,
            c.replacen('}', r#","text":"t","translated_text":"u"}"#, 1)
        );
        if let Ok(rs) = parse_vision_regions(&raw, ReadingDirection::Rtl) {
            assert!(rs.iter().all(|r| r.bounds.validate().is_ok()), "case {c}");
        }
    }
}

#[test]
fn empty_text_and_missing_box_are_dropped() {
    let r = parse(
        r#"{"regions":[
        {"box_2d":[0,0,100,100],"text":"  ","translated_text":"  "},
        {"box_2d":[0,0,100,100],"text":"src","translated_text":""},
        {"text":"nobox","translated_text":"x"},
        {"box_2d":[0,0,100,100],"text":"keep","translated_text":"giữ"}]}"#,
    );
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].text, "keep");
}

#[test]
fn malformed_item_is_skipped_not_fatal() {
    let r = parse(
        r#"{"regions":[42,{"box_2d":[0,0,100,100],"text":1},{"box_2d":[0,0,100,100],"text":"a","translated_text":"b"}]}"#,
    );
    assert_eq!(r.len(), 1);
}

#[test]
fn kind_mapping() {
    let mk = |k: &str| {
        let raw = format!(
            r#"{{"regions":[{{"box_2d":[0,0,100,100],"text":"a","translated_text":"b","k":"{k}"}}]}}"#
        );
        parse(&raw)[0].kind
    };
    assert_eq!(mk("dialogue"), RegionKind::Balloon);
    assert_eq!(mk("thought"), RegionKind::Balloon);
    assert_eq!(mk("narration"), RegionKind::Narration);
    assert_eq!(mk("SFX"), RegionKind::Sfx);
    assert_eq!(mk("sign"), RegionKind::Sign);
    assert_eq!(mk("invalid"), RegionKind::Balloon);
}

#[test]
fn speaker_and_tag_fields() {
    let r = parse(
        r#"{"regions":[
        {"id":"b1","box_2d":[100,200,300,400],"text":"a","translated_text":"b","k":"dialogue","spk":"Conan","g":"m","age":"child","emo":"serious","int":0.85},
        {"id":"b2","box_2d":[500,600,700,800],"text":"a","translated_text":"b","spk":"null","g":"alien","age":"ancient","emo":"bored","int":2.5},
        {"id":"b3","box_2d":[800,600,900,800],"text":"a","translated_text":"b","spk":" ","g":"Female","int":-1}]}"#,
    );
    let (a, b, c) = (&r[0], &r[1], &r[2]);
    assert_eq!(a.speaker.as_deref(), Some("Conan"));
    assert_eq!(a.gender.as_deref(), Some("m"));
    assert_eq!(a.age.as_deref(), Some("child"));
    assert_eq!(a.emotion.as_deref(), Some("serious"));
    assert!(near(a.intensity.unwrap_or(-1.0), 0.85));
    assert_eq!((b.speaker.as_deref(), b.gender.as_deref()), (None, None));
    assert_eq!((b.age.as_deref(), b.emotion.as_deref()), (None, None));
    assert_eq!(b.intensity, Some(1.0));
    assert_eq!(c.speaker, None);
    assert_eq!(c.gender.as_deref(), Some("f"));
    assert_eq!(c.intensity, None);
}

#[test]
fn duplicate_and_missing_ids_become_unique() {
    let r = parse(
        r#"{"regions":[
        {"id":"a","box_2d":[0,0,100,100],"text":"1","translated_text":"1"},
        {"id":"a","box_2d":[200,0,300,100],"text":"2","translated_text":"2"},
        {"box_2d":[400,0,500,100],"text":"3","translated_text":"3"}]}"#,
    );
    let mut ids: Vec<_> = r.iter().map(|x| x.id.clone()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 3);
}

fn rect_region(id: &str, x: f32, y: f32, w: f32, h: f32) -> LlmRegion {
    LlmRegion {
        id: id.into(),
        bounds: NormalizedRect::new(x, y, w, h).expect("rect"),
        text: "t".into(),
        translated: "t".into(),
        kind: RegionKind::Balloon,
        speaker: None,
        gender: None,
        age: None,
        emotion: None,
        intensity: None,
    }
}

fn ids(rs: &[LlmRegion]) -> Vec<&str> {
    rs.iter().map(|r| r.id.as_str()).collect()
}

#[test]
fn reading_order_rtl_vs_ltr_with_row_grouping() {
    let base = vec![
        rect_region("bl", 0.1, 0.62, 0.2, 0.1),
        rect_region("tl", 0.1, 0.20, 0.2, 0.1),
        rect_region("tr", 0.6, 0.22, 0.2, 0.1),
        rect_region("br", 0.6, 0.60, 0.2, 0.1),
    ];
    let mut rtl = base.clone();
    sort_regions_by_reading_order(&mut rtl, ReadingDirection::Rtl);
    assert_eq!(ids(&rtl), ["tr", "tl", "br", "bl"]);
    let mut ltr = base;
    sort_regions_by_reading_order(&mut ltr, ReadingDirection::Ltr);
    assert_eq!(ids(&ltr), ["tl", "tr", "bl", "br"]);
}

#[test]
fn reading_order_is_stable_and_handles_tiny_inputs() {
    let mut none: Vec<LlmRegion> = Vec::new();
    sort_regions_by_reading_order(&mut none, ReadingDirection::Rtl);
    let mut same = vec![
        rect_region("a", 0.3, 0.3, 0.2, 0.1),
        rect_region("b", 0.3, 0.3, 0.2, 0.1),
    ];
    sort_regions_by_reading_order(&mut same, ReadingDirection::Rtl);
    assert_eq!(ids(&same), ["a", "b"]);
}

#[test]
fn parse_sorts_by_direction() {
    let raw = r#"{"regions":[
        {"id":"l","box_2d":[200,100,300,300],"text":"a","translated_text":"L"},
        {"id":"r","box_2d":[200,600,300,800],"text":"a","translated_text":"R"}]}"#;
    let rtl = parse_vision_regions(raw, ReadingDirection::Rtl).expect("ok");
    assert_eq!(ids(&rtl), ["r", "l"]);
    let ltr = parse_vision_regions(raw, ReadingDirection::Ltr).expect("ok");
    assert_eq!(ids(&ltr), ["l", "r"]);
}

#[test]
fn empty_regions_list_is_ok() {
    assert!(parse(r#"{"regions": []}"#).is_empty());
    assert!(parse("[]").is_empty());
}

#[test]
fn invalid_input_errors_without_echoing_it() {
    let secret = "SECRET-PAGE-TEXT-42";
    for raw in [
        "".to_owned(),
        "   \n\t ".to_owned(),
        format!("Sorry, I cannot process {secret} due to safety filters."),
        format!("{{\"regions\": \"{secret}\"}}"),
        format!("{{\"regions\": [ {secret} ]}}"),
        format!("\"{secret}\""),
        format!("{{{secret}}}"),
    ] {
        let err = parse_vision_regions(&raw, ReadingDirection::Ltr).expect_err("must fail");
        assert!(!err.to_string().contains(secret), "{err}");
        assert!(!format!("{err:?}").contains(secret), "{err:?}");
    }
    assert_eq!(
        parse_vision_regions("", ReadingDirection::Ltr),
        Err(ParseError::Empty)
    );
}
