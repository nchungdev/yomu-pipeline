//! Short per-call retry loop (ADR-0002 section 4). The pipeline only retries briefly; long
//! waits and quota pauses belong to the app. JSON-level retries are NOT done here.

use super::classify::classify_response;
use super::transport::{HttpRequest, HttpResponse, HttpTransport, Sleeper, TransportError};
use crate::contract::v1::{CancelToken, ProviderError, ProviderKind};
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CallError {
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error("cancelled")]
    Cancelled,
}

#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    /// Upper bound of the SUM of all scheduled sleeps in one call.
    pub total_budget: Duration,
    pub base_backoff: Duration,
    /// A rate-limit hint longer than this is returned to the caller instead of waited for.
    pub max_rate_limit_wait: Duration,
    /// Extra delay added to each backoff, from the 0-based retry index.
    pub jitter: fn(u32) -> Duration,
}

/// Pseudo-random 0..250 ms from the clock; good enough to de-synchronize retries.
fn clock_jitter(_retry: u32) -> Duration {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    Duration::from_millis(u64::from(nanos % 250))
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            total_budget: Duration::from_secs(30),
            base_backoff: Duration::from_millis(800),
            max_rate_limit_wait: Duration::from_secs(10),
            jitter: clock_jitter,
        }
    }
}

impl RetryPolicy {
    /// Wait before retry number `retry` (0-based) after `err`, or `None` to give up.
    fn wait_for(&self, err: &ProviderError, retry: u32, slept: Duration) -> Option<Duration> {
        let wait = match err {
            ProviderError::Server(_) | ProviderError::Network | ProviderError::Timeout => {
                let factor = 1u32.checked_shl(retry).unwrap_or(u32::MAX);
                self.base_backoff
                    .saturating_mul(factor)
                    .saturating_add((self.jitter)(retry))
            }
            ProviderError::RateLimited { .. } => {
                let hint = err.retry_hint()?;
                if hint > self.max_rate_limit_wait {
                    return None;
                }
                hint
            }
            _ => return None,
        };
        (slept.saturating_add(wait) <= self.total_budget).then_some(wait)
    }
}

fn map_transport(e: TransportError) -> ProviderError {
    match e {
        TransportError::Timeout => ProviderError::Timeout,
        TransportError::Connect | TransportError::Other => ProviderError::Network,
        TransportError::BodyTooLarge => {
            ProviderError::InvalidResponse("response body too large".to_owned())
        }
    }
}

/// Sends `req`, retrying transient failures per `policy`. Returns the first usable response.
pub async fn call_with_retry(
    transport: &dyn HttpTransport,
    sleeper: &dyn Sleeper,
    kind: ProviderKind,
    req: &HttpRequest,
    policy: &RetryPolicy,
    cancel: &CancelToken,
    now: &dyn Fn() -> SystemTime,
) -> Result<HttpResponse, CallError> {
    let mut slept = Duration::ZERO;
    let mut attempt: u32 = 0;
    loop {
        if cancel.is_cancelled() {
            return Err(CallError::Cancelled);
        }
        let err = match transport.send(req).await {
            Ok(resp) => match classify_response(kind, &resp.view(), now()) {
                None => return Ok(resp),
                Some(e) => e,
            },
            Err(te) => map_transport(te),
        };
        attempt += 1;
        if attempt >= policy.max_attempts {
            return Err(err.into());
        }
        let Some(wait) = policy.wait_for(&err, attempt - 1, slept) else {
            return Err(err.into());
        };
        slept += wait;
        if !sleeper.sleep_unless_cancelled(wait, cancel).await {
            return Err(CallError::Cancelled);
        }
    }
}
