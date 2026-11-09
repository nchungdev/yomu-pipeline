# Yomu Pipeline — AI Agent Engineering & Security Contract

> Managed by `project-pro` (`.agents/project-profile.json`). Every coding task MUST follow the 6-Gate Execution Harness.

## 1. Architectural Profile
- **Stack**: Rust (single crate / headless library + CLI worker)
- **Architecture**: `lightweight` — one module per pipeline stage: `contour`, `inpaint` (+ `flat_fill`), `layout` (+ `dp`), `typography` (+ `analyze`), `render`, `mask`, `types`; `pipeline::process_page` composes them. `src/bin/yomu_pipeline.rs` is the CLI/worker entrypoint.
- **Flow**: `headless-pipeline` — pure functions over DTOs (`types.rs`) and `image::RgbaImage`; no UI, no global state.
- **Reference**: [`LukeMathWalker/zero-to-production`](https://github.com/LukeMathWalker/zero-to-production) for typed errors, config, and worker/service structure.
- **Consumer**: `nchungdev/yomu` depends on this crate by path (`../yomu-pipeline` beside the `yomu` checkout) through its `crates/pipeline` wrapper. Public API changes must keep that wrapper compiling.
- **Enabled Capabilities**: `solid-dry`, `typed-errors`, `arch-lint`, `testing-ci`, `di-core`

## 2. Canonical Commands
- **Build**: `cargo build`
- **Run All Tests**: `cargo test`
- **Run Single Test**: `cargo test --test <file> -- <test_name>` (integration tests live in `tests/`)
- **Lint & Typecheck**: `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings`
- **Security Audit**: `cargo audit`

## 3. Module Dependency Rules
- **Dependency Rule**: Stage modules are pure and must not import `src/bin`, CLI parsing (`clap`), or process/env/filesystem code. Only `src/bin` and explicit loader functions touch the filesystem or env.
- **Zero App Coupling**: No `egui`, `eframe`, or any `yomu` crate dependency — this crate must stay usable headlessly and from FFI.
- **Rust Modeling**: Prefer borrowing (`&RgbaImage`, slices) over cloning buffers; enums/newtypes for invariants (e.g. `NormalizedRect`); `Result<T, PipelineError>` via `thiserror` for recoverable errors. No DTO/entity pairs unless a wire format actually differs.
- **Config**: Worker/CLI settings come from args or env validated at the entrypoint; never log page text or file paths containing user data without need.
- **Boundary Enforcement**: Module visibility (`pub(crate)` by default, re-export the public API from `lib.rs`) plus clippy `-D warnings`.

## 4. Mandatory 6-Gate Execution & Behavioral Guardrails
1. **Think & Ground (`karpathy-skills`)**: Read `.agents/CODEBASE_MAP.md`, `.agents/memory/lessons.md`, and `referenceRepo` before coding. Surface hidden assumptions; push back on unsafe anti-patterns; write `.agents/memory/active-plan.md` for > 3-file tasks.
2. **Surgical & Bounded Files**: Touch ONLY lines/files required. **1 File = 1 Primary Responsibility** (`> 300 LOC` = WARN, `> 450 LOC` = FAIL).
3. **Disciplined Coding (`obra/superpowers`)**: Build from domain types and behavior outward; use Rust-native adapters and UI state only where the project needs them. For bugs, trace **Root Cause** + **3-Strike Stop** (if 3 fixes fail, STOP & re-evaluate architecture). Zero unfinished stubs, zero dead code.
4. **SOLID & Anti-False-DRY**: Use ownership, borrowing, enums, and traits to model invariants; introduce wire/database models only at real boundaries. Keep state types as simple as the UI requires. Guard clauses (`<= 3` indent levels), immutable by default.
5. **Security & Paranoid Review (`gstack`)**: Zero hardcoded secrets, parameterized SQL, redacted logs, check race conditions/N+1/pagination bounds. Never bypass Git hooks with `--no-verify`.
6. **Self-Healing & Terse Delivery (`caveman`)**: Run `Single Test` -> `Lint/Typecheck` -> `audit` until green; log non-obvious pitfalls to `.agents/memory/lessons.md`; report concisely with file links.

## 5. Scoped Rules, Memory & Architecture Docs (Read on Demand)
- Codebase Map & Memory: [`.agents/CODEBASE_MAP.md`](./.agents/CODEBASE_MAP.md) | [`active-plan.md`](./.agents/memory/active-plan.md) | [`lessons.md`](./.agents/memory/lessons.md)
- Execution & Security: [`.agents/rules/execution-harness.md`](./.agents/rules/execution-harness.md) | [`.agents/rules/security-gate.md`](./.agents/rules/security-gate.md)
- Module Guardrails: [`stages`](./.agents/rules/domain-layer.md) | [`I/O & loaders`](./.agents/rules/data-layer.md) | [`CLI/worker`](./.agents/rules/presentation-layer.md)
- ADR-0001: [`docs/architecture/ADR-0001-architecture.md`](./docs/architecture/ADR-0001-architecture.md)
