# I/O & Loader Guardrails

1. Filesystem, font loading, image decode/encode, and sidecar JSON parsing happen only in `src/bin` or dedicated loader functions (e.g. `FontRenderer` constructors), never inside stage algorithms.
2. Map I/O and `serde_json` failures into `PipelineError` variants that keep the source error.
3. Treat sidecar JSON and input images as untrusted: validate sizes and coordinates before indexing buffers.
