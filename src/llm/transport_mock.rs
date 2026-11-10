//! Test doubles: scripted transport that records requests, and sleepers that never sleep.

use super::transport::{
    BoxFuture, HttpRequest, HttpResponse, HttpTransport, Sleeper, TransportError,
};
use crate::contract::v1::CancelToken;
use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Replays scripted results in order; an exhausted script yields `TransportError::Other`.
#[derive(Debug, Default)]
pub struct MockTransport {
    script: Mutex<VecDeque<Result<HttpResponse, TransportError>>>,
    seen: Mutex<Vec<HttpRequest>>,
}

impl MockTransport {
    pub fn new(script: impl IntoIterator<Item = Result<HttpResponse, TransportError>>) -> Self {
        Self {
            script: Mutex::new(script.into_iter().collect()),
            seen: Mutex::default(),
        }
    }

    pub fn call_count(&self) -> usize {
        lock(&self.seen).len()
    }

    pub fn requests(&self) -> Vec<HttpRequest> {
        lock(&self.seen).clone()
    }
}

impl HttpTransport for MockTransport {
    fn send<'a>(
        &'a self,
        req: &'a HttpRequest,
    ) -> BoxFuture<'a, Result<HttpResponse, TransportError>> {
        lock(&self.seen).push(req.clone());
        let next = lock(&self.script)
            .pop_front()
            .unwrap_or(Err(TransportError::Other));
        Box::pin(async move { next })
    }
}

/// Records every requested sleep instead of sleeping. Optionally cancels `token` as soon as a
/// sleep is requested (simulates a cancel arriving during a backoff).
#[derive(Debug, Default)]
pub struct RecordingSleeper {
    sleeps: Mutex<Vec<Duration>>,
    cancel_on_sleep: Option<CancelToken>,
}

impl RecordingSleeper {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancelling(token: CancelToken) -> Self {
        Self {
            cancel_on_sleep: Some(token),
            ..Self::default()
        }
    }

    pub fn sleeps(&self) -> Vec<Duration> {
        lock(&self.sleeps).clone()
    }

    pub fn total(&self) -> Duration {
        lock(&self.sleeps).iter().sum()
    }

    fn note(&self, d: Duration) {
        lock(&self.sleeps).push(d);
        if let Some(t) = &self.cancel_on_sleep {
            t.cancel();
        }
    }
}

impl Sleeper for RecordingSleeper {
    fn sleep(&self, d: Duration) -> BoxFuture<'_, ()> {
        self.note(d);
        Box::pin(async {})
    }

    fn sleep_unless_cancelled<'a>(
        &'a self,
        d: Duration,
        cancel: &'a CancelToken,
    ) -> BoxFuture<'a, bool> {
        self.note(d);
        let done = !cancel.is_cancelled();
        Box::pin(async move { done })
    }
}

/// Never sleeps, records nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoSleep;

impl Sleeper for NoSleep {
    fn sleep(&self, _d: Duration) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

/// Deterministic jitter: 100 ms regardless of attempt.
pub fn fixed_jitter(_attempt: u32) -> Duration {
    Duration::from_millis(100)
}
