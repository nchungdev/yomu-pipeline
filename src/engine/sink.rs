use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use crate::contract::v1::{EventSink, PipelineEvent};

/// An [`EventSink`] that delegates to a thread-safe callback function.
pub struct CallbackEventSink<F> {
    callback: F,
}

impl<F> CallbackEventSink<F>
where
    F: Fn(PipelineEvent) + Send + Sync,
{
    pub fn new(callback: F) -> Self {
        Self { callback }
    }
}

impl<F> EventSink for CallbackEventSink<F>
where
    F: Fn(PipelineEvent) + Send + Sync,
{
    fn emit(&self, event: PipelineEvent) {
        (self.callback)(event);
    }
}

/// An [`EventSink`] backed by a standard library MPSC channel.
pub struct ChannelEventSink {
    sender: Mutex<Sender<PipelineEvent>>,
}

impl ChannelEventSink {
    pub fn new(sender: Sender<PipelineEvent>) -> Self {
        Self {
            sender: Mutex::new(sender),
        }
    }
}

impl EventSink for ChannelEventSink {
    fn emit(&self, event: PipelineEvent) {
        if let Ok(guard) = self.sender.lock() {
            let _ = guard.send(event);
        }
    }
}

/// Creates a linked pair of [`ChannelEventSink`] and [`Receiver<PipelineEvent>`].
pub fn event_channel() -> (Arc<ChannelEventSink>, Receiver<PipelineEvent>) {
    let (tx, rx) = channel();
    (Arc::new(ChannelEventSink::new(tx)), rx)
}
