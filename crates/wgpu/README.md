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

|                                   | `retroglyph-wgpu`    | `retroglyph-gl`             |
| --------------------------------- | -------------------- | --------------------------- |
| APIs                              | Vulkan, Metal, D3D12 | OpenGL 3.3, WebGL2          |
| Browser                           | not yet (see below)  | yes, WebGL2                 |
| `unsafe` in the backend           | none                 | unavoidable (every GL call) |
| Direct dependencies, transitively | 85 crates            | 54 crates                   |
| Clean debug build of the crate    | 15s                  | 7s                          |
| Offscreen render tests            | every platform       | Linux/EGL only              |

The dependency and build-time figures are for one host (macOS, `--all-features`); the ratio is what
matters, not the absolute numbers. The difference is `naga` (the shader front end that compiles this
crate's WGSL) plus `wgpu-core` (the validation and state-tracking layer that makes the API safe),
not the three backend APIs: only one hardware abstraction layer compiles per platform, since
`wgpu-hal`'s backends are target-gated. A macOS build pulls the Metal stack and no Vulkan; a Linux
build pulls `ash` and no Metal.

Pick `retroglyph-gl` for a browser target today, or when a smaller dependency tree matters more than
the rest. Pick this one for validation-layer diagnostics, a modern driver path, and a backend with
no `unsafe` in it.

## Platform support

Native today. The browser is not supported yet, and the obstacle is this crate's, not `wgpu`'s:
WebGPU is a browser API and `wgpu` targets it directly.

What blocks it is that `Presenter::init_surface` is synchronous while `request_adapter` and
`request_device` are not, and a browser's main thread has no way to block on a future. The way
around that is to defer rather than block: `Instance::create_surface` _is_ synchronous, so
`init_surface` can create the surface from the canvas, spawn the adapter and device request, and
return; `present` already no-ops until a device exists, so the first few frames would simply be
blank until it lands. That is a real design with real costs (a shared-state cell, blank startup
frames, and a wasm-only dependency set), and it is not implemented here.

`retroglyph-gl` covers the browser through WebGL2, whose context creation is synchronous, so there
is a working browser backend either way. The `webgpu` and `gles` wgpu features are off; see this
crate's `Cargo.toml`.

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
