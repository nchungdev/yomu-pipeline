//! Command-line interface and background worker for yomu-pipeline.

#[path = "yomu_pipeline/process.rs"]
mod process;
#[path = "yomu_pipeline/worker.rs"]
mod worker;

use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use yomu_pipeline::FontRenderer;

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

    /// Path to regions JSON file (region array or `.yomu.json` sidecar)
    #[arg(short, long)]
    regions: PathBuf,

    /// Path to output rendered image
    #[arg(short, long)]
    output: PathBuf,

    /// Optional path to custom TTF/OTF font file
    #[arg(short, long)]
    font: Option<PathBuf>,
}

#[derive(Clone, Copy, ValueEnum)]
enum WorkerMode {
    /// JSON lines on stdin, one response line per job on stdout
    Stdin,
}

#[derive(Args)]
struct WorkerArgs {
    /// Input mode
    #[arg(long, value_enum, default_value_t = WorkerMode::Stdin)]
    mode: WorkerMode,

    /// Per-job timeout in seconds
    #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u64).range(1..))]
    job_timeout_secs: u64,

    /// Optional path to custom TTF/OTF font file
    #[arg(short, long)]
    font: Option<PathBuf>,
}

fn load_font(font_path: Option<&PathBuf>) -> Result<FontRenderer, Box<dyn std::error::Error>> {
    match font_path {
        Some(p) => Ok(FontRenderer::from_file(p)?),
        None => Ok(FontRenderer::default()),
    }
}

/// Prints the per-region report as JSON. Exit code is non-zero only for an `error` status.
fn run_single_inpaint(args: InpaintArgs) -> Result<(), Box<dyn std::error::Error>> {
    let renderer = load_font(args.font.as_ref())?;
    let regions_json: serde_json::Value =
        serde_json::from_reader(std::fs::File::open(&args.regions)?)?;
    let resp = process::run_page_job(
        None,
        &renderer,
        &args.image,
        &regions_json,
        &args.output,
        &AtomicBool::new(false),
    );
    println!("{}", serde_json::to_string_pretty(&resp)?);
    match resp.error.filter(|_| resp.status == "error") {
        Some(msg) => Err(msg.into()),
        None => Ok(()),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Inpaint(args) => run_single_inpaint(args),
        Commands::Worker(args) => {
            let WorkerMode::Stdin = args.mode;
            let renderer = load_font(args.font.as_ref())?;
            worker::run_worker(&renderer, Duration::from_secs(args.job_timeout_secs))
        }
    }
}
