//! Stdin JSON-lines worker: one job per line, one response per line, never dies on a bad job.

use serde::Deserialize;
use std::io::{BufRead, BufReader, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;
use yomu_pipeline::FontRenderer;

use crate::process::{run_page_job, WorkerResponse};

#[derive(Debug, Deserialize)]
pub struct WorkerJob {
    pub id: Option<String>,
    pub image_path: PathBuf,
    /// A `RegionInput` array or a sidecar object (see `yomu_pipeline::parse_page`).
    pub regions: serde_json::Value,
    pub output_path: PathBuf,
}

/// Best-effort recovery of the top-level `"id"` string from a line that is not valid JSON.
pub fn recover_id(line: &str) -> Option<String> {
    let key = line.find("\"id\"")?;
    let rest = line[key + 4..].trim_start().strip_prefix(':')?;
    let mut chars = rest.trim_start().strip_prefix('"')?.chars();
    let mut id = String::new();
    loop {
        match chars.next()? {
            '"' => return Some(id),
            '\\' => id.push(chars.next()?),
            c => id.push(c),
        }
    }
}

/// Parses one line into a job, or the error response to send back.
fn parse_line(raw: &[u8]) -> Result<WorkerJob, Box<WorkerResponse>> {
    let text = match std::str::from_utf8(raw) {
        Ok(t) => t.trim(),
        Err(_) => {
            let lossy = String::from_utf8_lossy(raw);
            return Err(Box::new(WorkerResponse::error(
                recover_id(&lossy),
                "Invalid job line: not valid UTF-8",
            )));
        }
    };
    serde_json::from_str(text).map_err(|e| {
        Box::new(WorkerResponse::error(
            recover_id(text),
            format!("Invalid job JSON: {e}"),
        ))
    })
}

/// Runs a job on its own thread so that a panic or a hang cannot take the worker down.
fn run_guarded(job: WorkerJob, renderer: &FontRenderer, timeout: Duration) -> WorkerResponse {
    let id = job.id.clone();
    let abandoned = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let (renderer, flag) = (renderer.clone(), Arc::clone(&abandoned));
    std::thread::spawn(move || {
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            run_page_job(
                job.id.clone(),
                &renderer,
                &job.image_path,
                &job.regions,
                &job.output_path,
                &flag,
            )
        }));
        let _ = tx.send(outcome);
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok(resp)) => resp,
        Ok(Err(_)) => WorkerResponse::error(id, "internal error: job panicked"),
        Err(_) => {
            abandoned.store(true, Ordering::Release);
            WorkerResponse::error(id, format!("job timed out after {}s", timeout.as_secs()))
        }
    }
}

pub fn run_worker(
    renderer: &FontRenderer,
    timeout: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = BufReader::new(std::io::stdin());
    let mut out = std::io::stdout().lock();
    let mut buf = Vec::new();
    loop {
        buf.clear();
        if reader.read_until(b'\n', &mut buf)? == 0 {
            return Ok(());
        }
        if buf.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let resp = match parse_line(&buf) {
            Ok(job) => run_guarded(job, renderer, timeout),
            Err(resp) => *resp,
        };
        writeln!(out, "{}", serde_json::to_string(&resp)?)?;
        out.flush()?;
    }
}

#[cfg(test)]
mod tests {
    use super::recover_id;

    #[test]
    fn recovers_id_from_partial_json() {
        assert_eq!(
            recover_id(r#"{"id": "abc", "image_path":"#).as_deref(),
            Some("abc")
        );
        assert_eq!(recover_id(r#"{ "id" :"a\"b" "#).as_deref(), Some("a\"b"));
        assert_eq!(recover_id(r#"{"id": 5}"#), None);
        assert_eq!(recover_id(r#"{"id": "unterminated"#), None);
        assert_eq!(recover_id("garbage"), None);
    }
}
