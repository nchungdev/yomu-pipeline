//! Embedded (in-process) implementation of [`TranslationEngine`].

use std::sync::Arc;

use super::cache_key::cache_key;
use super::runner::load_page_image;
#[cfg(feature = "llm")]
use super::runner::run_batch;
#[cfg(not(feature = "llm"))]
use super::runner::run_batch_no_llm;

use crate::contract::v1::{
    CancelToken, EngineError, EventSink, JobHandle, PageInput, RegionResult, RenderMode,
    TranslateBatchRequest, TranslateOptions, TranslationEngine, SCHEMA_VERSION,
};
use crate::pipeline::PipelineOptions;
use crate::pipeline_result::process_page_result;
use crate::render::FontRenderer;
use crate::types::{BubbleStyleHint, RegionInput};

#[cfg(feature = "llm")]
use crate::llm::{
    transport::{HttpTransport, Sleeper, TokioSleeper},
    transport_reqwest::ReqwestTransport,
};

/// An in-process translation engine wrapping image processing, layout, and LLM calls.
#[derive(Clone)]
pub struct EmbeddedEngine {
    renderer: Arc<FontRenderer>,
    options: PipelineOptions,
    #[cfg(feature = "llm")]
    transport: Arc<dyn HttpTransport>,
    #[cfg(feature = "llm")]
    sleeper: Arc<dyn Sleeper>,
}

impl EmbeddedEngine {
    /// Creates an embedded engine with default bundled font and pipeline options.
    pub fn new() -> Result<Self, EngineError> {
        let renderer = Arc::new(FontRenderer::default());
        let options = PipelineOptions::default();
        #[cfg(feature = "llm")]
        {
            let transport =
                Arc::new(ReqwestTransport::new().map_err(|e| {
                    EngineError::Internal(format!("failed to init transport: {e}"))
                })?);
            let sleeper = Arc::new(TokioSleeper);
            Ok(Self {
                renderer,
                options,
                transport,
                sleeper,
            })
        }
        #[cfg(not(feature = "llm"))]
        {
            Ok(Self { renderer, options })
        }
    }

    /// Creates an engine with a custom renderer and pipeline options.
    pub fn with_options(renderer: FontRenderer, options: PipelineOptions) -> Self {
        let renderer = Arc::new(renderer);
        #[cfg(feature = "llm")]
        {
            let transport = Arc::new(ReqwestTransport::new().unwrap_or_else(|_| {
                panic!("ReqwestTransport creation should not fail in standard environment")
            }));
            let sleeper = Arc::new(TokioSleeper);
            Self {
                renderer,
                options,
                transport,
                sleeper,
            }
        }
        #[cfg(not(feature = "llm"))]
        {
            Self { renderer, options }
        }
    }

    #[cfg(feature = "llm")]
    /// Creates an engine with injected transport and sleeper (for testing or mocking).
    pub fn with_transport(
        renderer: FontRenderer,
        options: PipelineOptions,
        transport: Arc<dyn HttpTransport>,
        sleeper: Arc<dyn Sleeper>,
    ) -> Self {
        Self {
            renderer: Arc::new(renderer),
            options,
            transport,
            sleeper,
        }
    }
}

impl Default for EmbeddedEngine {
    fn default() -> Self {
        Self::new().unwrap_or_else(|e| panic!("Failed to initialize EmbeddedEngine default: {e}"))
    }
}

impl TranslationEngine for EmbeddedEngine {
    fn submit(
        &self,
        request: TranslateBatchRequest,
        sink: Arc<dyn EventSink>,
        cancel: CancelToken,
    ) -> Result<JobHandle, EngineError> {
        if request.schema_version != SCHEMA_VERSION {
            return Err(EngineError::InvalidRequest(format!(
                "schema version mismatch: {} != {SCHEMA_VERSION}",
                request.schema_version
            )));
        }
        if request.pages.len() as u32 > request.options.limits.max_pages {
            return Err(EngineError::InvalidRequest(format!(
                "batch size {} exceeds limit {}",
                request.pages.len(),
                request.options.limits.max_pages
            )));
        }
        if cancel.is_cancelled() {
            return Err(EngineError::Cancelled);
        }

        let handle = JobHandle {
            request_id: request.request_id.clone(),
        };

        let renderer = Arc::clone(&self.renderer);
        let options = self.options;

        #[cfg(feature = "llm")]
        {
            let transport = Arc::clone(&self.transport);
            let sleeper = Arc::clone(&self.sleeper);
            let runner = async move {
                run_batch(request, sink, cancel, renderer, options, transport, sleeper).await;
            };

            if let Ok(rt) = tokio::runtime::Handle::try_current() {
                rt.spawn(runner);
            } else {
                let thread_name = format!("pipeline-worker-{}", handle.request_id);
                std::thread::Builder::new()
                    .name(thread_name)
                    .spawn(move || {
                        if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                        {
                            rt.block_on(runner);
                        }
                    })
                    .map_err(|e| EngineError::Internal(e.to_string()))?;
            }
        }

        #[cfg(not(feature = "llm"))]
        {
            let thread_name = format!("pipeline-worker-{}", handle.request_id);
            std::thread::Builder::new()
                .name(thread_name)
                .spawn(move || {
                    run_batch_no_llm(request, sink, cancel, renderer, options);
                })
                .map_err(|e| EngineError::Internal(e.to_string()))?;
        }

        Ok(handle)
    }

    fn relayout_region(
        &self,
        page: &PageInput,
        region_id: &str,
        translated_text: &str,
    ) -> Result<RegionResult, EngineError> {
        let (bytes, img) = load_page_image(page).map_err(EngineError::InvalidRequest)?;

        let prior_region = page
            .prior
            .as_ref()
            .and_then(|p| p.regions.iter().find(|r| r.region_id == region_id))
            .ok_or_else(|| {
                EngineError::InvalidRequest(format!("region {region_id} not found in page prior"))
            })?;

        let input = RegionInput {
            id: region_id.to_string(),
            bounds: prior_region.bounds,
            text: translated_text.to_string(),
            style_hint: BubbleStyleHint::Balloon,
            kind: prior_region.kind,
            typography: Some(prior_region.typography),
        };

        let outcome = process_page_result(
            &page.page_id,
            &img,
            Some(&bytes),
            &[input],
            &self.renderer,
            self.options,
            RenderMode::Overlay,
        );

        let mut result =
            outcome.result.regions.into_iter().next().ok_or_else(|| {
                EngineError::Internal("failed to generate region result".to_string())
            })?;

        result.source_text = prior_region.source_text.clone();
        result.speaker = prior_region.speaker.clone();
        result.emotion = prior_region.emotion.clone();
        result.intensity = prior_region.intensity;
        result.region_id = region_id.to_string();

        Ok(result)
    }

    fn cache_key(&self, source_hash: &str, options: &TranslateOptions) -> String {
        cache_key(source_hash, options)
    }
}
