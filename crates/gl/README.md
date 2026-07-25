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

```rust,ignore
use retroglyph_gl::GlBackendBuilder;
use retroglyph_window::winit::{WindowConfig, run_windowed};

let renderer = GlBackendBuilder::new()
    .grid_size(80, 25)
    .scale(2)
    // .ttf(std::fs::read("MyFont.ttf")?, 16.0) // dynamic TrueType instead of the bitmap font
    .build()
    .expect("gl backend init failed");

let config = WindowConfig::fit(&renderer, "Hello, GL", None);
run_windowed(config, renderer, move |term| {
    term.clear();
    term.print(0, 0, "Hello from retroglyph-gl!");
    true
})
.expect("event loop failed");
```

## Features

| Feature        | Effect                                                                 |
| -------------- | ---------------------------------------------------------------------- |
| `default-font` | Embeds the Unscii 16 font so a renderer can be built with no own font. |

## Status

Renders either a static CP437 bitmap-font atlas or a dynamic TrueType atlas
(`GlBackendBuilder::ttf`, via `fontdue`), with per-cell foreground/background color. The atlas
grid-packs glyphs into `TEXTURE_2D_ARRAY` layers, so a font can exceed the 256-layer GL floor; the
dynamic atlas rasterizes glyphs on demand into an LRU-managed cache. Grid layers are composited
back-to-front on the GPU (occlusion + transparency matching `retroglyph-software`), rather than
being flattened by the core `Terminal`. Sub-cell offsets (`dx`/`dy`) shift the glyph by
whole/fractional pixels via a two-pass draw (opaque backgrounds first, then offset glyphs
alpha-blended on top), so an offset glyph spills past its cell edge into neighbors. WebGL2 context
loss is recovered by rebuilding GL resources (the dynamic atlas re-rasterizes its working set).
Sprites/tilesets remain a follow-up.

## Testing

Beyond the CPU-side units (atlas byte layout, shader-string generation), `src/headless.rs` runs the
real GL pipeline into an offscreen framebuffer and reads it back, asserting property checks and
pixel-for-pixel parity with the `retroglyph-software` CPU rasterizer. Those tests are
`cfg(target_os = "linux")` and run in CI on Mesa's llvmpipe software rasterizer (no GPU needed); see
`docs/testing.md` for details.

## License

Same as the workspace (MIT).
