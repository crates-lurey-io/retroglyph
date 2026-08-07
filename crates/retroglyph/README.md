# retroglyph

[![crates.io](https://img.shields.io/crates/v/retroglyph.svg)](https://crates.io/crates/retroglyph)
[![docs.rs](https://img.shields.io/docsrs/retroglyph)](https://docs.rs/retroglyph)
[![license](https://img.shields.io/crates/l/retroglyph.svg)](https://github.com/crates-lurey-io/retroglyph/blob/main/LICENSE)

The one dependency you add for [retroglyph](https://github.com/crates-lurey-io/retroglyph): the
double-buffered `Terminal`/`App` game loop, styled cells, text and layout helpers, and input events
-- `app`, `color`, `event`, `frames`, `grid`, `layout`, `surface`, `terminal`, `text`, `tile`,
`symbols` -- plus a [prelude](#quick-start) with the handful of names a program can't avoid, and one
feature-gated module per backend (`crossterm`, `software`, `gl`, `wgpu`, `ui`). Writing a new
backend instead of a game? Depend on [`retroglyph-core`](https://crates.io/crates/retroglyph-core)
directly for its lower-level `backend`, `dev`, and `math` modules.

## Quick start

```sh
cargo add retroglyph
```

```rust,no_run
use retroglyph::crossterm::Crossterm;
use retroglyph::prelude::*;

struct Game;

impl App<Crossterm> for Game {
    fn update(&mut self, term: &mut Terminal<Crossterm>, _frame: &Frame) -> Flow {
        term.surface().put((5, 5), '@', Style::new().fg(Color::GREEN));

        if let Some(Event::Key(k)) = term.poll(std::time::Duration::from_secs(1)) {
            if k.code == KeyCode::Char('q') {
                return Flow::Exit;
            }
        }
        Flow::Continue
    }
}

fn main() -> std::io::Result<()> {
    retroglyph::app::run(Crossterm::new()?, Game)
}
```

Want a native window or a browser tab instead of a real terminal? Enable the `software`, `gl`, or
`wgpu` feature instead of (or alongside) `crossterm`: same `Terminal`/`App` contract, a different
`Backend` type.

## Features

<!-- gen-features:start -->

Default features: `crossterm`, `ui`.

### `crossterm`

🟢 Enabled by default.

Re-exports `retroglyph-crossterm` as `crossterm`: a real-terminal `Backend` via `crossterm`.

### `default-font`

⚪ Optional.

Forwards each enabled backend's own `default-font` feature (an embedded Unscii 16 bitmap font), so a
caller doesn't need to know which backend crate actually owns it.

### `gl`

⚪ Optional.

Re-exports `retroglyph-gl` as `gl`: a GPU `Backend` via `glow` (OpenGL 3.3 native, WebGL2 wasm).
Also pulls in the curated windowed re-exports (`WindowConfig`, `PresenterBuilder`, `run_app`,
`run_app_on`).

### `software`

⚪ Optional.

Re-exports `retroglyph-software` as `software`: a CPU pixel `Backend` via `softbuffer`. Also pulls
in the curated windowed re-exports (`WindowConfig`, `PresenterBuilder`, `run_app`, `run_app_on`).

### `testing`

⚪ Optional.

Enables `TestHarness` and its error, the published headless `App` driver for testing your own `App`.
Forwards to `retroglyph-core`'s own `testing` feature.

### `tracing`

⚪ Optional.

Forwards to `retroglyph-crossterm`'s `tracing` feature: instruments `draw`/`flush`/`poll_event` with
`tracing` spans for profiling render/input time.

### `ui`

🟢 Enabled by default.

Re-exports `retroglyph-ui` as `ui`: the immediate-mode widget/layout toolkit.

### `wgpu`

⚪ Optional.

Re-exports `retroglyph-wgpu` as `wgpu`: a GPU `Backend` via `wgpu` (Vulkan, Metal, D3D12, WebGPU).
Also pulls in the curated windowed re-exports (`WindowConfig`, `PresenterBuilder`, `run_app`,
`run_app_on`).

<!-- gen-features:end -->

See [docs.rs](https://docs.rs/retroglyph) for the full API, or the
[workspace README](https://github.com/crates-lurey-io/retroglyph#readme) for the crate list and more
examples.
