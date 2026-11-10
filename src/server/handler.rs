//! In-memory, zero-retention HTTP request handler for Yomu Pipeline Server (ADR-0002 §6).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::contract::v1::{
    CancelToken, EngineError, ImageSource, PageInput, PipelineEvent, RegionResult,
    TranslateBatchRequest, TranslationEngine,
};
use crate::engine::sink::event_channel;

/// Configuration for the pipeline server.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub auth_tokens: HashSet<String>,
    pub max_payload_bytes: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            auth_tokens: HashSet::new(),
            max_payload_bytes: 50 * 1024 * 1024, // 50 MiB
        }
    }
}

/// Request body for region relayout over HTTP.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayoutApiRequest {
    pub page: PageInput,
    pub region_id: String,
    pub new_text: String,
}

/// Response status for HTTP endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiStatus {
    Ok,
    Unauthorized,
    PayloadTooLarge,
    BadRequest,
    InternalError,
}

/// Dispatches requests against a backing [`TranslationEngine`] with zero disk storage.
pub struct ServerHandler {
    config: ServerConfig,
    engine: Arc<dyn TranslationEngine>,
}

impl ServerHandler {
    pub fn new(config: ServerConfig, engine: Arc<dyn TranslationEngine>) -> Self {
        Self { config, engine }
    }

    /// Verifies that the provided Bearer token is valid.
    pub fn authenticate(&self, auth_header: Option<&str>) -> bool {
        if self.config.auth_tokens.is_empty() {
            // Open server mode (e.g. local loopback daemon)
            return true;
        }
        let Some(header) = auth_header else {
            return false;
        };
        let token = header
            .strip_prefix("Bearer ")
            .map(str::trim)
            .unwrap_or(header.trim());
        self.config.auth_tokens.contains(token)
    }

    /// Handles a batch translation request, collecting events in memory with zero disk retention.
    pub fn handle_translate(
        &self,
        auth_header: Option<&str>,
        payload_bytes: &[u8],
    ) -> Result<Vec<PipelineEvent>, (ApiStatus, String)> {
        let start = Instant::now();

        if !self.authenticate(auth_header) {
            return Err((
                ApiStatus::Unauthorized,
                "Unauthorized: invalid or missing Bearer token".into(),
            ));
        }

        if payload_bytes.len() > self.config.max_payload_bytes {
            return Err((
                ApiStatus::PayloadTooLarge,
                format!(
                    "Payload size {} exceeds limit {}",
                    payload_bytes.len(),
                    self.config.max_payload_bytes
                ),
            ));
        }

        let request: TranslateBatchRequest =
            serde_json::from_slice(payload_bytes).map_err(|e| {
                (
                    ApiStatus::BadRequest,
                    format!("Malformed JSON request: {e}"),
                )
            })?;

        // Zero-retention & security check: remote clients MUST upload bytes, never local file paths.
        for page in &request.pages {
            if matches!(page.image, ImageSource::Path(_)) {
                return Err((
                    ApiStatus::BadRequest,
                    format!("Page '{}': server mode requires ImageSource::Bytes, local file paths are forbidden", page.page_id),
                ));
            }
        }

        let request_id = request.request_id.clone();
        let page_count = request.pages.len();

        let (sink, rx) = event_channel();
        let cancel = CancelToken::default();

        self.engine.submit(request, sink, cancel).map_err(|e| {
            (
                ApiStatus::InternalError,
                format!("Engine submit failure: {e}"),
            )
        })?;

        let mut events = Vec::new();
        while let Ok(event) = rx.recv() {
            let is_finished = matches!(event, PipelineEvent::BatchFinished { .. });
            events.push(event);
            if is_finished {
                break;
            }
        }

        // Sanitized metadata log only (ADR-0002 §6) - no image bytes, no prompt text, no tokens.
        eprintln!(
            "[yomu-server] request_id={} pages={} elapsed_ms={} status=ok",
            request_id,
            page_count,
            start.elapsed().as_millis()
        );

        Ok(events)
    }

    /// Handles a relayout request for a single region.
    pub fn handle_relayout(
        &self,
        auth_header: Option<&str>,
        payload_bytes: &[u8],
    ) -> Result<RegionResult, (ApiStatus, String)> {
        if !self.authenticate(auth_header) {
            return Err((
                ApiStatus::Unauthorized,
                "Unauthorized: invalid Bearer token".into(),
            ));
        }

        let req: RelayoutApiRequest = serde_json::from_slice(payload_bytes)
            .map_err(|e| (ApiStatus::BadRequest, format!("Malformed JSON: {e}")))?;

        self.engine
            .relayout_region(&req.page, &req.region_id, &req.new_text)
            .map_err(|e| match e {
                EngineError::InvalidRequest(msg) => (ApiStatus::BadRequest, msg),
                other => (ApiStatus::InternalError, other.to_string()),
            })
    }
}
