//! Remote client implementation of [`TranslationEngine`] calling a Yomu backend server.

use std::sync::Arc;
use std::time::Duration;

use crate::contract::v1::{
    CancelToken, EngineError, EventSink, JobHandle, PageInput, PipelineEvent, RegionResult,
    TranslateBatchRequest, TranslateOptions, TranslationEngine,
};
use crate::engine::cache_key::cache_key;
use crate::server::handler::RelayoutApiRequest;

/// [`TranslationEngine`] backed by a remote HTTP Yomu Pipeline Server.
pub struct RemoteEngine {
    endpoint: String,
    auth_token: Option<String>,
    client: reqwest::Client,
}

impl RemoteEngine {
    pub fn new(endpoint: impl Into<String>, auth_token: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(180))
            .build()
            .unwrap_or_default();

        Self {
            endpoint: endpoint.into().trim_end_matches('/').to_string(),
            auth_token,
            client,
        }
    }
}

impl TranslationEngine for RemoteEngine {
    fn submit(
        &self,
        request: TranslateBatchRequest,
        sink: Arc<dyn EventSink>,
        cancel: CancelToken,
    ) -> Result<JobHandle, EngineError> {
        let job_handle = JobHandle {
            request_id: request.request_id.clone(),
        };

        let url = format!("{}/v1/translate", self.endpoint);
        let client = self.client.clone();
        let token = self.auth_token.clone();

        tokio::spawn(async move {
            if cancel.is_cancelled() {
                sink.emit(PipelineEvent::BatchFinished {
                    request_id: request.request_id,
                    ok_pages: 0,
                    failed_pages: request.pages.len() as u32,
                });
                return;
            }

            let mut req = client.post(&url).json(&request);
            if let Some(t) = &token {
                req = req.bearer_auth(t);
            }

            let resp = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    for p in &request.pages {
                        sink.emit(PipelineEvent::PageFailed {
                            page_id: p.page_id.clone(),
                            failure: crate::contract::v1::PageFailure::Internal(e.to_string()),
                        });
                    }
                    sink.emit(PipelineEvent::BatchFinished {
                        request_id: request.request_id,
                        ok_pages: 0,
                        failed_pages: request.pages.len() as u32,
                    });
                    return;
                }
            };

            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                for p in &request.pages {
                    sink.emit(PipelineEvent::PageFailed {
                        page_id: p.page_id.clone(),
                        failure: crate::contract::v1::PageFailure::Internal(format!(
                            "Server error {status}: {text}"
                        )),
                    });
                }
                sink.emit(PipelineEvent::BatchFinished {
                    request_id: request.request_id,
                    ok_pages: 0,
                    failed_pages: request.pages.len() as u32,
                });
                return;
            }

            match resp.json::<Vec<PipelineEvent>>().await {
                Ok(events) => {
                    for ev in events {
                        if cancel.is_cancelled() {
                            break;
                        }
                        sink.emit(ev);
                    }
                }
                Err(e) => {
                    for p in &request.pages {
                        sink.emit(PipelineEvent::PageFailed {
                            page_id: p.page_id.clone(),
                            failure: crate::contract::v1::PageFailure::Internal(format!(
                                "Malformed response from server: {e}"
                            )),
                        });
                    }
                    sink.emit(PipelineEvent::BatchFinished {
                        request_id: request.request_id,
                        ok_pages: 0,
                        failed_pages: request.pages.len() as u32,
                    });
                }
            }
        });

        Ok(job_handle)
    }

    fn relayout_region(
        &self,
        page: &PageInput,
        region_id: &str,
        new_text: &str,
    ) -> Result<RegionResult, EngineError> {
        let url = format!("{}/v1/relayout", self.endpoint);
        let req_body = RelayoutApiRequest {
            page: page.clone(),
            region_id: region_id.to_string(),
            new_text: new_text.to_string(),
        };

        let client = self.client.clone();
        let token = self.auth_token.clone();

        // Synchronous / blocking block or spawn on tokio
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let mut req = client.post(&url).json(&req_body);
                if let Some(t) = &token {
                    req = req.bearer_auth(t);
                }
                let resp = req
                    .send()
                    .await
                    .map_err(|e| EngineError::Internal(e.to_string()))?;
                if !resp.status().is_success() {
                    let text = resp.text().await.unwrap_or_default();
                    return Err(EngineError::Internal(text));
                }
                resp.json::<RegionResult>()
                    .await
                    .map_err(|e| EngineError::Internal(e.to_string()))
            })
        })
    }

    fn cache_key(&self, source_hash: &str, options: &TranslateOptions) -> String {
        cache_key(source_hash, options)
    }
}
