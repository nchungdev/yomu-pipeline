//! CLI and worker report per-region results and survive bad input.

use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn make_image(path: &Path) {
    let mut img = RgbaImage::from_pixel(150, 150, Rgba([30, 30, 30, 255]));
    for y in 10..140 {
        for x in 10..140 {
            let border = !(12..138).contains(&x) || !(12..138).contains(&y);
            img.put_pixel(
                x,
                y,
                if border {
                    Rgba([0, 0, 0, 255])
                } else {
                    Rgba([255, 255, 255, 255])
                },
            );
        }
    }
    img.save(path).expect("image saved");
}

fn region(id: &str, text: &str, kind: &str) -> Value {
    json!({ "id": id, "bounds": { "x": 0.08, "y": 0.08, "width": 0.84, "height": 0.84 },
            "text": text, "kind": kind })
}

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("yomu_pipeline_{name}_{}", std::process::id()));
    std::fs::create_dir_all(&d).expect("dir");
    d
}

fn exe() -> Command {
    Command::new(env!("CARGO_BIN_EXE_yomu-pipeline"))
}

fn run_worker(args: &[&str], input: &[u8]) -> Output {
    let mut child = exe()
        .arg("worker")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input)
        .expect("write");
    child.wait_with_output().expect("finished")
}

fn lines(out: &Output) -> Vec<Value> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).expect("response line is JSON"))
        .collect()
}

#[test]
fn worker_reports_partial_with_region_reasons() {
    let d = dir("partial");
    let (img, out) = (d.join("in.png"), d.join("out.png"));
    make_image(&img);
    let job = json!({ "id": "j1", "image_path": img, "output_path": out,
        "regions": [region("a", "Xin chào", "balloon"), region("s", "Rầm", "sfx")] });
    let res = run_worker(&[], format!("{job}\n").as_bytes());
    let resp = &lines(&res)[0];
    assert_eq!(resp["status"], "partial");
    assert_eq!(
        resp["regions"][0],
        json!({ "id": "a", "state": "rendered" })
    );
    assert_eq!(
        resp["regions"][1],
        json!({ "id": "s", "state": "skipped", "reason": "sfx_policy" })
    );
    assert!(out.exists());
}

#[test]
fn worker_skips_pages_whose_sidecar_status_is_not_ok() {
    let d = dir("notok");
    let (img, out) = (d.join("in.png"), d.join("out.png"));
    make_image(&img);
    let job = json!({ "id": "j", "image_path": img, "output_path": out,
        "regions": { "status": "error", "regions": [] } });
    let resp = lines(&run_worker(&[], format!("{job}\n").as_bytes())).remove(0);
    assert_eq!(resp["status"], "skipped");
    assert!(!out.exists());
}

#[test]
fn worker_survives_bad_lines_and_recovers_ids() {
    let d = dir("badlines");
    let (img, out) = (d.join("in.png"), d.join("out.png"));
    make_image(&img);
    let good = json!({ "id": "good", "image_path": img, "output_path": out,
        "regions": [region("a", "Xin chào", "balloon")] });
    let mut input = Vec::new();
    input.extend_from_slice(b"{\"id\": \"broken-1\", \"image_path\": \n");
    input.extend_from_slice(b"{\"id\": \"bad-utf8\", \"x\": \"\xff\xfe\"}\n");
    input.extend_from_slice(b"not json at all\n");
    input.extend_from_slice(format!("{good}\n").as_bytes());
    let res = run_worker(&[], &input);
    assert!(res.status.success());
    let r = lines(&res);
    assert_eq!(r.len(), 4);
    assert_eq!(
        (r[0]["status"].as_str(), r[0]["id"].as_str()),
        (Some("error"), Some("broken-1"))
    );
    assert_eq!(
        (r[1]["status"].as_str(), r[1]["id"].as_str()),
        (Some("error"), Some("bad-utf8"))
    );
    assert_eq!(
        (r[2]["status"].as_str(), r[2]["id"].as_str()),
        (Some("error"), None)
    );
    assert_eq!(
        (r[3]["status"].as_str(), r[3]["id"].as_str()),
        (Some("ok"), Some("good"))
    );
}

#[test]
fn worker_leaves_no_temp_files_and_reports_missing_image() {
    let d = dir("atomic");
    let (img, out) = (d.join("in.png"), d.join("out.png"));
    make_image(&img);
    let ok = json!({ "id": "a", "image_path": img, "output_path": out,
        "regions": [region("a", "Xin chào", "balloon")] });
    let missing = json!({ "id": "b", "image_path": d.join("nope.png"), "output_path": d.join("o2.png"),
        "regions": [] });
    let res = run_worker(&[], format!("{ok}\n{missing}\n").as_bytes());
    let r = lines(&res);
    assert_eq!(r[0]["status"], "ok");
    assert_eq!(r[1]["status"], "error");
    let mut names: Vec<_> = std::fs::read_dir(&d)
        .expect("read dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, vec!["in.png", "out.png"]);
}

#[test]
fn worker_rejects_unknown_mode() {
    let res = exe()
        .args(["worker", "--mode", "socket"])
        .output()
        .expect("ran");
    assert!(!res.status.success());
}

#[test]
fn cli_inpaint_prints_per_region_report() {
    let d = dir("cli");
    let (img, out, regions) = (d.join("in.png"), d.join("out.png"), d.join("r.json"));
    make_image(&img);
    let sidecar = json!({ "status": "ok", "regions": [
        { "recognized": { "id": "a", "bounds": { "x": 0.08, "y": 0.08, "width": 0.84, "height": 0.84 }, "text": "原" },
          "translated_text": "Xin chào" },
        { "recognized": { "id": "b", "bounds": { "x": 0.1, "y": 0.1, "width": 0.2, "height": 0.2 }, "text": "原" } }
    ] });
    std::fs::write(&regions, sidecar.to_string()).expect("write");
    let res = exe()
        .args(["inpaint", "--image"])
        .arg(&img)
        .arg("--regions")
        .arg(&regions)
        .arg("--output")
        .arg(&out)
        .output()
        .expect("ran");
    assert!(res.status.success());
    let report: Value = serde_json::from_slice(&res.stdout).expect("json report");
    assert_eq!(report["status"], "partial");
    let states: Vec<_> = report["regions"]
        .as_array()
        .expect("array")
        .iter()
        .map(|r| {
            (
                r["id"].as_str().unwrap_or(""),
                r["state"].as_str().unwrap_or(""),
            )
        })
        .collect();
    assert_eq!(states, vec![("a", "rendered"), ("b", "skipped")]);
    assert!(out.exists());
}

#[test]
fn cli_inpaint_fails_on_missing_image() {
    let d = dir("cli_err");
    let regions = d.join("r.json");
    std::fs::write(&regions, "[]").expect("write");
    let res = exe()
        .args(["inpaint", "--image"])
        .arg(d.join("none.png"))
        .arg("--regions")
        .arg(&regions)
        .arg("--output")
        .arg(d.join("o.png"))
        .output()
        .expect("ran");
    assert!(!res.status.success());
}
