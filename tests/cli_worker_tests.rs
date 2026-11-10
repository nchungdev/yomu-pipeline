use image::{Rgba, RgbaImage};
use std::io::Write;
use std::process::{Command, Stdio};
use yomu_pipeline::types::{BubbleStyleHint, NormalizedRect, RegionInput};

fn create_temp_image(path: &std::path::Path) {
    let mut img = RgbaImage::from_pixel(150, 150, Rgba([30, 30, 30, 255]));
    for y in 10..140 {
        for x in 10..140 {
            let is_border = !(12..138).contains(&x) || !(12..138).contains(&y);
            if is_border {
                img.put_pixel(x, y, Rgba([0, 0, 0, 255]));
            } else {
                img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
    }
    img.save(path).expect("saved temp image");
}

#[test]
fn test_cli_inpaint_subcommand() {
    let temp_dir = std::env::temp_dir().join("yomu_pipeline_cli_test");
    std::fs::create_dir_all(&temp_dir).unwrap();

    let img_path = temp_dir.join("test_in.png");
    let out_path = temp_dir.join("test_out.png");
    let regions_path = temp_dir.join("regions.json");

    create_temp_image(&img_path);

    let regions = vec![RegionInput {
        id: "b1".to_string(),
        bounds: NormalizedRect::new(0.08, 0.08, 0.84, 0.84).unwrap(),
        text: "Chào bạn!".to_string(),
        style_hint: BubbleStyleHint::Balloon,
        kind: Default::default(),
        typography: None,
    }];
    std::fs::write(&regions_path, serde_json::to_string(&regions).unwrap()).unwrap();

    let exe = env!("CARGO_BIN_EXE_yomu-pipeline");
    let status = Command::new(exe)
        .arg("inpaint")
        .arg("--image")
        .arg(&img_path)
        .arg("--regions")
        .arg(&regions_path)
        .arg("--output")
        .arg(&out_path)
        .status()
        .expect("executed inpaint command");

    assert!(status.success());
    assert!(out_path.exists());
}

#[test]
fn test_cli_worker_subcommand() {
    let temp_dir = std::env::temp_dir().join("yomu_pipeline_worker_test");
    std::fs::create_dir_all(&temp_dir).unwrap();

    let img_path = temp_dir.join("worker_in.png");
    let out_path = temp_dir.join("worker_out.png");

    create_temp_image(&img_path);

    let regions = vec![RegionInput {
        id: "b1".to_string(),
        bounds: NormalizedRect::new(0.08, 0.08, 0.84, 0.84).unwrap(),
        text: "Worker OK".to_string(),
        style_hint: BubbleStyleHint::Balloon,
        kind: Default::default(),
        typography: None,
    }];

    let job_json = serde_json::json!({
        "id": "job_123",
        "image_path": img_path,
        "regions": regions,
        "output_path": out_path
    });

    let exe = env!("CARGO_BIN_EXE_yomu-pipeline");
    let mut child = Command::new(exe)
        .arg("worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn worker");

    {
        let stdin = child.stdin.as_mut().expect("stdin handle");
        writeln!(stdin, "{}", job_json).expect("write job line");
    }

    let output = child.wait_with_output().expect("worker finished");
    assert!(output.status.success());

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    assert!(stdout_str.contains("\"status\":\"ok\""));
    assert!(stdout_str.contains("\"id\":\"job_123\""));
    assert!(out_path.exists());
}
