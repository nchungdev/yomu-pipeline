//! Batch processing worker for [`EmbeddedEngine`].

use std::sync::Arc;

use image::RgbaImage;

use super::cache_key::cache_key;
use crate::contract::v1::{
    CancelToken, EventSink, ImageSource, PageFailure, PageInput, PagePx, PageResult, PipelineEvent,
    TranslateBatchRequest, SCHEMA_VERSION,
};
use crate::pipeline::PipelineOptions;
use crate::render::FontRenderer;

#[cfg(feature = "llm")]
use crate::contract::v1::PageStage;
#[cfg(feature = "llm")]
use crate::llm::{
    call::CallError,
    chain::{translate_page, LlmPageRequest},
    image_prep::PrepOptions,
    provider::CallEnv,
    transport::{HttpTransport, Sleeper},
};
#[cfg(feature = "llm")]
use crate::pipeline_result::process_page_result;
#[cfg(feature = "llm")]
use crate::types::{BubbleStyleHint, RegionInput};

pub(crate) fn load_page_image(page: &PageInput) -> Result<(Vec<u8>, RgbaImage), String> {
    let bytes = match &page.image {
        ImageSource::Bytes(b) => b.clone(),
        ImageSource::Path(p) => {
            std::fs::read(p).map_err(|e| format!("failed to read image file {p:?}: {e}"))?
        }
    };
    let img = image::load_from_memory(&bytes)
        .map_err(|e| format!("failed to decode image: {e}"))?
        .into_rgba8();
    Ok((bytes, img))
}

#[cfg(feature = "llm")]
pub(crate) async fn run_batch(
    request: TranslateBatchRequest,
    sink: Arc<dyn EventSink>,
    cancel: CancelToken,
    renderer: Arc<FontRenderer>,
    options: PipelineOptions,
    transport: Arc<dyn HttpTransport>,
    sleeper: Arc<dyn Sleeper>,
) {
    let total = request.pages.len() as u32;
    sink.emit(PipelineEvent::BatchStarted {
        request_id: request.request_id.clone(),
        total_pages: total,
    });

    let mut ok_pages = 0u32;
    let mut failed_pages = 0u32;

    for page in &request.pages {
        if cancel.is_cancelled() {
            sink.emit(PipelineEvent::PageFailed {
                page_id: page.page_id.clone(),
                failure: PageFailure::Cancelled,
            });
            failed_pages += 1;
            continue;
        }

        sink.emit(PipelineEvent::PageStage {
            page_id: page.page_id.clone(),
            stage: PageStage::Decode,
        });

        let (bytes, img) = match load_page_image(page) {
            Ok(v) => v,
            Err(e) => {
                sink.emit(PipelineEvent::PageFailed {
                    page_id: page.page_id.clone(),
                    failure: PageFailure::Decode(e),
                });
                failed_pages += 1;
                continue;
            }
        };

        let source_hash = page
            .known_source_hash
            .clone()
            .unwrap_or_else(|| blake3::hash(&bytes).to_hex().to_string());
        let page_cache_key = cache_key(&source_hash, &request.options);

        // Check prior cache hit
        if page
            .prior
            .as_ref()
            .is_some_and(|p| p.cache_key == page_cache_key)
        {
            let res = PageResult {
                schema_version: SCHEMA_VERSION,
                page_id: page.page_id.clone(),
                source_hash,
                cache_key: page_cache_key,
                page_px: PagePx {
                    width: img.width(),
                    height: img.height(),
                },
                regions: page
                    .prior
                    .as_ref()
                    .map(|p| p.regions.clone())
                    .unwrap_or_default(),
                composited: None,
            };
            sink.emit(PipelineEvent::PageDone(res));
            ok_pages += 1;
            continue;
        }

        sink.emit(PipelineEvent::PageStage {
            page_id: page.page_id.clone(),
            stage: PageStage::Recognize,
        });

        let llm_req = LlmPageRequest {
            image: &bytes,
            chain: request.options.chain.clone(),
            source_lang: &request.options.source_lang,
            target_lang: &request.options.target_lang,
            glossary: &request.options.glossary,
            context: &page.context,
            known_characters: &[],
            reading_direction: request.options.reading_direction,
            prep: PrepOptions::default(),
        };

        let policy = crate::llm::call::RetryPolicy::default();
        let now_fn = std::time::SystemTime::now;
        let env = CallEnv {
            transport: transport.as_ref(),
            sleeper: sleeper.as_ref(),
            policy: &policy,
            cancel: &cancel,
            now: &now_fn,
        };

        let llm_page = match translate_page(&env, &llm_req).await {
            Ok(p) => p,
            Err(e) => {
                let failure = match e {
                    CallError::Cancelled => PageFailure::Cancelled,
                    CallError::Provider(pe) => PageFailure::Provider(pe),
                };
                sink.emit(PipelineEvent::PageFailed {
                    page_id: page.page_id.clone(),
                    failure,
                });
                failed_pages += 1;
                continue;
            }
        };

        sink.emit(PipelineEvent::PageStage {
            page_id: page.page_id.clone(),
            stage: PageStage::Refine,
        });
        sink.emit(PipelineEvent::PageStage {
            page_id: page.page_id.clone(),
            stage: PageStage::Layout,
        });
        sink.emit(PipelineEvent::PageStage {
            page_id: page.page_id.clone(),
            stage: PageStage::Erase,
        });

        let region_inputs: Vec<RegionInput> = llm_page
            .regions
            .iter()
            .map(|r| RegionInput {
                id: r.id.clone(),
                bounds: r.bounds,
                text: r.translated.clone(),
                style_hint: BubbleStyleHint::Balloon,
                kind: r.kind,
                typography: None,
            })
            .collect();

        let mut outcome = process_page_result(
            &page.page_id,
            &img,
            Some(&bytes),
            &region_inputs,
            &renderer,
            options,
            request.options.render_mode,
        );

        for (res, llm_reg) in outcome.result.regions.iter_mut().zip(&llm_page.regions) {
            res.source_text = llm_reg.text.clone();
            res.speaker = llm_reg.speaker.clone();
            res.gender = llm_reg.gender.clone();
            res.age = llm_reg.age.clone();
            res.emotion = llm_reg.emotion.clone();
            res.intensity = llm_reg.intensity;
        }

        outcome.result.cache_key = page_cache_key;
        sink.emit(PipelineEvent::PageDone(outcome.result));
        ok_pages += 1;
    }

    sink.emit(PipelineEvent::BatchFinished {
        request_id: request.request_id,
        ok_pages,
        failed_pages,
    });
}

#[cfg(not(feature = "llm"))]
pub(crate) fn run_batch_no_llm(
    request: TranslateBatchRequest,
    sink: Arc<dyn EventSink>,
    cancel: CancelToken,
    _renderer: Arc<FontRenderer>,
    _options: PipelineOptions,
) {
    let total = request.pages.len() as u32;
    sink.emit(PipelineEvent::BatchStarted {
        request_id: request.request_id.clone(),
        total_pages: total,
    });

    let mut ok_pages = 0u32;
    let mut failed_pages = 0u32;

    for page in &request.pages {
        if cancel.is_cancelled() {
            sink.emit(PipelineEvent::PageFailed {
                page_id: page.page_id.clone(),
                failure: PageFailure::Cancelled,
            });
            failed_pages += 1;
            continue;
        }

        let (bytes, img) = match load_page_image(page) {
            Ok(v) => v,
            Err(e) => {
                sink.emit(PipelineEvent::PageFailed {
                    page_id: page.page_id.clone(),
                    failure: PageFailure::Decode(e),
                });
                failed_pages += 1;
                continue;
            }
        };

        let source_hash = page
            .known_source_hash
            .clone()
            .unwrap_or_else(|| blake3::hash(&bytes).to_hex().to_string());
        let page_cache_key = cache_key(&source_hash, &request.options);

        if let Some(prior) = &page.prior {
            if prior.cache_key == page_cache_key {
                let res = PageResult {
                    schema_version: SCHEMA_VERSION,
                    page_id: page.page_id.clone(),
                    source_hash,
                    cache_key: page_cache_key,
                    page_px: PagePx {
                        width: img.width(),
                        height: img.height(),
                    },
                    regions: prior.regions.clone(),
                    composited: None,
                };
                sink.emit(PipelineEvent::PageDone(res));
                ok_pages += 1;
                continue;
            }
        }

        sink.emit(PipelineEvent::PageFailed {
            page_id: page.page_id.clone(),
            failure: PageFailure::Internal("LLM feature disabled in pipeline build".to_string()),
        });
        failed_pages += 1;
    }

    sink.emit(PipelineEvent::BatchFinished {
        request_id: request.request_id,
        ok_pages,
        failed_pages,
    });
}
