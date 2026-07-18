# retroglyph-software

[![crates.io](https://img.shields.io/crates/v/retroglyph-software.svg)](https://crates.io/crates/retroglyph-software)
[![docs.rs](https://img.shields.io/docsrs/retroglyph-software)](https://docs.rs/retroglyph-software)
[![coverage](https://codecov.io/gh/crates-lurey-io/retroglyph/graph/badge.svg?token=z8BBUp8fiY&flag=software)](https://app.codecov.io/gh/crates-lurey-io/retroglyph/flags)
[![license](https://img.shields.io/crates/l/retroglyph-software.svg)](https://github.com/crates-lurey-io/retroglyph/blob/main/LICENSE)

A CPU rasterization backend for [retroglyph](https://github.com/crates-lurey-io/retroglyph): renders
grid cells into a pixel buffer and blits it to a window surface via
[`softbuffer`](https://crates.io/crates/softbuffer). `SoftwareBackend` holds configuration only
(font, grid size, scale); it builds a `SoftwareRenderer`, wrapped by
[`retroglyph-window`](https://crates.io/crates/retroglyph-window) into a real windowed `Backend`.

Optional features: `default-font` (an embedded VGA 8x16 bitmap font) and `tilesets` (PNG sprite
sheet tilesets with alpha blending).

## Quick start

```toml
[dependencies]
retroglyph-core = "0.1"
retroglyph-software = { version = "0.1", features = ["default-font"] }
retroglyph-window = "0.1"
```

Most apps open a real window via `retroglyph-window`'s `run_app`/`run_windowed` (see the
[workspace README](https://github.com/crates-lurey-io/retroglyph#readme) for that quick start).
Without a window,
[`run_headless`](https://docs.rs/retroglyph-software/latest/retroglyph_software/struct.SoftwareBackend.html#method.run_headless)
renders straight into an in-memory pixel buffer -- useful for pixel-level tests:

```rust
use retroglyph_core::{Backend, Color, Style, Terminal};
use retroglyph_software::SoftwareBackendBuilder;

let renderer = SoftwareBackendBuilder::new()
    .grid_size(1, 1)
    .scale(1)
    .build()
    .unwrap()
    .run_headless()
    .unwrap();

let mut term = Terminal::new(renderer);
term.put_styled(0, 0, ' ', Style::new().bg(Color::Rgb { r: 255, g: 0, b: 0 }));
term.present().unwrap();

assert!(term.backend().pixels().iter().all(|&p| p == 0x00FF_0000));
```

See [docs.rs](https://docs.rs/retroglyph-software) for the full API, or the
[workspace README](https://github.com/crates-lurey-io/retroglyph#readme) for a real backend quick
start.

## WASM caveat: `target_fps` is a no-op

[`SoftwareBackendBuilder::target_fps`](https://docs.rs/retroglyph-software/latest/retroglyph_software/struct.SoftwareBackendBuilder.html#method.target_fps)
caps the frame rate on native targets by sleeping in `about_to_wait` until the next frame deadline.
On `wasm32` this has no effect: `requestAnimationFrame` drives the loop at the display refresh rate
regardless, so a native app that relies on `target_fps` to throttle rendering will run uncapped once
ported to the web.

## Backend parity caveat

`SoftwareRenderer`'s layer compositing
([`Backend::draw_layers`](https://docs.rs/retroglyph-software/latest/retroglyph_software/struct.SoftwareRenderer.html#method.draw_layers))
diverges from cell backends (e.g. `retroglyph-crossterm`'s `Grid::flatten_into`) in one case: an
occupied space (`' '`, non-empty) with a `Color::Default` background on a layer above 0.

- **Cell backends:** flattening treats that space as opaque and erases whatever glyph was on the
  layer beneath it, replacing it with a blank cell.
- **`retroglyph-software`:** the renderer paints per pixel and never re-paints a lower layer's
  pixels once drawn, so the lower glyph's pixels remain visible underneath the higher-layer space
  instead of being erased.

Repainting the lower layer's background per pixel to match cell-backend behavior would require
tracking composited per-cell state, which this renderer intentionally avoids for simplicity and
performance. See the doc comment on
[`draw_layers`](https://docs.rs/retroglyph-software/latest/retroglyph_software/struct.SoftwareRenderer.html#method.draw_layers)
for the implementation-level note.
