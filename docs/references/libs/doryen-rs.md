# Research: doryen-rs

## Summary

doryen-rs is a Rust ASCII roguelike console library created by jice (also the creator of libtcod,
"The Doryen Library"). It renders via a GPU-accelerated GLSL fragment shader that composites the
entire console in a single draw call, targeting both native (OpenGL) and web (WebGL/WASM). It sits
in the same niche as BearLibTerminal but with a Rust-native API, GPU rendering, and first-class WASM
support. The project has 291 GitHub stars, 7 forks, and its last commit was October 2023.

## Findings

1. **Language: Rust** - Pure Rust with GLSL shaders for rendering. Edition 2021, MIT licensed.

   Dependencies are `uni-gl`, `uni-app` (from the [unrust](https://github.com/unrust/unrust) game
   engine), and `image` (PNG only).
   [Cargo.toml](https://github.com/jice-nospam/doryen-rs/blob/master/Cargo.toml)

1. **Author lineage** - Created by jice-nospam, who also created libtcod (The Doryen Library), the

   most widely-used C/C++ roguelike library. doryen-rs is the spiritual Rust successor. The name
   "doryen" carries over from libtcod's full name. [GitHub profile](https://github.com/jice-nospam)

1. **Single-draw-call GPU rendering** - The entire console is rendered in one GPU draw call. Three

   textures are uploaded each frame: ASCII codes (`u32` per cell), foreground colors (RGBA per
   cell), and background colors (RGBA per cell). A fragment shader samples these textures plus the
   font spritesheet to composite the final image. The vertex shader draws a single full-screen quad
   (triangle fan, 4 vertices). Console buffers use power-of-two dimensions for GPU texture
   compatibility. [program.rs](https://github.com/jice-nospam/doryen-rs/blob/master/src/program.rs),
   [doryen_fs.glsl](https://github.com/jice-nospam/doryen-rs/blob/master/src/doryen_fs.glsl)

1. **GLSL shader detail** - The fragment shader does: (a) map screen pixel to console cell

   coordinate, (b) sample the ASCII texture to get the glyph index, (c) look up the glyph in the
   font spritesheet, (d) sample foreground and background colors, (e) blend using
   `font_alpha * foreground * font_rgb + (1 - font_alpha) * background`. This means the CPU cost per
   frame is just filling three flat arrays; the GPU does all the actual rendering.
   [doryen_fs.glsl](https://github.com/jice-nospam/doryen-rs/blob/master/src/doryen_fs.glsl)

1. **Subcell resolution** - Uses special characters (ASCII 226-232) to achieve 2x resolution. The

   `blit_2x` method takes a 2x2 pixel block from an image, posterizes it to two colors, and picks
   the best subcell character to approximate the four subpixels. This is the same technique libtcod
   used. Enables surprisingly detailed images in ASCII.
   [img.rs](https://github.com/jice-nospam/doryen-rs/blob/master/src/img.rs)

1. **Trait-based Engine pattern** - Users implement the `Engine` trait with `init()`, `update()`,

   `render()`, and `resize()` methods. The `DoryenApi` trait provides access to the root `Console`,
   input, and FPS counters. Update runs at a fixed 60 ticks/second (independent of framerate), while
   render runs at display refresh rate. This is a clean game-loop abstraction.
   [app.rs](https://github.com/jice-nospam/doryen-rs/blob/master/src/app.rs)

1. **Console API** - Cell-based: `ascii(x, y, char)`, `fore(x, y, color)`, `back(x, y, color)` for

   individual cells. Higher-level: `rectangle()`, `area()`, `clear()`, `print()`, `print_color()`.
   Provides both bounds-checked and `unsafe_*` (unchecked) variants for performance. Console
   blitting with alpha blending via `blit()` / `blit_ex()`. Colors are `(u8, u8, u8, u8)` tuples.
   [console.rs](https://github.com/jice-nospam/doryen-rs/blob/master/src/console.rs)

1. **Colored text markup** - `print_color()` uses `#[color_name]` inline markers:

   `"#[red]arrows#[white] : move"`. Colors must be pre-registered with
   `register_color("red", (255, 92, 92, 255))`. Empty `#[]` pops to the previous color. This is
   simpler than libtcod's `%c` approach but requires manual color registration.
   [console.rs](https://github.com/jice-nospam/doryen-rs/blob/master/src/console.rs)

1. **Font support** - Expects a PNG spritesheet with 16x16 character grid layout. Auto-detects three

   formats based on the top-left pixel: RGBA (alpha channel transparency), greyscale (black =
   transparent, grey = white semi-transparent), and RGB (top-left pixel color = transparent).
   Character size is derived from `image_width / 16` unless overridden in the filename (e.g.,
   `myfont_8x8.png`). [app.rs](https://github.com/jice-nospam/doryen-rs/blob/master/src/app.rs)

1. **WASM support** - Compiles to `wasm32-unknown-unknown` via wasm-pack. The `uni-gl` and

   `uni-app` crates abstract over native OpenGL and WebGL. Each example includes
   `#[wasm_bindgen(start)]` boilerplate. The author hosts live WASM demos at
   `jice-nospam.github.io/doryen-rs/docs/`.
   [README](https://github.com/jice-nospam/doryen-rs/blob/master/README.md)

1. **Image blitting** - PNG images can be blitted to console backgrounds at 1:1 or 2x subcell

   resolution. `blit_ex()` supports rotation (radians) and scaling with transparency key color.
   Image loading is async-compatible (for WASM), requiring explicit `try_load()` checks.
   [img.rs](https://github.com/jice-nospam/doryen-rs/blob/master/src/img.rs)

1. **Companion crate: doryen-fov** - Separate crate for 2D field-of-view algorithms. Used in

   dev-dependencies of the examples. 409 SLoC, 9,480 all-time downloads. Also by jice.
   [crates.io/doryen-fov](https://crates.io/crates/doryen-fov)

1. **Examples and demos** - Ships with 10 examples: basic (walking @), perf test, fonts, unicode,

   console blitting, image blitting, subcell resolution, transparent consoles, text input, and a
   visual demo with dynamic lighting + real-time movement. The perf test fills every cell with
   random values each frame (randomized ASCII, fg, bg) to stress the renderer.
   [examples](https://github.com/jice-nospam/doryen-rs/tree/master/examples)

1. **No known games shipped with it** - No notable games or tools found built with doryen-rs. The

   demos serve as the primary showcases. The author's own game "Chronicles of Doryen" appears in
   subcell documentation screenshots but is not publicly released as a doryen-rs project.
   bracket-lib (by Herbert Wolverson) has absorbed most of the Rust roguelike community instead.

1. **Development activity** - Created June 2018. Latest release: v1.4.0 (unreleased on crates.io;

   v1.3.0 released Oct 2022). Last commit: Oct 2023. 5 open issues. The project is effectively in
   maintenance/dormant mode. [GitHub API](https://api.github.com/repos/jice-nospam/doryen-rs)

## Strengths

- **GPU rendering performance** - Single-draw-call architecture means CPU work is just filling flat

  arrays. The GPU handles all font lookup, color application, and compositing. This scales well to
  large consoles and high refresh rates.

- **WASM/native parity** - Same codebase runs natively and in browsers. Live web demos make it easy

  to evaluate.

- **Subcell resolution** - 2x effective resolution using specialized characters, well-implemented

  from libtcod heritage.

- **Clean game loop** - Fixed 60Hz update, variable render rate, proper frame skipping. The Engine

  trait is minimal and understandable.

- **Alpha-blended console blitting** - Sophisticated blending of overlapping consoles with

  per-foreground and per-background alpha, key color transparency. More capable than most roguelike
  libraries.

- **Multiple font formats** - Auto-detects RGBA, RGB, and greyscale font formats from the image

  data.

## Weaknesses

- **Depends on unrust ecosystem** - `uni-gl` and `uni-app` are niche crates with minimal community.

  If they break or become unmaintained, doryen-rs breaks. Most Rust game projects use `wgpu`,
  `winit`, or `glow` instead.

- **Color type is a bare tuple** - `type Color = (u8, u8, u8, u8)` provides no named fields, no

  methods, no type safety. Easy to mix up RGBA ordering. No predefined color constants.

- **No ECS integration** - No integration with Bevy, specs, or legion. Users must build their own

  game architecture on top.

- **Limited documentation** - docs.rs page returns "no such resource" for the latest version.

  Examples are the primary documentation. No tutorial beyond the examples.

- **Single developer, dormant** - One maintainer, no active development since 2023. Only 7 forks.

  Not a safe bet for long-term projects.

- **No audio** - Pure rendering library. Users need separate audio solutions.
- **Owns the game loop** - `App::run()` takes ownership and enters an event loop. Cannot be embedded

  in another framework's loop or used with Bevy/macroquad.

- **API surface is small** - No built-in pathfinding (FOV is a separate crate), no map generation,

  no GUI widgets, no tilemap layers. Compare to bracket-lib which bundles these.

- **Requires OpenGL** - No Vulkan/Metal/DirectX path. The `uni-gl` abstraction only covers

  OpenGL/WebGL.

## Sources

- Kept: [GitHub repo](https://github.com/jice-nospam/doryen-rs) - primary source for README,

  features, demos

- Kept: [crates.io/doryen-rs](https://crates.io/crates/doryen-rs) - package metadata, description
- Kept: [Source: console.rs](https://github.com/jice-nospam/doryen-rs/blob/master/src/console.rs) -

  full Console API

- Kept: [Source: app.rs](https://github.com/jice-nospam/doryen-rs/blob/master/src/app.rs) - Engine

  trait, game loop, DoryenApi

- Kept: [Source: program.rs](https://github.com/jice-nospam/doryen-rs/blob/master/src/program.rs) -

  GL rendering pipeline

- Kept:

  [Source: doryen_fs.glsl](https://github.com/jice-nospam/doryen-rs/blob/master/src/doryen_fs.glsl) -
  fragment shader

- Kept: [Source: img.rs](https://github.com/jice-nospam/doryen-rs/blob/master/src/img.rs) - image

  blitting, subcell

- Kept: [Source: color.rs](https://github.com/jice-nospam/doryen-rs/blob/master/src/color.rs) -

  color type and operations

- Kept: [CHANGELOG.md](https://github.com/jice-nospam/doryen-rs/blob/master/CHANGELOG.md) - version

  history

- Kept: [GitHub API](https://api.github.com/repos/jice-nospam/doryen-rs) - stars, forks, activity

  dates

- Kept: [crates.io/doryen-fov](https://crates.io/crates/doryen-fov) - companion FOV crate
- Dropped: docs.rs/doryen-rs - returns "no such resource" for both latest and 1.2.2 versions

## Gaps

- **Benchmark numbers** - No concrete FPS or throughput numbers from the perf example. The

  architecture (single draw call, flat array upload) should be fast, but no published benchmarks
  comparing it to bracket-lib or BearLibTerminal.

- **Community usage** - Could not find any shipped games, jam entries, or substantial projects using

  doryen-rs. Web search was unavailable due to API rate limits; a manual search of itch.io or
  roguelike jam archives might find some.

- **Comparison with bracket-lib** - bracket-lib (by Herbert Wolverson, author of "Hands-on Rust")

  has become the dominant Rust roguelike library. A direct feature/performance comparison would be
  valuable but was not available.

- **Future plans** - v1.4.0 exists on master but was never released to crates.io. No roadmap or

  indication of future development.
