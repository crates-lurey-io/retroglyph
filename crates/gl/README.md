# retroglyph-gl

GPU rendering backend for [retroglyph](https://github.com/crates-lurey-io/retroglyph): native OpenGL
3.3 core and browser WebGL2, from a single codebase via [`glow`](https://crates.io/crates/glow).

One instanced draw call per frame (the beamterm/alacritty/xterm.js model): a unit quad is instanced
`cols * rows` times, each instance carrying a glyph id plus foreground/background color, sampling an
`R8` glyph atlas (`TEXTURE_2D_ARRAY`) and blending `mix(bg, fg, coverage)`.

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

v1 renders a fixed CP437 bitmap-font atlas with per-cell foreground/background color. Layers are
flattened by the core `Terminal` before they reach this backend. Sub-cell offsets (`dx`/`dy`),
sprites/tilesets, dynamic Unicode atlases, and GPU-side layer compositing are tracked as follow-ups.

## License

Same as the workspace (MIT).
