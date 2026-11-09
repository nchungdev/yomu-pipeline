# CLI / Worker Guardrails (`src/bin/yomu_pipeline.rs`)

1. The binary is a thin composition root: parse args with `clap`, validate config/env once, call the library, map errors to exit codes.
2. Keep business logic in the library so `yomu` (via its `crates/pipeline` wrapper and FFI) gets identical behavior.
3. Logs and stderr must not dump full page text or secrets; keep worker output machine-readable (JSON) where `tests/cli_worker_tests.rs` expects it.
