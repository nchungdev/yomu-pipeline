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
