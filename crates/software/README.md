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

Optional features: `default-font` (an embedded Unscii 16 bitmap font) and `tilesets` (PNG sprite
sheet tilesets with alpha blending).

Artwork larger than one cell is drawn as a multi-cell span (`Terminal::put_span`): the span's anchor
blits one sprite across the whole footprint, and the cells it covers draw no glyph of their own and
take the anchor's background, so the sprite sits on one uniform backdrop. Their glyphs are the
span's text fallback, which cell backends print instead. `SpriteAlign` positions art inside a span
box larger than itself.

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

## Frame rate and window title live on `WindowConfig`, not on this builder

This builder configures the renderer -- font, grid, scale, tilesets -- and nothing else. Window
title and frame rate are windowing concerns, so they belong to
[`WindowConfig::fit`](https://docs.rs/retroglyph-window/latest/retroglyph_window/winit/struct.WindowConfig.html#method.fit),
which takes both:

```rust,ignore
let config = WindowConfig::fit(&renderer, "My Game", Some(60));
```

`target_fps` there is a mode switch before it is a cap. `Some(_)` renders continuously, which is
what an app animating over time (a `Tween`, a `FrameClock`, anything driven by `Frame::delta`)
needs; `None` renders only in response to input, which is right for an event-driven UI that should
sleep when idle but will look frozen under an animation.

The number itself is only honored on native, where the event loop can sleep until the next frame
deadline. On `wasm32` the browser owns frame pacing -- winit's web backend services each requested
redraw on the next `requestAnimationFrame` -- so a `Some(_)` app always runs at the display refresh
rate, and a native app relying on `target_fps` to throttle below that will run uncapped once ported
to the web.

## Backend parity: occupied default-background cells

`SoftwareRenderer`'s layer compositing
([`Backend::draw_layers`](https://docs.rs/retroglyph-software/latest/retroglyph_software/struct.SoftwareRenderer.html#method.draw_layers))
matches cell backends (e.g. `retroglyph-crossterm`'s `Grid::flatten_into`) for an occupied space
(`' '`, non-empty) with a `Color::Default` background on a layer above 0: that space is opaque and
erases whatever glyph was on the layer beneath it, replacing it with a blank cell, on both kinds of
backend.

The background that shows through after that erasure is _not_ necessarily this renderer's default
background color, though: matching `flatten_into`'s inheritance rule, a `Color::Default` background
never overwrites the destination background, so the erased cell inherits whichever layer below it
(down to and including layer 0) last established a real background. See the `resolve_bg_fill`
private helper's doc comment in `crates/software/src/lib.rs` for the implementation-level rule
(retroglyph#304).
