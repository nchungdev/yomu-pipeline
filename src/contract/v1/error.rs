use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Typed failure of one LLM provider call (replaces string matching on error messages).
///
/// Messages carried by variants must never contain URLs, headers, API keys, image data or user
/// text: the value is logged and crosses the contract as-is.
///
/// Retry hints (`retry_after_ms`, `reset_hint_ms`) are reported verbatim and are NOT clamped; a
/// hint can be hours. Only the decision whether to sleep is bounded elsewhere.
///
/// Wire form: externally tagged, snake_case. Legacy payloads still deserialize: a plain number
/// for `rate_limited` / `quota_exhausted` (the hint in ms), the bare string `"auth"` (status
/// defaults to 401) and objects without the newer optional fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case", try_from = "wire::Wire")]
pub enum ProviderError {
    #[error("rate limited")]
    RateLimited {
        #[serde(skip_serializing_if = "Option::is_none")]
        retry_after_ms: Option<u64>,
    },
    #[error("quota exhausted")]
    QuotaExhausted {
        #[serde(skip_serializing_if = "Option::is_none")]
        reset_hint_ms: Option<u64>,
    },
    #[error("authentication failed (HTTP {status})")]
    Auth { status: u16 },
    /// Safety or content filter rejected the request or response.
    #[error("content blocked: {reason}")]
    ContentBlocked { reason: String },
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("provider server error (HTTP {0})")]
    Server(u16),
    #[error("network error")]
    Network,
    #[error("request timed out")]
    Timeout,
    #[error("invalid provider response: {0}")]
    InvalidResponse(String),
}

impl ProviderError {
    /// Provider-suggested wait before retrying, unclamped. `None` when the provider gave no hint.
    pub fn retry_hint(&self) -> Option<Duration> {
        match self {
            Self::RateLimited { retry_after_ms } => *retry_after_ms,
            Self::QuotaExhausted { reset_hint_ms } => *reset_hint_ms,
            _ => None,
        }
        .map(Duration::from_millis)
    }

    /// Stable snake_case name of the variant (matches the wire tag), safe to log.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::RateLimited { .. } => "rate_limited",
            Self::QuotaExhausted { .. } => "quota_exhausted",
            Self::Auth { .. } => "auth",
            Self::ContentBlocked { .. } => "content_blocked",
            Self::BadRequest(_) => "bad_request",
            Self::Server(_) => "server",
            Self::Network => "network",
            Self::Timeout => "timeout",
            Self::InvalidResponse(_) => "invalid_response",
        }
    }
}

mod wire {
    use super::ProviderError;
    use serde::Deserialize;

    /// Hint given either as a plain number (legacy) or as an object.
    #[derive(Deserialize)]
    #[serde(untagged)]
    pub enum RateRepr {
        Ms(u64),
        Obj {
            #[serde(default)]
            retry_after_ms: Option<u64>,
        },
    }

    #[derive(Deserialize)]
    #[serde(untagged)]
    pub enum QuotaRepr {
        Ms(u64),
        Obj {
            #[serde(default)]
            reset_hint_ms: Option<u64>,
        },
    }

    #[derive(Deserialize)]
    #[serde(untagged)]
    pub enum AuthRepr {
        Status(u16),
        Obj {
            #[serde(default = "default_status")]
            status: u16,
        },
        Unit(()),
    }

    fn default_status() -> u16 {
        401
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum Tagged {
        RateLimited(RateRepr),
        QuotaExhausted(QuotaRepr),
        Auth(AuthRepr),
        ContentBlocked { reason: String },
        BadRequest(String),
        Server(u16),
        InvalidResponse(String),
    }

    #[derive(Deserialize)]
    #[serde(untagged)]
    pub enum Wire {
        /// Unit form: `"network"`, `"timeout"`, legacy `"auth"`.
        Name(String),
        Tagged(Tagged),
    }

    impl TryFrom<Wire> for ProviderError {
        type Error = String;

        fn try_from(w: Wire) -> Result<Self, String> {
            Ok(match w {
                Wire::Name(n) => match n.as_str() {
                    "network" => Self::Network,
                    "timeout" => Self::Timeout,
                    "auth" => Self::Auth {
                        status: default_status(),
                    },
                    "rate_limited" => Self::RateLimited {
                        retry_after_ms: None,
                    },
                    "quota_exhausted" => Self::QuotaExhausted {
                        reset_hint_ms: None,
                    },
                    other => return Err(format!("unknown provider error `{other}`")),
                },
                Wire::Tagged(t) => match t {
                    Tagged::RateLimited(RateRepr::Ms(ms)) => Self::RateLimited {
                        retry_after_ms: Some(ms),
                    },
                    Tagged::RateLimited(RateRepr::Obj { retry_after_ms }) => {
                        Self::RateLimited { retry_after_ms }
                    }
                    Tagged::QuotaExhausted(QuotaRepr::Ms(ms)) => Self::QuotaExhausted {
                        reset_hint_ms: Some(ms),
                    },
                    Tagged::QuotaExhausted(QuotaRepr::Obj { reset_hint_ms }) => {
                        Self::QuotaExhausted { reset_hint_ms }
                    }
                    Tagged::Auth(AuthRepr::Status(status))
                    | Tagged::Auth(AuthRepr::Obj { status }) => Self::Auth { status },
                    Tagged::Auth(AuthRepr::Unit(())) => Self::Auth {
                        status: default_status(),
                    },
                    Tagged::ContentBlocked { reason } => Self::ContentBlocked { reason },
                    Tagged::BadRequest(m) => Self::BadRequest(m),
                    Tagged::Server(c) => Self::Server(c),
                    Tagged::InvalidResponse(m) => Self::InvalidResponse(m),
                },
            })
        }
    }
}

/// Violations of the structural invariants of the contract.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContractError {
    #[error("unsupported schema version {found}, expected {expected}")]
    SchemaVersion { found: u32, expected: u32 },
    #[error("region `{0}` is not rendered but still carries an erase spec")]
    EraseWithoutRender(String),
    #[error("region `{0}` is rendered but has no layout")]
    RenderedWithoutLayout(String),
    #[error("region `{0}` has invalid normalized bounds")]
    InvalidBounds(String),
}
