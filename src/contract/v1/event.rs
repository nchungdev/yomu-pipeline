use serde::{Deserialize, Serialize};

use super::error::ProviderError;
use super::result::PageResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageStage {
    Decode,
    Recognize,
    Refine,
    Layout,
    Erase,
}

/// Why a whole page failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageFailure {
    Provider(ProviderError),
    Decode(String),
    Cancelled,
    Internal(String),
}

/// Progress events emitted while a batch runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum PipelineEvent {
    BatchStarted {
        request_id: String,
        total_pages: u32,
    },
    PageStage {
        page_id: String,
        stage: PageStage,
    },
    Throttled {
        retry_after_ms: u64,
    },
    PageDone(PageResult),
    PageFailed {
        page_id: String,
        failure: PageFailure,
    },
    BatchFinished {
        request_id: String,
        ok_pages: u32,
        failed_pages: u32,
    },
}
