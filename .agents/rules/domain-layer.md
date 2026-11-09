# Stage Module Guardrails (`contour`, `inpaint`, `layout`, `typography`, `mask`, `render`, `types`)

1. **Pure & Headless**: No `egui`/`eframe`/`yomu` imports, no `clap`, no `std::env`, no filesystem access. Inputs are DTOs from `types.rs` and image buffers; outputs are values or `Result<T, PipelineError>`.
2. **Rust Types**: Validate invariants at construction (`NormalizedRect::new` returns `Result`), use enums for exclusive styles/orientations, and exhaustive `match`.
3. **Performance & Safety**: Borrow image buffers; bound loops by image dimensions; no `unwrap()`/`expect()` on data-dependent paths; no `unsafe`.
4. **Tests**: Each stage has an integration test file in `tests/` built from synthetic images — keep them deterministic and fixture-free where possible.
