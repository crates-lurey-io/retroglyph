# Research: Native OpenGL Backend for Rust Terminal/Grid Rendering

## Summary

OpenGL 3.3 core profile is the right minimum target: it covers all needed features (instanced
rendering, texture arrays, UBOs, VAOs) while running on virtually all hardware from 2010 onward,
including macOS (which caps at GL 4.1). The recommended rendering approach is **instanced rendering
with a glyph texture atlas** (one instanced draw call per frame), using `glow` for GL bindings and
`glutin` + `winit` for context/window management. This is the approach proven by beamterm,
alacritty, ghostty, and xterm.js's WebGL renderer, all achieving sub-millisecond render times for
10k-45k cell grids.

## Findings

### 1. Minimum OpenGL Version: GL 3.3 Core Profile

**GL 3.3 core is the consensus target.** All terminal renderers studied (beamterm, alacritty,
bracket-lib, ghostty) target GL 3.3 or its WebGL2 equivalent.

- GL 3.3 core provides everything needed: instanced rendering (`glDrawElementsInstanced`,

  `glVertexAttribDivisor`), 2D texture arrays (`GL_TEXTURE_2D_ARRAY`), uniform buffer objects,
  vertex array objects, and GLSL 330 shaders.
  [OpenGL 3.3 Core Spec](https://registry.khronos.org/OpenGL/specs/gl/glspec33.core.withchanges.pdf)

- GL 3.3 code is **upward compatible** with all GL 4.x core profiles. The OpenGL 4.0-4.6 core specs

  explicitly state upward compatibility with 3.3. GLSL 330 shaders work without modification on 4.x
  contexts.
  [Stack Exchange: GL 3.3 Compatibility with 4.x](https://gamedev.stackexchange.com/questions/124993/opengl-3-3-core-compatibility-with-opengl-4-x)

- **macOS caps at OpenGL 4.1** (deprecated since Catalina but still functional). Targeting GL 3.3

  ensures macOS works. GL 4.x features like compute shaders (4.3) or SSBO (4.3) are unavailable on
  macOS without Metal/MoltenVK.

- GL 3.3 covers Linux Mesa drivers from ~2012+, Windows from ~2010+ (any discrete GPU), and Intel HD

  2500+ integrated GPUs.

- **No reason to target GL 4.x** unless you need compute shaders. For a terminal grid renderer, GL

  3.3 is sufficient.

### 2. Rendering Approaches

Three distinct approaches exist in the wild. **Instanced rendering** is the clear winner for
performance.

#### a) Instanced Rendering (Recommended)

One quad geometry shared across all cells via `glDrawElementsInstanced`. Per-instance data (grid
position, glyph ID, fg/bg colors) is packed into instance VBOs. The vertex shader computes screen
position from grid coordinates; the fragment shader samples the glyph atlas.

- **beamterm**: Single draw call for the entire terminal (e.g., 200x80 = 16,000 cells). Uses 8 bytes

  per cell (16-bit glyph ID + 24-bit fg + 24-bit bg). Achieves sub-millisecond render times at 45k
  cells. Uses `u64` bitmask dirty tracking to minimize `bufferSubData` uploads.
  [beamterm](https://github.com/junkdog/beamterm)

- **alacritty** (current): Each glyph rendered as a separate instanced quad. The WIP PR #4373

  proposes a grid-based approach (rendering background as a full-screen grid, glyphs separately)
  that is 2-20x faster. Key insight: the grid shader covers cell backgrounds without needing
  separate quads per cell. [Alacritty PR #4373](https://github.com/alacritty/alacritty/pull/4373)

- **ghostty**: Uses instanced rendering with per-cell vertex data. Vertex shader unpacks grid

  position and cell size from packed uniforms. Renders quads via triangle strip (4 vertices per
  quad).
  [Ghostty cell_text.v.glsl](https://github.com/ghostty-org/ghostty/blob/main/src/renderer/shaders/glsl/cell_text.v.glsl)

**Buffer layout** (beamterm reference):

| Buffer            | Type | Size         | Update Freq | Purpose                    |
| ----------------- | ---- | ------------ | ----------- | -------------------------- |
| Vertex            | VBO  | 64 bytes     | Never       | Quad geometry (4 vertices) |
| Index             | IBO  | 6 bytes      | Never       | Triangle indices           |
| Instance Position | VBO  | 4 bytes/cell | On resize   | Grid coordinates           |
| Instance Cell     | VBO  | 8 bytes/cell | Per frame   | Glyph ID + colors          |
| Vertex UBO        | UBO  | 80 bytes     | On resize   | Projection matrix          |
| Fragment UBO      | UBO  | 32 bytes     | On resize   | Cell metadata              |

#### b) Per-Cell Textured Quads (Simple but Slower)

Build vertex buffers with 4 vertices per cell, each with position, color, and texture coordinates.
Upload the entire VBO each frame (or use sub-buffer updates). One or two draw calls total.

- **bracket-lib** uses this approach: per-vertex data includes position (vec3), fg color (vec4), bg

  color (vec4), and tex coords (vec2). Rebuilds vertex arrays each frame. Uses GL 3.3 core, GLSL 330
  shaders. Two render passes: one with background, one without (for layering).
  [bracket-lib shader_strings.rs](https://github.com/amethyst/bracket-lib/blob/master/bracket-terminal/src/hal/native/shader_strings.rs)

- Simpler to implement but produces far more vertex data. For an 80x50 grid: 4,000 cells x 4

  vertices x ~52 bytes/vertex = ~800KB per frame upload vs. beamterm's 4,000 x 8 bytes = 32KB.

#### c) Full-Screen Shader (doryen-rs approach)

Render the entire terminal as a single full-screen quad. Pass cell data as textures (one for glyphs,
one for fg colors, one for bg colors). The fragment shader computes which cell each pixel belongs to
and samples the glyph atlas.

- **doryen-rs**: Uses `uni-gl` (an OpenGL/WebGL abstraction). Renders the console as a single

  full-screen quad with a GLSL fragment shader that reads cell data from textures. Very minimal
  CPU-side work. [doryen-rs](https://github.com/jice-nospam/doryen-rs)

- Pros: Minimal draw calls (literally 1), no instancing needed, works on GL ES 2.0.
- Cons: All logic in fragment shader, harder to debug/extend, per-pixel branching may be slower on

  older GPUs, limited flexibility for per-cell effects (offsets, rotation).

### 3. Rust OpenGL Crates

#### glow (Recommended)

- Thin, unsafe wrapper over OpenGL/OpenGL ES/WebGL. Maps 1:1 to GL calls via a `Context` trait.

  Supports native (via `gl` crate or raw loader) and WASM (via `web_sys`).

- 1.5M downloads/month, used by egui, beamterm, and many others. Latest: v0.17.0 (March 2026).
- Works with any windowing system that provides a GL context (glutin, SDL2, GLFW).
- Best choice for a library: no opinions about resource management, caller controls everything.
- [glow on crates.io](https://crates.io/crates/glow)

#### glium

- Higher-level safe wrapper. Handles buffer binding, state tracking, RAII for GL objects. Tightly

  coupled with glutin for context creation.

- No longer actively maintained by original author. Community-maintained.
- Good for applications, but too opinionated for a library backend. Forces its own buffer/texture

  types.

- [glium on GitHub](https://github.com/glium/glium)

#### Raw `gl` / `gl_generator`

- Direct 1:1 C bindings via `gl_generator`. Maximum control, maximum boilerplate.
- Good for `learnopengl.com`-style tutorials; not practical for a library that also targets WebGL.
- [Learn OpenGL in Rust](https://rust-tutorials.github.io/learn-opengl/basics/index.html)

**Recommendation**: `glow` is the clear winner. It provides cross-platform GL/WebGL from a single
API, is actively maintained, and is thin enough that you control all GPU state directly. beamterm
proves this works well for exactly this use case.

### 4. Windowing Integration: winit + glutin

The standard Rust OpenGL windowing stack:

1. **winit** - Cross-platform window creation and event loop. Provides `RawWindowHandle` /

   `RawDisplayHandle` for GL context creation. Does not handle GL contexts directly.
   [winit docs](https://docs.rs/winit)

1. **glutin** - Low-level OpenGL context creation library. Creates GL contexts from raw

   window/display handles. Handles platform differences (EGL, WGL, CGL, GLX).
   [glutin on GitHub](https://github.com/rust-windowing/glutin)

1. **glutin-winit** - Glue crate that connects winit windows to glutin GL contexts. Provides

   `DisplayBuilder` for simplified bootstrapping. [glutin-winit docs](https://docs.rs/glutin-winit)

**Typical initialization flow** (from beamterm's native examples):

```rust
// 1. Create event loop and window via winit
let event_loop = EventLoop::new()?;
let window_builder = WindowBuilder::new();

// 2. Create GL display + window via glutin-winit
let display_builder = DisplayBuilder::new().with_window_builder(Some(window_builder));
let (window, gl_config) = display_builder.build(&event_loop, ...)?;

// 3. Create GL context
let gl_context = unsafe {
    gl_display.create_context(&gl_config, &context_attrs)?
};

// 4. Create glow::Context from the GL loader
let gl = unsafe {
    glow::Context::from_loader_function(|s| gl_display.get_proc_address(s) as *const _)
};
```

**For a library**: Accept a `glow::Context` from the caller rather than managing windows. This lets
users bring their own windowing (SDL2, GLFW, etc.). Provide example code with glutin+winit but don't
depend on them in the core crate.

### 5. Glyph Texture Atlas Construction

Two approaches, both proven in production:

#### Static Atlas (Build-Time)

Pre-rasterize all needed glyphs into a texture atlas at build time. Ship the atlas as a binary
asset.

- **beamterm-atlas**: CLI tool that rasterizes TTF/OTF fonts into a binary `.atlas` format. Packs

  glyphs into a GL 2D texture array (each layer = 1x32 grid of glyphs). Supports
  Normal/Bold/Italic/BoldItalic styles and emoji. ASCII characters use direct bit-manipulation
  lookup (char_code | style_bits) for zero-overhead glyph ID resolution.

- **Pros**: Zero runtime rasterization cost, deterministic atlas layout, optimal texture packing.
- **Cons**: Fixed glyph set, HiDPI requires snapped scaling (0.5x, 1x, 2x, 3x).

#### Dynamic Atlas (Runtime)

Rasterize glyphs on-demand when first encountered. Use LRU eviction when atlas fills up.

- **Native rasterization crates**:
  - `fontdue` - Pure Rust, fast, lightweight. Good for monospace grid rendering. No shaping.
  - `ab_glyph` - Pure Rust, based on `ttf-parser`. More features than fontdue.
  - `swash` - High-quality rasterizer with hinting, used by beamterm's native dynamic atlas via

    `swash` + `fontdb` for font discovery.

  - `cosmic-text` - Full text layout engine (shaping + rasterization). Overkill for grid rendering.

- **beamterm dynamic atlas**: Uses `swash` + `fontdb` on native, Canvas API on WASM. ASCII Normal

  pre-allocated in fixed slots (0-94), all other glyphs in LRU-managed slots. Re-rasterizes at new
  DPR on display change.

**Atlas texture format**: GL `TEXTURE_2D_ARRAY` is ideal for grid renderers. Each layer stores a row
of 32 glyphs. Layer index = glyph_id / 32, position = glyph_id % 32. Adjacent ASCII characters share
layers, maximizing texture cache coherence.

**Packing for monospace**: Monospace fonts simplify packing enormously. All glyphs fit within a
fixed cell_width x cell_height rectangle. No need for complex bin-packing; just use a uniform grid
within each texture layer.

### 6. BearLibTerminal's OpenGL Renderer

BearLibTerminal (C++) uses OpenGL for "high performance" rendering of a cell grid with tile
composition:

- **Cell model**: Grid of cells, each cell can hold a stack of tiles. Multiple layers. Each tile has

  its own color and offset.

- **Tileset system**: Bitmap tilesets (spritesheet images sliced by cell size), TrueType tilesets

  (rasterized via FreeType at load time), or individual tile images. All tiles assigned to Unicode
  codepoint slots (BMP, ~65k slots).

- **Codepage mapping**: Handles CP437 and other legacy codepages by mapping tileset indices to

  Unicode codepoints. TrueType fonts use built-in Unicode cmaps.

- **Texture atlas**: Tiles packed into atlas textures. Configurable `output.texture-filter`

  (linear/nearest). Atlas can be dynamically regenerated when fonts/tilesets change.

- **Rendering**: Renders by iterating over visible cells, compositing tile stacks with alpha

  blending. Uses OpenGL textured quads with per-tile coloring.

- [BearLibTerminal Design](http://foo.wyrd.name/en:bearlibterminal:design),

  [BearLibTerminal Source](https://github.com/cfyzium/bearlibterminal)

**Key takeaway for retroglyph**: BearLibTerminal's tile-stacking model (multiple tiles per cell with
individual offsets/colors) is more flexible than most terminal renderers need. Its codepage/Unicode
mapping system is well-designed for CP437 roguelike fonts. The renderer itself is straightforward
GL2-era textured quads, not optimized with instancing.

### 7. bracket-lib's OpenGL Backend

bracket-lib (Rust) provides a virtual CP437/ASCII terminal with OpenGL, WebGPU, curses, and
crossterm backends:

- **GL version**: GLSL 330 core (`#version 330 core` in all shaders).
- **Architecture**: HAL layer in `bracket-terminal/src/hal/`. Native GL code in `hal/native/`, uses

  raw `gl` bindings (not glow).

- **Rendering approach**: Per-cell vertex data. Each cell becomes 2 triangles (6 indices, 4

  vertices). Vertex attributes: position (vec3), fg color (vec4), bg color (vec4), tex coords
  (vec2).

- **Shader types**:
  - `CONSOLE_WITH_BG`: Background fills where glyph alpha is low, foreground where glyph is visible.
  - `CONSOLE_NO_BG`: Discards pixels where glyph is dark (for transparent overlay layers).
  - `FANCY_CONSOLE`: Same as WITH_BG but adds per-vertex rotation and scale.
  - `SPRITE_CONSOLE`: For sprite rendering with transform.
  - `SCANLINES`: Post-process effect with CRT scanlines and screen burn.
  - `BACKING`: Simple passthrough for final framebuffer blit.
- **Render pipeline**: Renders each console layer to a framebuffer texture, then composites layers

  by blitting framebuffer textures with the BACKING shader.

- **Font atlas**: Uses CP437 bitmap font atlas loaded as a single GL texture. Tex coords computed

  from character code.

- [bracket-lib](https://github.com/amethyst/bracket-lib),

  [shader_strings.rs](https://github.com/amethyst/bracket-lib/blob/master/bracket-terminal/src/hal/native/shader_strings.rs)

**Key takeaway for retroglyph**: bracket-lib is functional but not performance-optimized. The
per-vertex approach rebuilds all geometry each frame. For a new implementation, instanced rendering
would be strictly better. The multi-layer framebuffer compositing pattern is worth adopting if layer
support is needed.

### 8. Performance Considerations

#### Minimize Draw Calls

- **One instanced draw call** for the entire grid is achievable and proven (beamterm, ghostty).

  `glDrawElementsInstanced(GL_TRIANGLES, 6, GL_UNSIGNED_BYTE, null, cell_count)`.

- If multiple layers are needed, one draw call per layer (still very few).

#### Minimize GPU Uploads

- **Dirty tracking**: Use a bitmask to track which cell chunks changed since last frame. Only upload

  dirty ranges via `glBufferSubData`. beamterm uses a `u64` bitmask for 1024-cell chunks. For an
  80x50 terminal (4000 cells), only 4 bits needed.

- **Static buffers**: Grid positions only change on resize. Mark as `GL_STATIC_DRAW`. Only the cell

  data buffer (glyph + colors) is `GL_DYNAMIC_DRAW`.

#### Pack Instance Data Tightly

- 8 bytes per cell is sufficient: 2 bytes glyph ID + 3 bytes fg RGB + 3 bytes bg RGB.
- Unpack in vertex shader to avoid per-fragment cost. Use `flat` varyings for colors (no

  interpolation needed).

- ASCII characters in first ~4 texture layers provides good cache locality.

#### Minimize State Changes

- Single VAO, single texture (atlas), single shader program. Bind once per frame, draw once.
- Use UBOs for projection matrix and grid metadata (change only on resize).
- Avoid `glUniform*` calls per-cell; pack everything into instance attributes or UBOs.

#### Atlas Texture Format

- `TEXTURE_2D_ARRAY` with `GL_R8` format for grayscale glyphs (1 byte/pixel) or `GL_RGBA8` for color

  emoji.

- Array textures avoid texture switches between glyph pages. All glyphs in one bind.
- `GL_NEAREST` filtering for pixel-perfect rendering at 1:1 scale; `GL_LINEAR` for fractional

  scaling.

#### Benchmarks from Real Systems

| System                            | Cells         | Render Time                      | Draw Calls |
| --------------------------------- | ------------- | -------------------------------- | ---------- |
| beamterm (WebGL2, 2019 HW)        | 45,156        | <1ms                             | 1          |
| beamterm (WebGL2, low-end target) | 16,000        | <1ms                             | 1          |
| alacritty (new renderer, OpenGL)  | 10,000-30,000 | 2-20x faster than old            | N/A        |
| xterm.js WebGL renderer           | ~10,000       | Significantly faster than canvas | 1          |

### 9. Trade-offs: OpenGL vs wgpu/Vulkan/Metal

| Aspect                  | OpenGL 3.3                                                                                 | wgpu                                                                          | Raw Vulkan/Metal               |
| ----------------------- | ------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------- | ------------------------------ | --------------------------------------------------------------------- |
| **Platform coverage**   | Windows, Linux, macOS (deprecated), WebGL2                                                 | Windows, Linux, macOS, Web (WebGPU), Android, iOS                             | Per-platform only              |
| **API complexity**      | Low. ~200 LOC for a working renderer                                                       | Medium. Explicit pipeline setup, bind groups                                  | Very high                      |
| **Compile times**       | Fast. `glow` is thin                                                                       | Slow. Large dependency tree                                                   | Medium-high                    |
| **Binary size**         | Small                                                                                      | Large (+wgpu, naga, etc.)                                                     | Medium                         |
| **Performance ceiling** | Sufficient for 2D grid rendering. Not bottleneck.                                          | Higher theoretical throughput, explicit sync                                  | Highest                        |
| **macOS status**        | Deprecated (GL 4.1 max) but works. Apple ships GL drivers with macOS through at least 2026 | Metal backend, first-class                                                    | Metal is Apple's preferred API |
| **WebGL support**       | WebGL2 via same `glow` code                                                                | WebGPU (still rolling out in browsers) or WebGL2 fallback                     | N/A                            |
| **Maintenance**         | Stable, no API churn                                                                       | Frequent breaking changes (wgpu 22 was 23% slower than 0.20 in one benchmark) | Stable but verbose             |
| **Safety**              | All `glow` calls are `unsafe`                                                              | Safe Rust API                                                                 | Unsafe everywhere              |
| **Multi-threading**     | Single-thread only (GL context bound to one thread)                                        | Multi-threaded by design                                                      | Multi-threaded                 | **For a terminal/grid renderer**, OpenGL 3.3 is the pragmatic choice: |

- The workload (a few thousand textured quads) is trivially simple for any GPU. Performance

  differences between APIs are irrelevant at this scale.

- OpenGL + glow gives the widest platform coverage with the least code. Same codebase covers desktop

  and WebGL2.

- wgpu adds significant dependency weight and compile time for no performance benefit in this use

  case. Its API instability (breaking changes between minor versions) is a real maintenance cost.

- If macOS deprecation becomes a real problem (Apple actually removes GL), the migration path is to

  wgpu or Metal. But Apple has shown no signs of removing GL yet.

**Hybrid approach** (as seen in beamterm): Write the renderer against `glow::Context`. This works on
both OpenGL 3.3 (native) and WebGL2 (WASM) with zero platform-specific rendering code. Windowing is
the only platform-specific layer.

## Sources

- **Kept**: beamterm README and architecture docs (<https://github.com/junkdog/beamterm>) - The most

  directly relevant reference. Sub-millisecond terminal renderer targeting GL 3.3 / WebGL2,
  single-codebase via glow, instanced rendering, comprehensive atlas system. Powers Ratzilla's
  WebGL2 backend.

- **Kept**: Alacritty PR #4373 - New faster renderer

  (<https://github.com/alacritty/alacritty/pull/4373>) - Detailed technical discussion comparing
  instanced quad rendering vs grid-based full-screen shader approach. Benchmarks showing 2-20x
  speedup.

- **Kept**: bracket-lib shader_strings.rs

  (<https://github.com/amethyst/bracket-lib/blob/master/bracket-terminal/src/hal/native/shader_strings.rs>) -
  Complete GLSL 330 shader source for a production terminal renderer. Shows per-vertex approach and
  multi-layer compositing.

- **Kept**: BearLibTerminal design docs (<http://foo.wyrd.name/en:bearlibterminal:design>) -

  Detailed tileset/atlas architecture, codepage mapping, tile stacking model.

- **Kept**: Ghostty cell_text.v.glsl

  (<https://github.com/ghostty-org/ghostty/blob/main/src/renderer/shaders/glsl/cell_text.v.glsl>) -
  Production vertex shader for instanced cell rendering.

- **Kept**: OpenGL 3.3 Core Spec

  (<https://registry.khronos.org/OpenGL/specs/gl/glspec33.core.withchanges.pdf>) - Authoritative
  feature list for GL 3.3.

- **Kept**: glow crate (<https://lib.rs/crates/glow>) - API docs and usage patterns for the

  recommended GL bindings crate.

- **Kept**: glutin + glutin-winit docs (<https://docs.rs/glutin-winit>,

  <https://github.com/rust-windowing/glutin>) - Standard GL context creation for Rust.

- **Kept**: doryen-rs (<https://github.com/jice-nospam/doryen-rs>) - Example of full-screen shader

  approach for roguelike console rendering.

- **Kept**: xterm.js WebGL renderer PR (<https://github.com/xtermjs/xterm.js/pull/1790>) - Documents

  the Float32Array + shader approach for terminal rendering in WebGL.

- **Kept**: Handmade Network OpenGL font rendering tutorial

  (<https://handmade.network/forums/articles/t/3092-tutorial_opengl_font_rendering>) - Practical
  instanced font rendering implementation guide.

- **Kept**: wgpu issues #6434, #6688 (<https://github.com/gfx-rs/wgpu/issues/6434>,

  <https://github.com/gfx-rs/wgpu/discussions/6688>) - Evidence of wgpu performance regressions
  between versions.

- **Dropped**: LogRocket wgpu tutorial - Marketing blog post, no terminal-specific content.
- **Dropped**: dma9527/terminal-emulator - Incomplete project, no unique insights beyond "use wgpu".
- **Dropped**: luminance comparison blog - Dated comparison, luminance has low adoption.
- **Dropped**: ori-term - Alpha-stage terminal emulator, no published renderer details.

## Gaps

1. **doryen-rs shader source**: The actual GLSL shader implementing the full-screen terminal

   rendering approach was not directly accessible (it's in the `uni-gl` abstraction layer). To
   evaluate this approach fully, the shader code from `doryen-rs/src/shaders/` would need
   examination.

1. **BearLibTerminal renderer C++ source**: The actual OpenGL draw calls in BearLibTerminal were not

   examined at the source level. The design docs describe the data model well but not the specific
   GL techniques (batching strategy, number of draw calls, etc.).

1. **fontdue vs swash vs ab_glyph benchmarks**: No direct benchmarks were found comparing these

   rasterizers for monospace glyph atlas generation. For a grid renderer, any of them should be fast
   enough since rasterization happens once per glyph.

1. **GL 3.3 on Wayland**: Some Wayland compositors may have quirks with GL context creation via EGL.

   This would need testing with glutin's EGL backend.

1. **Subpixel/LCD rendering**: None of the studied renderers use subpixel rendering in their GL

   paths (alacritty does it in its existing renderer, but the new grid-based PR does not). Whether
   LCD antialiasing is worth the complexity for a grid renderer is an open question.
