use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::error::ProviderError;
use super::event::PipelineEvent;
use super::region::RegionResult;
use super::request::{PageInput, TranslateBatchRequest, TranslateOptions};

/// Receives events from a running job.
pub trait EventSink: Send + Sync {
    fn emit(&self, event: PipelineEvent);
}

/// Cooperative cancellation flag shared between caller and engine.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Handle of a submitted batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobHandle {
    pub request_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("cancelled")]
    Cancelled,
    #[error("internal error: {0}")]
    Internal(String),
}

/// One contract, two transports: implemented by the embedded engine and by the HTTP client.
/// Declared only; no implementation exists yet.
pub trait TranslationEngine: Send + Sync {
    /// Starts a batch; results are delivered through `sink`.
    fn submit(
        &self,
        request: TranslateBatchRequest,
        sink: Arc<dyn EventSink>,
        cancel: CancelToken,
    ) -> Result<JobHandle, EngineError>;

    /// Re-runs layout (and erase) of one region with new translated text.
    fn relayout_region(
        &self,
        page: &PageInput,
        region_id: &str,
        translated_text: &str,
    ) -> Result<RegionResult, EngineError>;

    /// Cache key of a page under the given options.
    fn cache_key(&self, source_hash: &str, options: &TranslateOptions) -> String;
}
