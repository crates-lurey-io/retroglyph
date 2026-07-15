# retroglyph-software

[![crates.io](https://img.shields.io/crates/v/retroglyph-software.svg)](https://crates.io/crates/retroglyph-software)
[![docs.rs](https://img.shields.io/docsrs/retroglyph-software)](https://docs.rs/retroglyph-software)
[![coverage](https://codecov.io/gh/crates-lurey-io/retroglyph/graph/badge.svg?token=z8BBUp8fiY&flag=software)](https://codecov.io/gh/crates-lurey-io/retroglyph)
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
