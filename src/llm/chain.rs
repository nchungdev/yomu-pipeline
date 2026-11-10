//! Fallback chain execution and multi-tile page orchestration (ADR-0002 section 4 & 7.2).
//!
//! The app provides an ordered sequence of candidate [`ProviderSpec`] items. The engine executes
//! the chain, skipping candidates that do not support the request language, and falling through
//! to subsequent candidates when a candidate fails with a retryable provider error.
//!
//! Cancellation terminates processing immediately. When all candidates fail, errors are merged
//! by priority: RateLimited (smallest retry_after_ms hint) > QuotaExhausted (smallest reset_hint_ms)
//! > Auth > last error.

use super::call::CallError;
use super::image_prep::{prepare_page, PrepOptions};
use super::prompt::{build_prompt, PromptInput};
use super::provider::{translate_tile, CallEnv, TileInput};
use super::reading_order::sort_regions_by_reading_order;
use super::region::{LlmPage, LlmRegion};
use super::tiling::{dedupe_overlapping, remap_tile_rect};
use crate::contract::v1::{
    ContextLine, GlossaryEntry, ProviderError, ProviderSpec, ReadingDirection,
};

/// Filters provider candidates by matching language requirements.
///
/// An empty `spec.languages` list indicates support for all languages.
/// If non-empty, matches if any entry equals `target_lang`, `source_lang` (if not "auto"),
/// or the pair `"source->target"`.
pub fn matches_languages(spec: &ProviderSpec, source_lang: &str, target_lang: &str) -> bool {
    if spec.languages.is_empty() {
        return true;
    }
    let s = source_lang.trim();
    let t = target_lang.trim();
    spec.languages.iter().any(|l| {
        let l = l.trim();
        l.eq_ignore_ascii_case(t)
            || (s != "auto" && !s.is_empty() && l.eq_ignore_ascii_case(s))
            || l.eq_ignore_ascii_case(&format!("{s}->{t}"))
    })
}

/// Merges candidate failure errors according to ADR-0002 priority:
/// 1. `RateLimited`: choose the SMALLEST `retry_after_ms` hint among all rate-limit errors.
/// 2. `QuotaExhausted`: choose the SMALLEST `reset_hint_ms` hint among all quota errors.
/// 3. `Auth`: keep authentication failure.
/// 4. Otherwise: return the last error encountered.
pub fn merge_provider_errors(errors: &[ProviderError]) -> ProviderError {
    assert!(!errors.is_empty(), "cannot merge empty errors");

    let rate_limited: Vec<_> = errors
        .iter()
        .filter_map(|e| match e {
            ProviderError::RateLimited { retry_after_ms } => Some(*retry_after_ms),
            _ => None,
        })
        .collect();

    if !rate_limited.is_empty() {
        let min_hint = rate_limited.iter().copied().flatten().min();
        return ProviderError::RateLimited {
            retry_after_ms: min_hint,
        };
    }

    let quota_exhausted: Vec<_> = errors
        .iter()
        .filter_map(|e| match e {
            ProviderError::QuotaExhausted { reset_hint_ms } => Some(*reset_hint_ms),
            _ => None,
        })
        .collect();

    if !quota_exhausted.is_empty() {
        let min_hint = quota_exhausted.iter().copied().flatten().min();
        return ProviderError::QuotaExhausted {
            reset_hint_ms: min_hint,
        };
    }

    if let Some(auth_err) = errors
        .iter()
        .find(|e| matches!(e, ProviderError::Auth { .. }))
    {
        return auth_err.clone();
    }

    errors.last().cloned().unwrap_or(ProviderError::Network)
}

/// Translates one tile by walking the fallback chain of providers.
///
/// Providers whose `languages` do not match `(tile.source_lang, tile.target_lang)` are skipped.
/// When a candidate fails, execution falls through to the next candidate.
/// Cancellation halts immediately.
pub async fn translate_tile_chain(
    env: &CallEnv<'_>,
    chain: &[ProviderSpec],
    tile: &TileInput<'_>,
) -> Result<Vec<LlmRegion>, CallError> {
    if env.cancel.is_cancelled() {
        return Err(CallError::Cancelled);
    }
    if chain.is_empty() {
        return Err(CallError::Provider(ProviderError::BadRequest(
            "no translation providers configured".to_owned(),
        )));
    }

    let mut attempted_errors: Vec<ProviderError> = Vec::new();

    for spec in chain {
        if env.cancel.is_cancelled() {
            return Err(CallError::Cancelled);
        }
        if !matches_languages(spec, tile.source_lang, tile.target_lang) {
            continue;
        }

        match translate_tile(env, spec, tile).await {
            Ok(regions) => return Ok(regions),
            Err(CallError::Cancelled) => return Err(CallError::Cancelled),
            Err(CallError::Provider(err)) => {
                attempted_errors.push(err);
            }
        }
    }

    if attempted_errors.is_empty() {
        return Err(CallError::Provider(ProviderError::BadRequest(
            "no eligible translation provider for request language".to_owned(),
        )));
    }

    Err(CallError::Provider(merge_provider_errors(
        &attempted_errors,
    )))
}

/// Request for full-page vision translation.
#[derive(Debug, Clone)]
pub struct LlmPageRequest<'a> {
    pub image: &'a [u8],
    pub chain: Vec<ProviderSpec>,
    pub source_lang: &'a str,
    pub target_lang: &'a str,
    pub glossary: &'a [GlossaryEntry],
    pub context: &'a [ContextLine],
    pub known_characters: &'a [String],
    pub reading_direction: ReadingDirection,
    pub prep: PrepOptions,
}

/// Prepares a page image, builds the prompt, executes translation across tiles via the provider chain,
/// remaps and deduplicates coordinates, and sorts regions into natural reading order.
///
/// # Per-tile failure policy
/// If any tile in a multi-tile page fails after exhausting the fallback chain,
/// `translate_page` fails atomically with that typed [`CallError`]. This avoids
/// producing partial pages with missing regions.
pub async fn translate_page(
    env: &CallEnv<'_>,
    req: &LlmPageRequest<'_>,
) -> Result<LlmPage, CallError> {
    if env.cancel.is_cancelled() {
        return Err(CallError::Cancelled);
    }

    let prepared = prepare_page(req.image, &req.prep).map_err(|e| {
        CallError::Provider(ProviderError::BadRequest(format!("image prep failed: {e}")))
    })?;

    let source_lang_opt = if req.source_lang == "auto" || req.source_lang.is_empty() {
        None
    } else {
        Some(req.source_lang)
    };
    let prompt_input = PromptInput {
        source_lang: source_lang_opt,
        target_lang: req.target_lang,
        glossary: req.glossary,
        context: req.context,
        known_characters: req.known_characters,
        reading_direction: req.reading_direction,
    };
    let prompt = build_prompt(&prompt_input);

    let is_multi_tile = prepared.tiles.len() > 1;
    let mut all_regions: Vec<LlmRegion> = Vec::new();

    for tile in &prepared.tiles {
        if env.cancel.is_cancelled() {
            return Err(CallError::Cancelled);
        }

        let tile_input = TileInput {
            prompt: &prompt,
            jpeg: &tile.jpeg,
            mime: tile.mime,
            reading_direction: req.reading_direction,
            page_id: "",
            source_lang: req.source_lang,
            target_lang: req.target_lang,
        };

        let tile_regions = translate_tile_chain(env, &req.chain, &tile_input).await?;

        for mut region in tile_regions {
            if is_multi_tile {
                if let Some(remapped) =
                    remap_tile_rect(tile, prepared.height, prepared.width, region.bounds)
                {
                    region.bounds = remapped;
                    all_regions.push(region);
                }
            } else {
                all_regions.push(region);
            }
        }
    }

    let deduped = if is_multi_tile {
        dedupe_overlapping(all_regions, |r| r.bounds, |r| &r.text)
    } else {
        all_regions
    };

    let mut final_regions = deduped;
    sort_regions_by_reading_order(&mut final_regions, req.reading_direction);

    Ok(LlmPage {
        source_hash: prepared.source_hash,
        regions: final_regions,
    })
}
