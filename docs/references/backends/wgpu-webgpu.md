# Research: wgpu/WebGPU Backend for a Rust Terminal/Grid Rendering Library

## Summary

wgpu is a mature, pure-Rust graphics API based on the WebGPU standard that runs natively on Vulkan,
Metal, DX12, and OpenGL, plus WebGPU/WebGL2 in browsers. For terminal/grid rendering, the proven
approach is instanced quads with a dynamic glyph atlas texture: each cell becomes a textured quad
(two triangles), glyphs are rasterized on demand into a GPU texture atlas, and the whole grid draws
in 2-3 draw calls per frame. Multiple production terminal emulators (Rio, par-term, ratatui-wgpu)
already use this architecture with wgpu in Rust, validating the approach.

## 1. wgpu Architecture

The wgpu API follows a layered object model derived from the WebGPU specification:

| Object           | Role                                                                                                                                         |
| ---------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| `Instance`       | Entry point. Discovers GPUs, creates Adapters and Surfaces.                                                                                  |
| `Surface`        | The window's drawable area (wraps a platform window handle).                                                                                 |
| `Adapter`        | Handle to a physical GPU. Used to query capabilities and create a Device. Does not need to be kept alive.                                    |
| `Device`         | Open connection to the GPU. Creates buffers, textures, pipelines, shaders. The main workhorse.                                               |
| `Queue`          | Submission point for command buffers. You record commands into a `CommandEncoder`, finish it into a `CommandBuffer`, then `queue.submit()`.  |
| `RenderPipeline` | Immutable, pre-compiled pipeline state: vertex/fragment shaders, blend modes, vertex layout, pixel format. Created once, reused every frame. |
| `BindGroup`      | Groups of resources (textures, samplers, uniform buffers) bound to shader slots.                                                             |
| `CommandEncoder` | Records GPU commands (render passes, copies). Produces a `CommandBuffer`.                                                                    |

### Initialization flow

```text
Instance::new()
  -> instance.create_surface(&window)
  -> instance.request_adapter(compatible_surface)
  -> adapter.request_device()
  -> device.create_render_pipeline()
  -> loop {
       surface.get_current_texture()
       device.create_command_encoder()
       encoder.begin_render_pass()
       render_pass.set_pipeline()
       render_pass.draw()
       encoder.finish()
       queue.submit()
       surface_texture.present()
     }
```text

The `Surface` is configured with size, format, and present mode (`Fifo` for vsync, `Mailbox` for
low-latency). On resize, call `surface.configure()` again.

### Key design properties

- All resource creation is validated at the API level (safe Rust, no `unsafe` for users).
- Pipeline state is immutable and pre-compiled, unlike OpenGL's mutable global state.
- Command recording is separate from submission, enabling multi-threaded command building.
- wgpu validates everything internally; errors are reported through callbacks, not GPU crashes.

[Source: wgpu docs](https://docs.rs/wgpu/) |
[Source: Learn Wgpu tutorial](https://sotrh.github.io/learn-wgpu/beginner/tutorial2-surface/)

## 2. Rendering a Cell Grid with wgpu

Three main approaches exist for rendering a terminal cell grid on the GPU. All have been used in
practice.

### Approach A: Instanced Quads (Recommended)

Each cell is one instanced draw of a unit quad. Per-instance data carries position,
foreground/background color, and glyph atlas UV coordinates.

### Instance data per cell (~32-48 bytes)

```rust
#[repr(C)]
struct CellInstance {
    pos: [f32; 2],        // pixel position of cell
    size: [f32; 2],       // cell width/height
    fg: [f32; 4],         // foreground RGBA
    bg: [f32; 4],         // background RGBA
    uv_offset: [f32; 2],  // glyph atlas UV origin
    uv_size: [f32; 2],    // glyph atlas UV extent
    flags: u32,           // bold, italic, underline, etc.
}
```

**Draw pattern:** Two draw calls per frame:

1. Background pass: draw all cell backgrounds (solid color, no texture).
2. Text pass: draw all cells with glyphs (sample from atlas texture, use glyph alpha as mask, apply

   foreground color).

3. Optional: color emoji pass with a separate RGBA atlas.
A 200x50 terminal = 10,000 instances. Modern GPUs handle millions of instances trivially.

**Advantages:** Minimal CPU work per frame; only rebuild the instance buffer when cells change.
Dirty-row tracking (a 256-bit bitset, one bit per row) means only changed rows need instance data
updates. GPU cost is nearly constant regardless of cell count.

### wgpu instancing API

```rust
render_pass.set_vertex_buffer(0, quad_vertex_buffer.slice(..));  // unit quad (4 verts)
render_pass.set_vertex_buffer(1, instance_buffer.slice(..));     // per-cell data
render_pass.set_index_buffer(index_buffer.slice(..), IndexFormat::Uint16);
render_pass.draw_indexed(0..6, 0, 0..cell_count);
```

The vertex shader reads both the shared quad vertices (slot 0) and the per-instance data (slot 1),
transforms position to NDC, and passes UVs + colors to the fragment shader.

[Source: Learn Wgpu Instancing](https://sotrh.github.io/learn-wgpu/beginner/tutorial7-instancing/) |
[Source: Attyx GPU rendering blog](https://semos.sh/blog/attyx-gpu-rendering/)

### Approach B: Pre-built Vertex Buffer

Build 6 vertices (two triangles) per cell on the CPU and upload the entire vertex buffer. This is
what Attyx (Zig terminal) and Alacritty (OpenGL) do.

```c
// Per vertex: 32 bytes
struct Vertex {
    float px, py;       // pixel position
    float u, v;         // texture coordinates
    float r, g, b, a;   // color
};
// 6 vertices per cell = 192 bytes per cell
// 200x50 terminal = ~1.8 MB vertex data
```

**Advantages:** Simpler shader (no instance buffer layout). Works on all hardware, including WebGL2
fallback. **Disadvantages:** More CPU work rebuilding vertices. Larger GPU uploads. But for terminal
workloads (10K-50K cells), the difference is negligible.

[Source: Attyx GPU rendering](https://semos.sh/blog/attyx-gpu-rendering/)

### Approach C: Compute Shader Grid

Use a compute shader to read a cell buffer (stored as a storage buffer) and write directly to a
render target or generate vertices. This is the most GPU-driven approach.

### Advantages:**Minimal CPU involvement; the grid state lives in a GPU buffer.**Disadvantages

Requires compute shader support (not available on WebGL2). More complex synchronization. Overkill
for typical terminal grid sizes.

### Recommendation

**Instanced quads (Approach A)** is the best balance for a terminal grid renderer. It minimizes
per-frame CPU work, plays well with dirty tracking, and is the pattern used by ratatui-wgpu and
Rio's Sugarloaf. The pre-built vertex buffer approach (B) is a fine fallback for WebGL2
compatibility.

## 3. Shader Language: WGSL and Naga

### WGSL (WebGPU Shading Language)

WGSL is the native shader language for WebGPU/wgpu. It is the only shader language that works on all
wgpu backends, including browsers via WebGPU.

```wgsl
// Vertex shader for cell rendering
@vertex
fn vs_main(
    @location(0) quad_pos: vec2<f32>,    // unit quad vertex
    @location(1) cell_pos: vec2<f32>,    // instance: cell pixel position
    @location(2) cell_size: vec2<f32>,   // instance: cell dimensions
    @location(3) uv_offset: vec2<f32>,   // instance: atlas UV
    @location(4) fg_color: vec4<f32>,    // instance: foreground
) -> VertexOutput {
    var out: VertexOutput;
    let pixel_pos = cell_pos + quad_pos * cell_size;
    out.position = vec4<f32>(
        pixel_pos / viewport * 2.0 - 1.0,
        0.0, 1.0
    );
    out.position.y = -out.position.y;
    out.uv = uv_offset + quad_pos * uv_size;
    out.color = fg_color;
    return out;
}

// Fragment shader for text (grayscale atlas)
@fragment
fn fs_text(in: VertexOutput) -> @location(0) vec4<f32> {
    let alpha = textureSample(glyph_atlas, atlas_sampler, in.uv).r;
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
```

### Naga (Shader Translation)

Naga is wgpu's built-in shader translator and validator. It is part of the wgpu monorepo and handles
all shader compilation for native backends.

| Front-end (input) | Status                           |
| ----------------- | -------------------------------- |
| WGSL              | Fully supported, fully validated |
| SPIR-V (binary)   | Fully supported                  |
| GLSL (440+)       | Supported (Vulkan semantics)     |

| Back-end (output)            | Status                                          |
| ---------------------------- | ----------------------------------------------- |
| SPIR-V                       | Fully supported (for Vulkan)                    |
| MSL (Metal Shading Language) | Fully supported (for Metal)                     |
| HLSL                         | Fully supported, SM 5.0+ (for DX12)             |
| GLSL                         | Fully supported (for OpenGL/WebGL2)             |
| WGSL                         | Supported (pass-through for WebGPU in browsers) |

**How it works at runtime:** You write shaders in WGSL. When targeting Vulkan, naga translates WGSL
to SPIR-V. When targeting Metal, naga translates to MSL. When targeting DX12, naga translates to
HLSL. When running in a browser with WebGPU, WGSL is passed through directly. This is all automatic
and invisible to the user of the wgpu API.

**Practical recommendation:** Write all shaders in WGSL. It is the only language guaranteed to work
on every backend. Use
`device.create_shader_module(wgpu::ShaderModuleDescriptor { source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()), .. })`.
Naga handles the rest.

If you have existing GLSL or SPIR-V shaders, naga can consume those too, but WGSL is the first-class
path.

[Source: naga README](https://github.com/gfx-rs/wgpu/tree/trunk/naga) |
[Source: naga docs](https://docs.rs/naga/latest/naga/)

## 4. Glyph Atlas Texture Management

The glyph atlas is the critical data structure for GPU text rendering. Every production GPU terminal
uses one.

### Architecture

1. **Rasterize on demand:** The first time a (codepoint, font_id, size, style) combination is

   needed, rasterize it to a CPU bitmap using the platform text engine (Core Text on macOS, FreeType
   on Linux, DirectWrite on Windows, or cross-platform via `cosmic-text`/`rustybuzz`).

1. **Pack into atlas texture:** Use a rectangle packing algorithm (e.g., `etagere` shelf packer) to

   find a free slot in the GPU texture. Upload the bitmap to that slot via `queue.write_texture()`.

1. **Record UV coordinates:** Store a mapping from (codepoint, font_id, size, style) to

   (atlas_texture_index, uv_rect) in a hash table.

1. **Sample in fragment shader:** The fragment shader samples the atlas at the UV coordinates and

   uses the result as alpha (for grayscale glyphs) or as direct color (for emoji).

### Atlas layout

Two common layouts:

**Fixed-size slots (monospace optimization):** For terminal rendering with a monospace font, all
glyph cells are the same size. The atlas becomes a simple grid of `glyph_w x glyph_h` slots. Slot
index maps directly to UV coordinates:

```text
u0 = (slot % cols) * glyph_w / atlas_w
v0 = (slot / cols) * glyph_h / atlas_h
```rust

This is what Attyx does. Simple, fast lookup, but wastes space for proportional or variable-width
glyphs.

**Dynamic rectangle packing (general case):** Use `etagere` (shelf-based) or `guillotiere`
(guillotine-based) to pack variably-sized glyph bitmaps. This is what glyphon and ratatui-wgpu use.
More flexible, handles mixed font sizes and emoji.

### Dual atlas textures

Most implementations use two textures:

- **Grayscale atlas** (`R8Unorm`): For regular text. Each pixel stores glyph coverage (0.0 =

  transparent, 1.0 = opaque). Fragment shader multiplies by foreground color.

- **Color atlas** (`Rgba8Unorm`): For color emoji and COLR/CBDT font glyphs. Fragment shader uses

  the texture color directly.

### Eviction and growth

- **Growth:** When the atlas fills up, either double its size (reallocate and re-upload) or allocate

  a second atlas texture. Most terminals never fill a 2048x2048 or 4096x4096 atlas in normal use.

- **LRU eviction:** For long-running sessions with many unique glyphs, use LRU to evict stale

  entries. ratatui-wgpu uses `evictor` for this.

- **Platform limits:** Some GPUs (notably mobile/WebGL2) limit textures to 2048x2048. Rio handles

  this by capping atlas size to `min(4096, device.max_texture_dimension_2d)`.

### Key crates for glyph atlas

| Crate         | Role                                                                                                                                  |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| `glyphon`     | Complete wgpu text renderer: cosmic-text for shaping + etagere for packing + wgpu for rendering. 700+ stars, used by iced, egui, etc. |
| `cosmic-text` | Pure-Rust text shaping and layout (uses rustybuzz + swash).                                                                           |
| `etagere`     | Shelf-based rectangle atlas allocator.                                                                                                |
| `rustybuzz`   | Pure-Rust port of HarfBuzz text shaper.                                                                                               |
| `swash`       | Glyph rasterization and font introspection.                                                                                           |

### Sub-pixel positioning

For proportional text, sub-pixel glyph positioning matters. Warp's approach: rasterize each glyph at
N sub-pixel offsets (e.g., 3: 0.0, 0.33, 0.66 pixels) and cache each variant. For monospace terminal
grids, glyphs always land on pixel boundaries, so this is unnecessary.

[Source: Warp glyph atlas blog](https://www.warp.dev/blog/adventures-text-rendering-kerning-glyph-atlases)
| [Source: Attyx GPU rendering](https://semos.sh/blog/attyx-gpu-rendering/) |
[Source: glyphon](https://github.com/grovesNL/glyphon) |
[Source: tchayen WebGPU text](https://tchayen.com/drawing-text-in-webgpu-using-just-the-font-file)

## 5. Cross-Platform Support

wgpu's backend matrix:

| Backend | Windows               | Linux                    | macOS        | iOS          | Android    | Web (WASM)           |
| ------- | --------------------- | ------------------------ | ------------ | ------------ | ---------- | -------------------- |
| Vulkan  | 1st class             | 1st class                | Via MoltenVK | Via MoltenVK | 1st class  | N/A                  |
| Metal   | N/A                   | N/A                      | 1st class    | 1st class    | N/A        | N/A                  |
| DX12    | 1st class             | N/A                      | N/A          | N/A          | N/A        | N/A                  |
| OpenGL  | GL 3.3+ (best effort) | GL ES 3.0+ (best effort) | Via ANGLE    | N/A          | GL ES 3.0+ | WebGL2 (best effort) |
| WebGPU  | N/A                   | N/A                      | N/A          | N/A          | N/A        | 1st class            |

**Key points:**- On**macOS/iOS**, Metal is the native and preferred backend. Vulkan works via MoltenVK but adds a
  translation layer.

- On **Linux**, Vulkan is preferred. Mesa provides Vulkan drivers for AMD, Intel, and (via lavapipe)

  CPU fallback. OpenGL (GL ES 3.0+) is the fallback.

- On **Windows**, both Vulkan and DX12 are first-class. DX12 is often preferred for broader hardware

  coverage.

- On **Android**, Vulkan is preferred (Android 7.0+). GL ES 3.0+ is the fallback.
- On **Web/WASM**, WebGPU is the preferred backend (Chrome 113+, Firefox nightly, Safari TP). WebGL2

  is the fallback for older browsers.

- Backend selection is automatic at runtime. wgpu picks the best available backend unless overridden

  via `WGPU_BACKEND` env var.

### Browser WebGPU status (as of mid-2025)

- Chrome/Edge: Shipped and stable since Chrome 113
- Firefox: Behind a flag, progressing (wgpu-core powers Firefox's WebGPU)
- Safari: WebGPU supported in Safari 18+

**Windowing integration:** wgpu does not create windows. Use `winit` (cross-platform) or
platform-specific window libraries. On web, winit creates a `<canvas>` element. `Surface` creation
requires a raw window handle (`raw-window-handle` crate).

[Source: wgpu README](https://github.com/gfx-rs/wgpu)

## 6. Prior Art in Rust

### Rio Terminal (raphamorim/rio) - 7K stars

The most mature wgpu-based terminal emulator in Rust. Uses its own rendering engine called
**Sugarloaf**.

- Built on wgpu with WGSL shaders
- Sugarloaf handles font loading (custom loader, previously cosmic-text), glyph atlas management,

  and cell grid rendering

- Atlas sizes capped at `min(4096, max_texture_dimension_2d)` for portability
- Supports both desktop (native) and web (WASM/WebGPU)
- Uses VTE parser from Alacritty
- Font cache uses LRU eviction with a unified `CachedTextRun` structure
- Supports box-drawing/block element rendering as atlas sprites (not font glyphs)
- Supports RetroArch-compatible post-processing shaders via librashader

[Source: Rio terminal](https://rioterm.com/) |
[Source: sugarloaf crate](https://crates.io/crates/sugarloaf)

### ratatui-wgpu (Jesterhearts/ratatui-wgpu)

A wgpu rendering backend for ratatui, targeting both desktop and web.

- Renders ratatui's cell grid to a wgpu surface
- Uses rustybuzz for text shaping, raqote for path rendering
- Supports custom post-processing shaders (CRT effects, etc.)
- ~800 FPS at 1080p updating every cell every frame with CRT shader
- ~3,750 unique glyph cache capacity at default font size
- Dirty cell tracking via bitvec
- Supports mixed bidi text and combining sequences
- Explicit web/WASM support with worker thread rendering

[Source: ratatui-wgpu](https://github.com/jesterhearts/ratatui-wgpu)

### par-term-render

GPU-accelerated rendering engine for the par-term terminal emulator. All rendering via wgpu.

- `CellRenderer` for terminal grid with glyph atlas
- Inline graphics support (Sixel, iTerm2, Kitty protocols)
- Custom GLSL post-processing (Shadertoy/Ghostty-compatible)
- Scrollbar rendering with mark overlays
- Background image support

[Source: par-term-render crate](https://crates.io/crates/par-term-render)

### bracket-lib (bracket-terminal)

Roguelike toolkit with a `webgpu` feature flag for wgpu rendering.

- Provides virtual ASCII/CP437 terminal with tile support
- wgpu backend is an alternative to the default OpenGL backend
- Users report better frame rates and input latency with wgpu vs OpenGL
- Known SRGB/gamma issues with the wgpu backend (open issue)

[Source: bracket-lib](https://github.com/thebracket/bracket-lib)

### CuTTY (gold-silver-copper/CuTTY)

A small wgpu-powered terminal emulator ("pronounced cutie"). Created March 2026.

[Source: CuTTY](https://github.com/gold-silver-copper/CuTTY)

### Alacritty (not wgpu, but relevant context)

The reference GPU-accelerated terminal in Rust. Uses raw OpenGL (not wgpu). 64K+ stars. Proves the
instanced-quad + glyph-atlas model works. Its rendering approach is the template most wgpu terminals
follow.

[Source: alacritty](https://github.com/alacritty/alacritty)

### ghostty-web (NimbleMarkets fork)

Ghostty's web port with three renderer options: WebGPU, WebGL2, and Canvas2D. Shows the WebGPU
renderer path for terminal grid rendering in the browser.

[Source: ghostty-web](https://github.com/NimbleMarkets/ghostty-web/tree/nm-webgpu)

### glyphon

Not a terminal, but the standard library for 2D text rendering with wgpu. Used by iced, egui, and
many others.

- cosmic-text for shaping/layout + etagere for atlas packing + wgpu for rendering
- Integrates into existing render passes (middleware pattern)
- 700+ stars, 120K+ downloads/90 days
- Could be used directly for non-grid (UI/overlay) text in a terminal app

[Source: glyphon](https://github.com/grovesNL/glyphon)

### Attyx (Zig, not Rust, but valuable architecture reference)

A Zig terminal emulator with Metal (macOS) and OpenGL (Linux) backends. Its GPU rendering blog post
is the most detailed public writeup of terminal GPU rendering architecture.

- Pre-built vertex buffer approach (6 verts/cell, 192 bytes/cell)
- Dynamic glyph atlas with Knuth multiplicative hash lookup
- Dual textures (grayscale R8 + color RGBA)
- Seqlock for thread-safe cell buffer sharing
- 256-bit dirty row bitset
- 3 draw calls per frame: backgrounds, text, color emoji
- Sub-millisecond frame times

[Source: Attyx blog](https://semos.sh/blog/attyx-gpu-rendering/)

## 7. Performance Expectations

### Frame time

- **ratatui-wgpu**: ~800 FPS at 1080p updating every cell every frame (with CRT post-processing

  shader), so ~1.25ms per frame.

- **Attyx**: Sub-millisecond frame times for typical terminal output at 120 FPS.
- **bracket-lib wgpu**: Users report better frame rates than OpenGL backend.
- **Basilisk**: Claims sub-millisecond render times with thousands of lines via wgpu.

### GPU vs CPU bottleneck

For terminal rendering, the GPU is never the bottleneck. Drawing 10K-50K textured quads is trivial
for any modern GPU. The bottlenecks are:

1. **VT parsing** on the CPU (processing escape sequences from pty output)
2. **Text shaping** (HarfBuzz/rustybuzz) for complex scripts
3. **Glyph rasterization** on cache miss (first appearance of a glyph)
4. **Buffer upload** when many cells change at once (e.g., `cat` of a large file)

Once glyphs are cached in the atlas, the per-frame GPU cost is minimal: update the instance/vertex
buffer for dirty rows, issue 2-3 draw calls, present.

### Dirty tracking optimization

Most terminal frames only change a few rows (prompt, cursor line, scrolling output). With dirty-row
tracking:

- Static prompt with blinking cursor: rebuild 1-2 rows per frame
- Streaming output: rebuild visible rows as they change
- Full-screen redraw (`cat` large file): all rows dirty, full rebuild, still sub-millisecond on GPU

### Memory

- Instance/vertex buffer: ~2 MB for a 200x50 grid
- Glyph atlas: 2048x2048 R8 = 4 MB, 4096x4096 = 16 MB
- Color emoji atlas: same dimensions but RGBA = 4x
- Total GPU memory for a terminal: typically 10-30 MB

## 8. Trade-offs: wgpu vs OpenGL

| Aspect                 | wgpu                                                                            | OpenGL                                                                             |
| ---------------------- | ------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| **API safety**         | Fully validated, safe Rust. Errors are callbacks, not GPU crashes.              | Unsafe C API. Easy to hit undefined behavior.                                      |
| **Platform coverage**  | Vulkan + Metal + DX12 + OpenGL + WebGPU. One API, all platforms.                | OpenGL works everywhere but is deprecated on macOS (stuck at 4.1), missing on iOS. |
| **macOS story**        | Metal backend is first-class. Apple actively supports Metal.                    | OpenGL deprecated since macOS 10.14. No new features, potential removal.           |
| **Browser/WASM**       | WebGPU (native-like performance) + WebGL2 fallback.                             | WebGL2 only. No compute shaders. Limited features.                                 |
| **Pipeline model**     | Immutable, pre-compiled render pipelines. Driver can optimize upfront.          | Mutable global state. Driver must defer optimization.                              |
| **Command recording**  | Explicit command buffers. Can record on multiple threads.                       | Immediate mode. Single-threaded by design.                                         |
| **Shader language**    | WGSL (cross-platform via naga translation).                                     | GLSL (platform-specific versions, driver-dependent compilation).                   |
| **Complexity**         | More boilerplate for setup. Explicit resource management.                       | Simpler initial setup. Familiar to most graphics programmers.                      |
| **Ecosystem maturity** | Young but rapidly maturing. wgpu v29 (June 2025). Used in Firefox, Servo, Deno. | Decades old. Every edge case documented.                                           |
| **Debugging tools**    | RenderDoc works with Vulkan/DX12 backends. Xcode GPU debugger with Metal.       | RenderDoc, Nsight, apitrace. More tooling available.                               |
| **Overhead**           | Thin layer over native APIs. Validation has some cost but can be disabled.      | Driver overhead varies wildly. Some drivers are fast, some are not.                |

### When to choose wgpu

- You want a single codebase for native desktop + web (WASM/WebGPU)
- You need macOS support without relying on deprecated OpenGL
- You want safe Rust API without `unsafe` blocks
- You want compute shaders (for advanced effects, or future compute-driven rendering)
- You want a forward-looking API; WebGPU is the future of portable GPU APIs

### When OpenGL might still be better

- You need maximum compatibility with old/embedded hardware (GL ES 2.0)
- You have existing GLSL shaders and a working OpenGL renderer
- You need minimal dependency size (wgpu pulls in naga, wgpu-core, wgpu-hal, etc.)
- You're targeting a platform where wgpu support is immature

### The Alacritty question

Alacritty uses OpenGL and is the most popular GPU terminal. It works well but has known issues on
macOS (OpenGL deprecation warnings, occasional rendering glitches on new macOS versions). Rio exists
partly as an answer to this: "what if Alacritty, but wgpu instead of OpenGL?" Rio's existence and
active development validate the wgpu approach.

### egui's experience

egui (the immediate-mode GUI library) transitioned from a `glow` (OpenGL) backend to wgpu as the
default in 2025. Users reported some performance regressions in specific scenarios (higher idle GPU
usage), but the wgpu backend provides better cross-platform support and access to compute shaders.
The transition highlighted that wgpu's overhead is real but manageable, and the platform coverage
benefits outweigh the costs.

[Source: egui wgpu transition issue](https://github.com/emilk/egui/issues/7761)

## Sources

### Kept

- [wgpu repository](https://github.com/gfx-rs/wgpu) - primary source for architecture, platform

  matrix, API design

- [Learn Wgpu tutorial](https://sotrh.github.io/learn-wgpu/) - best tutorial for wgpu fundamentals,

  instancing chapter directly relevant

- [Attyx GPU rendering blog](https://semos.sh/blog/attyx-gpu-rendering/) - the most detailed public

  writeup of terminal GPU rendering; covers vertex layout, glyph atlas, shaders, dirty tracking,
  threading

- [Warp glyph atlas blog](https://www.warp.dev/blog/adventures-text-rendering-kerning-glyph-atlases) -

  deep dive on glyph atlas design, sub-pixel positioning, caching tradeoffs

- [ratatui-wgpu](https://github.com/jesterhearts/ratatui-wgpu) - production wgpu terminal backend

  with performance numbers and dependency rationale

- [Rio terminal / sugarloaf](https://rioterm.com/) - most mature wgpu terminal emulator in Rust, 7K

  stars

- [glyphon](https://github.com/grovesNL/glyphon) - standard library for wgpu 2D text rendering
- [naga README](https://github.com/gfx-rs/wgpu/tree/trunk/naga) - shader translation matrix
- [bracket-lib](https://github.com/thebracket/bracket-lib) - roguelike toolkit with wgpu backend
- [par-term-render](https://crates.io/crates/par-term-render) - wgpu terminal renderer crate
- [tchayen WebGPU text rendering](https://tchayen.com/drawing-text-in-webgpu-using-just-the-font-file) -

  WebGPU text from raw font data

### Dropped

- LogRocket wgpu blog - rehashes docs without new information
- Various wgpu-zoo/quickgpu/tribufu examples - small demos, no terminal-specific insight
- Medium/Scribe Rio articles - early-stage (v0.0.8), superseded by current Rio docs
- basilisk/yetty/ori-term - very early stage / AI-generated projects, no real implementation to

  learn from

- WebGPU vs Vulkan Medium article - interesting but not directly relevant to implementation

## Gaps

1. **Compute shader grid rendering:** No production terminal uses compute shaders for cell rendering

   yet. The approach is theoretically interesting (store grid in a storage buffer, let compute
   shader generate vertices or write directly to a render target) but untested for this use case.

1. **SDF (Signed Distance Field) text rendering:** Some game engines use SDF for

   resolution-independent text. Could reduce atlas memory and enable smooth scaling. No terminal
   emulator uses this approach. Worth investigating for zoom/DPI-independent rendering.

1. **WebGPU browser performance benchmarks:** No direct comparisons of wgpu-on-WebGPU vs WebGL2 for

   terminal rendering in browsers. Rio supports both but doesn't publish comparative benchmarks.

1. **wgpu overhead measurements:** The exact CPU overhead of wgpu's validation layer vs raw

   Vulkan/Metal/OpenGL is not well-documented. egui's transition suggests it's measurable but not a
   problem for terminal workloads.

1. **Multi-window / multi-surface:** How wgpu handles multiple windows (multiple `Surface` objects

   sharing a `Device`) is less documented. Relevant if the terminal supports detachable panes or
   multiple windows.
