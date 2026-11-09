//! Command-line interface and background worker for yomu-pipeline.

use clap::{Args, Parser, Subcommand};
use image::ImageReader;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use yomu_pipeline::{
    process_page, BubbleStyleHint, FontRenderer, NormalizedRect, PipelineOptions, RegionInput,
};

#[derive(Parser)]
#[command(
    name = "yomu-pipeline",
    version,
    about = "Headless manga/comic speech bubble inpainting and typesetting engine"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Inpaint bubbles and typeset translated text for a single comic page image
    Inpaint(InpaintArgs),
    /// Run as a headless daemon worker reading jobs from stdin JSON lines
    Worker(WorkerArgs),
}

#[derive(Args)]
struct InpaintArgs {
    /// Path to input image (PNG, JPEG, WebP)
    #[arg(short, long)]
    image: PathBuf,

    /// Path to regions JSON file
    #[arg(short, long)]
    regions: PathBuf,

    /// Path to output rendered image
    #[arg(short, long)]
    output: PathBuf,

    /// Optional path to custom TTF/OTF font file
    #[arg(short, long)]
    font: Option<PathBuf>,
}

#[derive(Args)]
struct WorkerArgs {
    /// Input mode: currently supports "stdin" (JSON lines)
    #[arg(long, default_value = "stdin")]
    mode: String,

    /// Optional path to custom TTF/OTF font file
    #[arg(short, long)]
    font: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkerJob {
    pub id: Option<String>,
    pub image_path: PathBuf,
    pub regions: Vec<RegionInput>,
    pub output_path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkerResponse {
    pub id: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_path: Option<PathBuf>,
}

fn load_font(font_path: Option<&PathBuf>) -> Result<FontRenderer, Box<dyn std::error::Error>> {
    match font_path {
        Some(p) => Ok(FontRenderer::from_file(p)?),
        None => Ok(FontRenderer::default()),
    }
}

fn parse_regions_flexible<R: std::io::Read>(
    reader: R,
) -> Result<Vec<RegionInput>, Box<dyn std::error::Error>> {
    let value: serde_json::Value = serde_json::from_reader(reader)?;
    if let Ok(regions) = serde_json::from_value::<Vec<RegionInput>>(value.clone()) {
        return Ok(regions);
    }
    if let Some(arr) = value.get("regions").and_then(|r| r.as_array()) {
        let mut out = Vec::new();
        for item in arr {
            let recognized = item.get("recognized");
            let id = recognized
                .and_then(|r| r.get("id"))
                .and_then(|i| i.as_str())
                .unwrap_or("region")
                .to_string();
            let bounds_val = recognized.and_then(|r| r.get("bounds"));
            let bounds: NormalizedRect = if let Some(b) = bounds_val {
                serde_json::from_value(b.clone())?
            } else {
                continue;
            };
            let text = item
                .get("translated_text")
                .and_then(|t| t.as_str())
                .or_else(|| {
                    recognized
                        .and_then(|r| r.get("text"))
                        .and_then(|t| t.as_str())
                })
                .unwrap_or("")
                .to_string();
            out.push(RegionInput {
                id,
                bounds,
                text,
                style_hint: BubbleStyleHint::Balloon,
            });
        }
        return Ok(out);
    }
    Err("Failed to parse regions as either Vec<RegionInput> or Sidecar JSON with 'regions'".into())
}

fn run_single_inpaint(args: InpaintArgs) -> Result<(), Box<dyn std::error::Error>> {
    let renderer = load_font(args.font.as_ref())?;
    let img = ImageReader::open(&args.image)?.decode()?.into_rgba8();
    let file = File::open(&args.regions)?;
    let regions = parse_regions_flexible(file)?;

    let result = process_page(&img, &regions, &renderer, PipelineOptions::default());
    result.image.save(&args.output)?;
    println!(
        "Processed {} regions successfully -> {}",
        regions.len(),
        args.output.display()
    );
    Ok(())
}

fn run_worker(args: WorkerArgs) -> Result<(), Box<dyn std::error::Error>> {
    let renderer = load_font(args.font.as_ref())?;
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    for line in BufReader::new(stdin).lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let job: Result<WorkerJob, _> = serde_json::from_str(trimmed);
        let resp = match job {
            Ok(job) => match ImageReader::open(&job.image_path)
                .map_err(|e| e.to_string())
                .and_then(|r| r.decode().map_err(|e| e.to_string()))
            {
                Ok(dyn_img) => {
                    let rgba = dyn_img.into_rgba8();
                    let res =
                        process_page(&rgba, &job.regions, &renderer, PipelineOptions::default());
                    match res.image.save(&job.output_path) {
                        Ok(_) => WorkerResponse {
                            id: job.id,
                            status: "ok".to_string(),
                            error: None,
                            output_path: Some(job.output_path),
                        },
                        Err(e) => WorkerResponse {
                            id: job.id,
                            status: "error".to_string(),
                            error: Some(format!("Failed to save output image: {e}")),
                            output_path: None,
                        },
                    }
                }
                Err(e) => WorkerResponse {
                    id: job.id,
                    status: "error".to_string(),
                    error: Some(format!("Failed to read input image: {e}")),
                    output_path: None,
                },
            },
            Err(e) => WorkerResponse {
                id: None,
                status: "error".to_string(),
                error: Some(format!("Invalid job JSON: {e}")),
                output_path: None,
            },
        };

        let json_line = serde_json::to_string(&resp)?;
        writeln!(out, "{json_line}")?;
        out.flush()?;
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Inpaint(args) => run_single_inpaint(args),
        Commands::Worker(args) => run_worker(args),
    }
}
