# Task Execution Harness (Mandatory for Every Coding Request)

1. **Gate 1 — Grounding & Assumptions**: Read `.agents/CODEBASE_MAP.md`, `.agents/memory/lessons.md`, and `referenceRepo` conventions. Surface assumptions upfront; track > 3-file work in `.agents/memory/active-plan.md`.
2. **Gate 2 — Surgical File Split**: Touch only required files/lines (`karpathy-skills`). Keep 1 primary responsibility per file (`> 300 LOC` = WARN, `> 450 LOC` = FAIL).
3. **Gate 3 — Implementation & 3-Strike Debugging**:
   - **Feature**: Model domain behavior with owned/borrowed values, enums/newtypes, and narrow traits where a real boundary needs one; implement adapters at I/O edges and use `Result<T, E>` for recoverable errors. Add UI state/message types only when the chosen UI architecture needs them.
   - **Bugfix**: Trace root cause; test 1 hypothesis at a time; **STOP after 3 failed attempts** (`obra/superpowers`) to reassess architecture. Do not replace errors with `unwrap()`/`expect()` on runtime paths; preserve ownership and cancellation semantics.
   - **No Leftovers**: Ban `todo!()`, `unimplemented!()`, avoidable panics, and unused imports. Do not swallow cancellation or turn it into a success-shaped fallback.
4. **Gate 4 — Edge-Case Unit Tests**: Cover behavior with unit/integration tests; run native tests on Linux/macOS/Windows and compile each declared cross target with its required SDK/linker.
5. **Gate 5 — Paranoid Check & Self-Healing**: Review unsafe blocks and invariants, dependency changes, platform-specific input boundaries, and secrets/signing material. Run `cargo test --test <file> -- <test_name>` and `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings` until green; run the stack security audit before delivery.
6. **Gate 6 — Lessons & Terse Output**: Append new project gotchas to `.agents/memory/lessons.md`; deliver concise high-signal summary (`caveman`).
