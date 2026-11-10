//! Transport abstraction: plain request/response data, the `HttpTransport` and `Sleeper`
//! traits. No I/O lives here. Request `Debug`/`Display` never print secrets, query strings or
//! the body, and `TransportError` carries no payload (so it can never leak a URL).

use super::classify::ResponseView;
use crate::contract::v1::CancelToken;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Header {
    pub name: String,
    pub value: String,
    pub sensitive: bool,
}

impl Header {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            sensitive: false,
        }
    }

    /// Header whose value is a secret (API key, bearer token): redacted in `Debug`.
    pub fn secret(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            sensitive: true,
            ..Self::new(name, value)
        }
    }
}

impl fmt::Debug for Header {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = if self.sensitive {
            "<redacted>"
        } else {
            &self.value
        };
        write!(f, "{}: {value}", self.name)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
    pub timeout: Duration,
}

/// URL without userinfo, query and fragment (those can carry keys).
fn safe_url(url: &str) -> String {
    let no_tail = url.split(['?', '#']).next().unwrap_or("");
    match no_tail.split_once("://") {
        Some((scheme, rest)) => {
            let (auth, path) = rest
                .split_once('/')
                .map_or((rest, None), |(a, p)| (a, Some(p)));
            let host = auth.rsplit('@').next().unwrap_or("");
            format!(
                "{scheme}://{host}{}",
                path.map_or(String::new(), |p| format!("/{p}"))
            )
        }
        None => "<invalid url>".to_owned(),
    }
}

impl fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &safe_url(&self.url))
            .field("headers", &self.headers)
            .field("body_len", &self.body.len())
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl fmt::Display for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} ({} bytes)",
            self.method.as_str(),
            safe_url(&self.url),
            self.body.len()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn view(&self) -> ResponseView<'_> {
        ResponseView {
            status: self.status,
            headers: &self.headers,
            body: &self.body,
        }
    }
}

/// Payload-free on purpose: nothing here can contain a URL, header or body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    #[error("request timed out")]
    Timeout,
    #[error("connection failed")]
    Connect,
    #[error("response body too large")]
    BodyTooLarge,
    #[error("transport error")]
    Other,
}

pub trait HttpTransport: Send + Sync {
    fn send<'a>(
        &'a self,
        req: &'a HttpRequest,
    ) -> BoxFuture<'a, Result<HttpResponse, TransportError>>;
}

const CANCEL_SLICE: Duration = Duration::from_millis(100);

pub trait Sleeper: Send + Sync {
    fn sleep(&self, d: Duration) -> BoxFuture<'_, ()>;

    /// Sleeps `d` in small slices, checking `cancel` between them. `true` = slept the full
    /// time, `false` = interrupted by cancellation.
    fn sleep_unless_cancelled<'a>(
        &'a self,
        d: Duration,
        cancel: &'a CancelToken,
    ) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            let mut left = d;
            while !left.is_zero() {
                if cancel.is_cancelled() {
                    return false;
                }
                let slice = left.min(CANCEL_SLICE);
                self.sleep(slice).await;
                left -= slice;
            }
            !cancel.is_cancelled()
        })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TokioSleeper;

impl Sleeper for TokioSleeper {
    fn sleep(&self, d: Duration) -> BoxFuture<'_, ()> {
        Box::pin(tokio::time::sleep(d))
    }
}
