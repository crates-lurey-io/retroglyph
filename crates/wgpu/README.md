# retroglyph-wgpu

GPU rendering backend for [retroglyph](https://github.com/crates-lurey-io/retroglyph): Vulkan,
Metal, and D3D12 from a single codebase via [`wgpu`](https://crates.io/crates/wgpu).

Instanced quads (the beamterm/alacritty/xterm.js model): a unit quad is instanced `cols * rows`
times, each instance carrying an atlas glyph slot plus foreground/background color and sub-cell
offset, sampling an `R8` glyph atlas (a 2D array texture with glyphs grid-packed into layers) and
blending foreground over background by coverage. Grid layers are composited back to front on the
GPU, in one render pass.

A cell costs 16 bytes and no index buffer: the vertex shader derives the quad's corners from the
vertex index and the cell's `(column, row)` from the instance index, so the only per-instance data
is the glyph slot, two colors, the sub-cell offset, and the compositing flags. Every layer's cells
live back to back in one buffer, so a frame is one `write_buffer` plus one render pass.

Implements [`retroglyph_window::Presenter`], so it drops into the same winit windowing loop as
`retroglyph-software` and `retroglyph-gl`. The device and surface are created from the window's raw
handles, with no changes to `retroglyph-window`.

## Quick start

```sh
cargo add retroglyph-core retroglyph-window
cargo add retroglyph-wgpu --features default-font
```

```rust,no_run
use retroglyph_core::color::{Color, Style};
use retroglyph_wgpu::WgpuBackendBuilder;
use retroglyph_window::winit::{WindowConfig, run_windowed};

fn main() {
    let renderer = WgpuBackendBuilder::new()
        .grid_size(80, 25)
        .scale(2)
        .build()
        .expect("wgpu backend init failed");

    let config = WindowConfig::fit(&renderer, "retroglyph-wgpu", None, true);
    run_windowed(config, renderer, move |term| {
        term.draw(|s| {
            s.print((2, 2), "Hello from the GPU!", Style::new().fg(Color::GREEN));
        })
        .ok();
    })
    .expect("event loop failed");
}
```

## Choosing between this and `retroglyph-gl`

Both are GPU backends drawing the same instanced-quad pipeline, and both are checked pixel for pixel
against the `retroglyph-software` CPU rasterizer, so they render identically. They differ in which
driver stack they reach and what they cost to depend on:

|                         | `retroglyph-wgpu`            | `retroglyph-gl`             |
| ----------------------- | ---------------------------- | --------------------------- |
| APIs                    | Vulkan, Metal, D3D12         | OpenGL 3.3, WebGL2          |
| Browser                 | no (see below)               | yes, WebGL2                 |
| `unsafe` in the backend | none                         | unavoidable (every GL call) |
| Offscreen render tests  | every platform               | Linux/EGL only              |
| Dependency weight       | heavier (naga, per-API HALs) | lighter (`glow` + `glutin`) |

Pick `retroglyph-gl` for a browser target or the smallest dependency tree; pick this one for
validation-layer diagnostics, a modern driver path, and a backend with no `unsafe` in it.

## Platform support

Native only. `wgpu`'s browser path (WebGPU) needs `request_adapter` and `request_device` driven to
completion asynchronously, and `Presenter::init_surface` is synchronous, which a browser's main
thread cannot bridge: it has no way to block. `retroglyph-gl` covers the browser through WebGL2,
whose context creation is synchronous, so nothing is lost by this crate not trying. The `webgpu` and
`gles` wgpu features are both off; see this crate's `Cargo.toml` for the full reasoning.

## Features

<!-- gen-features:start -->

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

PNG sprite/tileset support: decodes sprite sheets into an RGBA array-texture atlas and draws them in
a third, source-over blended pass per grid layer.

Forwards to `retroglyph-window`'s shared tileset decode.

<!-- gen-features:end -->

## Status

Renders a static CP437 bitmap-font atlas with per-cell foreground/background color. The atlas
grid-packs glyphs into array-texture layers (a fixed 16x16 grid per layer), shared with
`retroglyph-gl` through `retroglyph_window::atlas`, so a font chain can hold up to 65536 glyphs
inside the 256-layer floor wgpu's downlevel limits guarantee. Grid layers are composited back to
front on the GPU (occlusion and transparency matching `retroglyph-software`) rather than being
flattened by the core `Terminal`. Sub-cell offsets (`dx`/`dy`) shift the glyph via a two-pass draw
(opaque backgrounds first, then offset glyphs alpha-blended on top), so an offset glyph spills past
its cell edge into neighbors.

With the `tilesets` feature, PNG sprite sheets (`WgpuBackendBuilder::tileset`) render on the GPU
too: sprites decode into an RGBA array texture and draw in a third, source-over blended pass over
the glyph passes, so a sprite's transparent pixels reveal the layers beneath. The tileset config and
decode are shared with the software backend via `retroglyph-window`, and the fragment shader
reproduces `Tint::apply`'s `u8` arithmetic exactly rather than approximating it in float.

Multi-cell spans, `SpriteAlign` placement, and the `Color::Default` background-inheritance rules all
behave as they do on the other backends, asserted by the offscreen parity tests.

Colors land byte-exact: the surface is always viewed through a non-sRGB format, so the shader's
`u8 / 255.0` output is written verbatim rather than re-encoded to sRGB and silently brightened
relative to the CPU rasterizer.

## Testing

Beyond the CPU-side units (instance byte layout, uniform block size, shader composition),
`src/headless.rs` runs the real pipeline into an offscreen texture and reads it back, asserting
property checks (a full-block cell is entirely its foreground, a glyph matches the font's own
coverage bits) and pixel-for-pixel parity with the `retroglyph-software` CPU rasterizer.

Unlike `retroglyph-gl`'s Linux-only equivalent, these need no platform-specific setup: `wgpu` treats
"a device with no surface" as a first-class option on every backend, so they run wherever an adapter
exists, including a developer's laptop. A machine with no adapter at all skips them with a message;
set `RETROGLYPH_REQUIRE_WGPU` to turn that skip into a hard failure, which is what CI does.

`WGPU_BACKEND`, `WGPU_POWER_PREF`, and `WGPU_VALIDATION` are passed through to `wgpu` rather than
overridden, so a run can be pinned to one backend or one software rasterizer.

## License

Same as the workspace (MIT).

[`retroglyph_window::Presenter`]:
  https://docs.rs/retroglyph-window/latest/retroglyph_window/presenter/trait.Presenter.html
