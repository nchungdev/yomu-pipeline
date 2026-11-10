//! `reqwest`-backed [`HttpTransport`]. One shared client (rustls), per-request timeout, no
//! retries, redirects only within the same host. Bodies are capped while streaming. Nothing
//! here logs URLs, headers or bodies.

use super::transport::{
    BoxFuture, HttpRequest, HttpResponse, HttpTransport, Method, TransportError,
};
use reqwest::redirect::Policy;

pub const MAX_SUCCESS_BODY: usize = 2 * 1024 * 1024;
pub const MAX_ERROR_BODY: usize = 64 * 1024;
const MAX_REDIRECTS: usize = 3;

#[derive(Debug, Clone)]
pub struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    pub fn new() -> Result<Self, TransportError> {
        let policy = Policy::custom(|attempt| {
            let same_host = attempt
                .previous()
                .last()
                .is_some_and(|prev| prev.host_str() == attempt.url().host_str());
            if attempt.previous().len() >= MAX_REDIRECTS || !same_host {
                attempt.stop()
            } else {
                attempt.follow()
            }
        });
        let client = reqwest::Client::builder()
            .redirect(policy)
            .build()
            .map_err(|_| TransportError::Other)?;
        Ok(Self { client })
    }

    async fn run(&self, req: &HttpRequest) -> Result<HttpResponse, TransportError> {
        let method = match req.method {
            Method::Get => reqwest::Method::GET,
            Method::Post => reqwest::Method::POST,
        };
        let mut builder = self
            .client
            .request(method, req.url.as_str())
            .timeout(req.timeout);
        for h in &req.headers {
            builder = builder.header(h.name.as_str(), h.value.as_str());
        }
        if !req.body.is_empty() {
            builder = builder.body(req.body.clone());
        }
        let mut resp = builder.send().await.map_err(map_error)?;
        let status = resp.status().as_u16();
        let headers = resp
            .headers()
            .iter()
            .filter_map(|(k, v)| Some((k.as_str().to_owned(), v.to_str().ok()?.to_owned())))
            .collect();
        let success = resp.status().is_success();
        let cap = if success {
            MAX_SUCCESS_BODY
        } else {
            MAX_ERROR_BODY
        };
        let mut body: Vec<u8> = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(map_error)? {
            let room = cap - body.len();
            if chunk.len() > room {
                if success {
                    return Err(TransportError::BodyTooLarge);
                }
                body.extend_from_slice(&chunk[..room]);
                break;
            }
            body.extend_from_slice(&chunk);
        }
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}

fn map_error(e: reqwest::Error) -> TransportError {
    if e.is_timeout() {
        TransportError::Timeout
    } else if e.is_connect() {
        TransportError::Connect
    } else {
        TransportError::Other
    }
}

impl HttpTransport for ReqwestTransport {
    fn send<'a>(
        &'a self,
        req: &'a HttpRequest,
    ) -> BoxFuture<'a, Result<HttpResponse, TransportError>> {
        Box::pin(self.run(req))
    }
}
