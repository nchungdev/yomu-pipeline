# Yomu Pipeline — Active Multi-Step Plan (`planning-with-files`)

## Goal
- ADR-0002 step 2b: pure part of `src/llm/` (typed provider response classification + retry-hint
  parsing), behind cargo feature `llm`. Replaces the app's string-matching classifier. No network.

## Planned File Slices
- [x] `Cargo.toml` — feature `llm = ["dep:httpdate"]`, optional `httpdate`
- [x] `src/lib.rs` — `#[cfg(feature = "llm")] pub mod llm;`
- [x] `src/llm/mod.rs` — module wiring
- [x] `src/llm/retry_hint.rs` — Go-duration / Retry-After / header / Gemini / prose hints, `best_hint`
- [x] `src/llm/classify.rs` — `ResponseView`, `classify_response`
- [x] `src/llm/sanitize.rs` — URL / key / data redaction, 512-char cap
- [x] `tests/llm_retry_hint_tests.rs`, `tests/llm_classify_tests.rs`
- [ ] Next steps (not this one): reqwest/tokio transport, per-call retry loop, provider adapters

## Step 2c (transport + short retry loop) — done
- [x] `Cargo.toml`: `llm = [httpdate, reqwest, tokio]`, `test-util = ["llm"]`; reqwest 0.12 (same features as app), tokio `time` (+ dev `rt`,`macros`)
- [x] `src/llm/transport.rs` (types, `HttpTransport`, `Sleeper`, `TokioSleeper`), `transport_reqwest.rs` (caps 2 MiB / 64 KiB, same-host redirects), `transport_mock.rs` (test-util), `call.rs` (`call_with_retry`, `RetryPolicy`, `CallError`)
- [x] `tests/llm_call_tests.rs` (needs `test-util`), `tests/llm_transport_tests.rs` (loopback, needs `llm`)
- [ ] Next: provider adapters (Gemini / OpenAI-compatible request building + response parsing)

## Step 2d (image preparation, feature `llm`) — done
- [x] `src/llm/image_prep.rs` (`PrepOptions`, `prepare_page`, `PreparedPage/Tile`, `PrepError`, `detect_image_mime`, `is_mostly_grayscale`; hash of ORIGINAL bytes, passthrough, single decode, downscale or tile)
- [x] `src/llm/tiling.rs` (`remap_tile_rect`, generic `dedupe_overlapping`, `rect_iou`, `text_similarity`)
- [x] `tests/llm_image_prep_tests.rs`, `tests/llm_tiling_tests.rs`; `pub mod` + re-exports in `src/llm/mod.rs`
- Note: 1600 px / q85 / 1280x2048 tiles / 15% overlap are unbenchmarked design defaults.

## Step 2e (prompt, schemas, parsing, reading order) — done
- [x] `src/llm/{region,schemas,prompt,parse,reading_order}.rs` (feature `llm`, pure; no segment/two-stage schemas)
- [x] `tests/llm_parse_tests.rs`, `tests/llm_prompt_tests.rs`
- Prompt caps: 100 glossary entries / 6000 chars, 12 context lines / 2000 chars, 60 names; untrusted text neutralized and placed before the format section.

## Step 2f (provider adapters, feature `llm`) — done
- [x] `src/llm/{provider,endpoint,gemini,openai_compat,custom_http}.rs`: `translate_tile(&CallEnv, &ProviderSpec, &TileInput)` -> `Vec<LlmRegion>`; endpoint policy (https, or http for loopback/LAN; no userinfo/query/fragment); key only in sensitive headers and scrubbed from echoed error text
- [x] `Cargo.toml`: optional `base64 = "0.22"` in `llm` (+ dev-dep for tests)
- [x] `tests/llm_provider_tests.rs`, `tests/llm_provider_custom_tests.rs`, `tests/common/llm_provider.rs` (MockTransport only)
- [x] Next (2g): fallback chain + multi-tile orchestration on top of `translate_tile`

## Step 2g (fallback chain & multi-tile orchestration, feature `llm`) — done
- [x] `src/llm/chain.rs`:
  - `matches_languages(spec, src, dst)`: filters providers by target/source languages (empty = all)
  - `merge_provider_errors(&[ProviderError])`: priority merge `RateLimited` (min wait hint) > `QuotaExhausted` (min reset hint) > `Auth` > last error
  - `translate_tile_chain`: provider fallback on RateLimited/Quota/Auth/Server/Network/Timeout/ContentBlocked/BadRequest/InvalidResponse; halts immediately on cancel
  - `LlmPageRequest` (9 fields: image, chain, source_lang, target_lang, glossary, context, known_characters, reading_direction, prep)
  - `translate_page`: atomic page failure policy, pipeline flow `prepare_page` -> `translate_tile_chain` per tile -> `remap_tile_rect` -> seam deduplication (`dedupe_overlapping`) -> `sort_regions_by_reading_order` -> `LlmPage`
- [x] `src/llm/mod.rs` & `src/lib.rs`: re-exports `translate_page`, `LlmPageRequest`, `LlmPage`, `LlmRegion` behind `#[cfg(feature = "llm")]`
- [x] `tests/llm_chain_tests.rs` (8 unit/integration tests with MockTransport)
- [x] `tests/llm_translate_page_tests.rs` (3 end-to-end tests: 1-tile, tall webtoon seam deduplication & coordinate remapping, atomic failure policy)
- [x] Step 3 (TranslationEngine async + events + cancel + relayout_region + cache_key) — done
  - `src/engine/cache_key.rs`: deterministic `cache_key` and order-independent `glossary_hash` (ADR-0002 §7.2, §8).
  - `src/engine/sink.rs`: `CallbackEventSink`, `ChannelEventSink`, and `event_channel()`.
  - `src/engine/runner.rs`: multi-stage batch processor (`Decode` -> `Recognize` -> `Refine` -> `Layout` -> `Erase`), prior cache hit optimization, and cooperative cancellation.
  - `src/engine/embedded.rs`: `EmbeddedEngine` struct implementing `TranslationEngine` (`submit`, `relayout_region`, `cache_key`).
  - `tests/engine_embedded_tests.rs`: tests for cache key sensitivity, glossary sort stability, request validation, and `relayout_region`.
  - `tests/engine_submit_tests.rs`: integration tests for async batch execution, prior cache skip, and cooperative cancel.
- [ ] Next: Đợt 2 (Step 4 & Step 5: Adapter in app `yomu`, Step 10: Server HTTP in pipeline)
