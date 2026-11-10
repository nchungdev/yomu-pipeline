//! Translation engine implementations (embedded and network).

pub mod cache_key;
pub mod embedded;
#[cfg(feature = "llm")]
pub mod remote;
pub(crate) mod runner;
pub mod sink;

pub use cache_key::{cache_key, glossary_hash};
pub use embedded::EmbeddedEngine;
#[cfg(feature = "llm")]
pub use remote::RemoteEngine;
pub use sink::{event_channel, CallbackEventSink, ChannelEventSink};
