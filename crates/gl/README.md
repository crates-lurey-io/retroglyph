# retroglyph-gl

GPU rendering backend for [retroglyph](https://github.com/crates-lurey-io/retroglyph): native OpenGL
3.3 core and browser WebGL2, from a single codebase via [`glow`](https://crates.io/crates/glow).

Instanced quads (the beamterm/alacritty/xterm.js model): a unit quad is instanced `cols * rows`
times, each instance carrying an atlas glyph slot plus foreground/background color and sub-cell
offset, sampling an `R8` glyph atlas (`TEXTURE_2D_ARRAY`, glyphs grid-packed into layers) and
blending foreground over background by coverage. Grid layers are composited back-to-front on the
GPU, two passes per layer.

Implements [`retroglyph_window::Presenter`], so it drops into the same winit windowing loop as
`retroglyph-software`. The GL context is created from the window's raw handles (native, via
`glutin`) or the winit `<canvas>` (wasm, WebGL2), with no changes to `retroglyph-window`.

## Quick start

```sh
cargo add retroglyph-core retroglyph-window
cargo add retroglyph-gl --features default-font
```

```no_run
# #[cfg(not(target_arch = "wasm32"))]
# fn main() {
use retroglyph_gl::config::GlBackendBuilder;
use retroglyph_window::winit::{WindowConfig, run_windowed};
use retroglyph_core::color::Style;

let renderer = GlBackendBuilder::new()
    .grid_size(80, 25)
    .scale(2)
    .build()
    .expect("gl backend init failed");

let config = WindowConfig::fit(&renderer, "Hello, GL", None, true);
run_windowed(config, renderer, move |term| {
    term.draw(|s| s.print((0, 0), "Hello from retroglyph-gl!", Style::default()))
        .ok();
})
.expect("event loop failed");
# }
# #[cfg(target_arch = "wasm32")]
# fn main() {}
```

## Features

<!-- gen-features:start -->
<details>

<summary>Features: all optional, none enabled by default.</summary>

This crate has no default features; every feature below is optional and off unless enabled.

### `default-font`

⚪ Optional.

Embeds the Unscii 16 default font so a caller can build a renderer with no font of its own.

Forwards to `retroglyph-window`'s `default-font` feature.

### `dev`

⚪ Optional.

Forwards `retroglyph-core`'s `dev` feature, which forces development diagnostics on in a build that
would otherwise compile them out (see `retroglyph_core::dev`).

### `tilesets`

⚪ Optional.

PNG sprite/tileset support (issue #366): decodes sprite sheets into an RGBA `TEXTURE_2D_ARRAY` atlas
and draws them in a second, source-over blended pass.

Forwards to `retroglyph-window`'s shared tileset decode, and (Linux only, where it's a dependency at
all) to the `retroglyph-software` dev-dependency's own `tilesets`, so the two stay in lockstep:
without this, `cargo test -p retroglyph-gl` (this feature off) still pulls in
`retroglyph-window/tilesets` transitively through that dev-dependency's forced-on `tilesets` below,
and the `PresenterBuilder` impl's `tileset` method (gated on this crate's own `tilesets` feature,
matching every other tileset-gated item in this crate) would then be missing an item the trait
requires whenever `retroglyph-window/tilesets` is on, regardless of this crate's own flag
(retroglyph#1192). Harmless outside a test build: `retroglyph-software` is dev-only, so this half of
the forward is a no-op for a plain `cargo build`/`check`.

</details>
<!-- gen-features:end -->

## Status

Renders a static CP437 bitmap-font atlas with per-cell foreground/background color. The atlas
grid-packs glyphs into `TEXTURE_2D_ARRAY` layers (a fixed NxM grid per layer), so a font can exceed
the 256-layer GL floor. Grid layers are composited back-to-front on the GPU (occlusion +
transparency matching `retroglyph-software`), rather than being flattened by the core `Terminal`.
Sub-cell offsets (`dx`/`dy`) shift the glyph by whole/fractional pixels via a two-pass draw (opaque
backgrounds first, then offset glyphs alpha-blended on top), so an offset glyph spills past its cell
edge into neighbors. WebGL2 context loss is recovered by rebuilding GL resources.

With the `tilesets` feature, PNG sprite sheets (`GlBackendBuilder::tileset`) render on the GPU too
(issue #366): sprites decode into an RGBA `TEXTURE_2D_ARRAY` and draw in a second, source-over
blended pass over the glyph passes, so a sprite's transparent pixels reveal the layers beneath. The
tileset config + decode is shared with the software backend via `retroglyph-window`.

Artwork larger than one cell is drawn as a multi-cell span (`Surface::put_span`): the span's anchor
emits one sprite across the whole footprint, and the cells it covers draw no glyph of their own and
take the anchor's background, so the sprite sits on one uniform backdrop. Their glyphs are the
span's text fallback, which cell backends print instead. `SpriteAlign` positions art inside a span
box larger than itself, folded into the per-instance sub-cell offset the vertex shader already
applies. All of it matches `retroglyph-software` pixel for pixel, asserted by the headless parity
tests.

## Testing

Beyond the CPU-side units (atlas byte layout, shader-string generation), `src/headless.rs` runs the
real GL pipeline into an offscreen framebuffer and reads it back, asserting property checks and
pixel-for-pixel parity with the `retroglyph-software` CPU rasterizer. Those tests are
`cfg(target_os = "linux")` and run in CI on Mesa's llvmpipe software rasterizer (no GPU needed).

## License

Same as the workspace (MIT).
