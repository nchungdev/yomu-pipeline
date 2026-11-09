# yomu-pipeline

Independent, headless comic & manga translation engine:
- Bubble contour detection & line slice calculation
- Statistical flat fill & smart inpaint (Otsu thresholding, variance rejection)
- Dynamic programming text wrapping with Vietnamese typography heuristics
- Typography inference (stroke width, core color, rotation, vertical orientation)
- Pure Rust headless typesetting & glyph rasterization (`ab_glyph`)
- Standalone CLI & headless background worker daemon

## Architecture & Features

`yomu-pipeline` is completely decoupled from any GUI or app framework (zero `egui`, zero `eframe`, zero `yomu` internal dependencies). It operates on pure DTOs and standard `image::RgbaImage` buffers.

- `contour`: 8-connected flood fill, row span anchoring, enclosed hole filling.
- `inpaint`: Statistical median background sampling, border mask preservation.
- `layout`: Dynamic programming line badness minimization, Vietnamese preposition/conjunction break penalties.
- `typography`: Radon transform tilt detection, vertical writing detection, ink cluster sampling.
- `render`: Font renderer with bundled Vietnamese diacritics font (`BeVietnamPro-Regular.ttf`) and custom font support.
- `pipeline`: Single-call `process_page` combining typography inference, inpainting, and typesetting.

## Usage

### As a Library

```rust
use yomu_pipeline::{process_page, FontRenderer, PipelineOptions, RegionInput, NormalizedRect, BubbleStyleHint};
use image::open;

let img = open("page.jpg")?.into_rgba8();
let renderer = FontRenderer::default();

let regions = vec![
    RegionInput {
        id: "b1".into(),
        bounds: NormalizedRect::new(0.1, 0.1, 0.3, 0.2)?,
        text: "Xin chào thế giới manga!".into(),
        style_hint: BubbleStyleHint::Balloon,
    }
];

let result = process_page(&img, &regions, &renderer, PipelineOptions::default());
result.image.save("page_translated.png")?;
```

### CLI Command

```sh
# Single page translation & inpainting
yomu-pipeline inpaint --image page.jpg --regions regions.json --output page_translated.png

# Headless worker daemon reading JSON lines from stdin
yomu-pipeline worker
```

## Testing & Quality

```sh
cargo test
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```
