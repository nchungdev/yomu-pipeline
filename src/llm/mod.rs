//! Provider HTTP layer (ADR-0002 section 4): typed classification of provider responses,
//! retry-hint parsing, the transport abstraction and the short per-call retry loop. Only
//! `transport_reqwest` performs I/O.

pub mod call;
pub mod classify;
pub mod image_prep;
pub mod parse;
pub mod prompt;
pub mod reading_order;
pub mod region;
pub mod retry_hint;
mod sanitize;
pub mod schemas;
pub mod tiling;
pub mod transport;
pub mod transport_reqwest;

#[cfg(any(test, feature = "test-util"))]
pub mod transport_mock;

pub use call::{call_with_retry, CallError, RetryPolicy};
pub use classify::{classify_response, ResponseView};
pub use image_prep::{
    detect_image_mime, is_mostly_grayscale, prepare_page, PrepError, PrepOptions, PreparedPage,
    PreparedTile,
};
pub use parse::{extract_json_payload, parse_vision_regions, ParseError};
pub use prompt::{build_prompt, PromptInput};
pub use reading_order::sort_regions_by_reading_order;
pub use region::{LlmPage, LlmRegion};
pub use tiling::{dedupe_overlapping, remap_tile_rect};
pub use transport::{
    BoxFuture, Header, HttpRequest, HttpResponse, HttpTransport, Method, Sleeper, TokioSleeper,
    TransportError,
};
pub use transport_reqwest::ReqwestTransport;
