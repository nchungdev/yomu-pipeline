//! `contract::v1`: serde types shared by the embedded and the HTTP transports (ADR-0002 section 7).
//!
//! Conventions: region coordinates are normalized to the page (0..1); `*_px` values are in pixels
//! of the original (undecoded-to-smaller) source image.

mod apply;
mod engine;
mod error;
mod event;
mod layout;
mod region;
mod request;
mod result;

pub use apply::{apply_erase, ApplyWarning};
pub use engine::{CancelToken, EngineError, EventSink, JobHandle, TranslationEngine};
pub use error::{ContractError, ProviderError};
pub use event::{PageFailure, PageStage, PipelineEvent};
pub use layout::{LayoutLine, PxPoint, TextLayoutV1};
pub use region::{
    EraseSpec, ImageBlob, ImageFormat, PatchFormat, RegionFailure, RegionKind, RegionResult,
    RegionStatus, SkipReason,
};
pub use request::{
    ContextLine, Credentials, GlossaryCategory, GlossaryEntry, ImageSource, Limits, PageInput,
    PriorPage, ProviderKind, ProviderSpec, ReadingDirection, RenderMode, TranslateBatchRequest,
    TranslateOptions,
};
pub use result::{PagePx, PageResult};

/// Schema version carried by every request and result.
pub const SCHEMA_VERSION: u32 = 1;
