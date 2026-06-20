# Research: WebGL 2 Backend for Rust Terminal/Grid Rendering (WASM)

## Summary

There are two proven approaches for GPU-accelerated terminal/grid rendering in the browser: (1) the
"data-texture" approach (doryen-rs, libtcod GL2) where cell data is uploaded as textures and a
single full-screen quad shader does all the work, and (2) the "instanced quads" approach (xterm.js,
bracket-lib) where each cell is a quad instance with per-instance vertex attributes. Both achieve a
single draw call per frame. The data-texture approach is simpler and a better fit for fixed-grid
rendering. For Rust/WASM, the `glow` crate provides the best balance of control, simplicity, and
cross-platform support (native OpenGL + WebGL2 via `web-sys`).

## Findings

### 1. Rendering Approach: Data Textures vs Instanced Quads

Two architectures dominate GPU grid rendering. Both aim to batch everything into one or two draw
calls per frame.

### Approach A: Data Textures + Full-Screen Quad (doryen-rs, libtcod)

The CPU maintains three flat arrays matching the grid dimensions:

- **Glyph index texture** (RGBA): encodes the character/tile index per cell
- **Foreground color texture** (RGBA): per-cell foreground color
- **Background color texture** (RGBA): per-cell background color

Each frame, these arrays are uploaded to GPU textures via `texSubImage2D`. A single full-screen quad
is drawn. The fragment shader reads the cell coordinates from the interpolated texture coordinate,
samples all three data textures, looks up the glyph in a font atlas texture, and composites
foreground/background.

This is conceptually identical to the "sprite tile maps on the GPU" technique described by Toji
(Brandon Jones): each pixel of a "map texture" is a lookup table entry into a sprite sheet, so the
entire grid is drawn with a single quad.
[TojiCode: Sprite tile maps on the GPU](https://blog.tojicode.com/2012/07/sprite-tile-maps-on-gpu.html)

Pros: Minimal GL state, trivially one draw call, shader does all the work, no vertex buffer
management per cell. Cons: Uploads entire grid data every frame (even unchanged cells). For a 200x50
grid = 10,000 cells, that is 3 textures x 40KB = 120KB per frame, which is negligible on modern
hardware.

### Approach B: Instanced Quads + Vertex Attributes (xterm.js, bracket-lib)

Build a `Float32Array` containing per-cell data (position, texture atlas coordinates, fg/bg colors).
Upload it as a vertex buffer. Use `drawElementsInstanced` (WebGL2) to draw all quads in one call.
Each instance is a single cell quad; the vertex shader positions it and the fragment shader samples
the glyph atlas.

xterm.js uses this approach. From the PR description: "the webgl renderer builds a Float32Array
containing all the data needed to draw and a webgl program (vertex + fragment shader) that knows how
to draw from the Float32Array is uploaded to the GPU."
[xterm.js WebGL Renderer PR #1790](https://github.com/xtermjs/xterm.js/pull/1790)

bracket-lib (the successor to bracket-terminal / rltk) takes a similar per-cell vertex approach,
building vertex arrays with position, color, background color, and texture coordinates per glyph.
[bracket-lib shader_strings.rs](https://github.com/amethyst/bracket-lib/blob/e2488ea/bracket-terminal/src/hal/native/shader_strings.rs)

Pros: Can selectively update only changed cells in the vertex buffer. More flexible for mixed-size
cells. Cons: More vertex buffer management. Vertex data is larger per cell (position + UV + colors
per vertex x 4 or 6 vertices per quad, or per instance).

**Recommendation for a fixed-size grid renderer**: Approach A (data textures) is simpler and
produces less code. The "upload 3 textures + draw 1 quad" pattern is easy to implement and debug.
Approach B is better when cells have variable sizes or you need to render subsets of the grid.

### 2. Shader Design for Grid Rendering

#### doryen-rs Shaders (GLSL 300 es, the canonical example)

**Vertex shader** (`doryen_vs.glsl`):

```glsl
in vec2 aVertexPosition;
in vec2 aTextureCoord;
out vec2 vTextureCoord;
uniform vec2 uTermSize;
void main(void) {
    gl_Position = vec4(aVertexPosition.xy, 0.0, 1.0);
    // texture coordinates from (0,0) to (console_width, console_height)
    vTextureCoord = aTextureCoord * uTermSize;
}
```

The vertex shader draws a full-screen quad (4 vertices: (-1,-1) to (1,1)). It scales texture
coordinates to console dimensions so the fragment shader receives cell-space coordinates.

**Fragment shader** (`doryen_fs.glsl`):

```glsl
precision mediump float;
uniform sampler2D uFont;   // font/glyph atlas texture
uniform sampler2D uAscii;  // per-cell glyph index (encoded in R,G channels)
uniform sampler2D uFront;  // per-cell foreground color
uniform sampler2D uBack;   // per-cell background color
uniform float uFontCharsPerLine;  // glyphs per row in the font atlas
uniform vec2 uFontCoef;    // converts glyph grid pos to UV (e.g. 1/16, 1/16)
uniform vec2 uTermCoef;    // converts cell pos to data texture UV

in vec2 vTextureCoord;
out vec4 FragColor;

void main() {
    // Which cell are we in? Floor to get integer cell coords, scale to UV
    vec2 address = floor(vTextureCoord) * uTermCoef + vec2(0.001, 0.001);
    // Decode glyph index from RGBA (supports up to 65535 glyphs)
    vec4 ascii_vec = texture(uAscii, address);
    float ascii_code = (ascii_vec.r * 255.0) + (ascii_vec.g * 255.0 * 256.0);
    // Sample fg/bg colors for this cell
    vec4 foreground = texture(uFront, address);
    vec4 background = texture(uBack, address);
    // Find glyph position in the font atlas
    vec2 tchar = vec2(
        mod(floor(ascii_code), floor(uFontCharsPerLine)),
        floor(ascii_code / uFontCharsPerLine)
    );
    // Sub-cell position within the glyph
    vec2 pixPos = fract(vTextureCoord) * uFontCoef;
    // Sample the glyph
    vec4 font_color = texture(uFont, tchar * uFontCoef + pixPos);
    // Composite: glyph alpha blends foreground over background
    FragColor = font_color.a * foreground * vec4(font_color.rgb, 1.0)

              + (1.0 - font_color.a) * background;

}
```

[Source: doryen-rs](https://github.com/jice-nospam/doryen-rs/blob/master/src/doryen_fs.glsl)

Key aspects:

- `floor(vTextureCoord)` gives the integer cell position
- `fract(vTextureCoord)` gives the sub-cell position (0..1 within a glyph)
- Glyph index is packed into R and G channels (16-bit, up to 65535 glyphs)
- The final composite formula: `alpha * fg * glyph_rgb + (1-alpha) * bg`

#### libtcod GL2 Renderer (same concept, slightly different encoding)

libtcod's fragment shader uses the same data-texture approach but encodes the tile address as a 2D
(x,y) coordinate in the atlas, packed across all 4 RGBA channels (supporting 16-bit x and y):

```glsl
vec4 tile_encoded = texture2D(t_console_tile, console_pos);
vec2 tile_address = vec2(
    tile_encoded.x * 0xff + tile_encoded.y * 0xff00,
    tile_encoded.z * 0xff + tile_encoded.w * 0xff00
);
```

It also clamps `tile_interp` (the sub-cell position) to prevent texture bleeding at tile edges:

```glsl
tile_interp = clamp(tile_interp, 0.5 / tile_size, 1.0 - 0.5 / tile_size);
```

[Source: libtcod renderer_gl2.c](https://github.com/libtcod/libtcod/blob/e9660659/src/libtcod/renderer_gl2.c)

#### rot.js tile-gl Shaders (per-tile draw calls, simpler but slower)

rot.js uses a simpler WebGL2 approach: one quad per tile, one `drawArrays` call per tile. Each tile
sets uniforms for position and atlas offset. This is straightforward but does not batch:

```glsl
// Vertex shader
void main() {
    vec2 targetPosPx = (targetPosRel + tilePosRel) * tileSize;
    vec2 targetPosNdc = ((targetPosPx / targetSize) - 0.5) * 2.0;
    targetPosNdc.y *= -1.0;
    gl_Position = vec4(targetPosNdc, 0.0, 1.0);
    tilesetPosPx = tilesetPosAbs + tilePosRel * tileSize;
}
// Fragment shader: uses texelFetch for pixel-perfect sampling
vec4 texel = texelFetch(image, ivec2(tilesetPosPx), 0);
```

This issues one draw call per visible tile. For a 80x25 grid that is 2000 draw calls per frame,
making it much slower than the batched approaches. It does support optional colorization via
uniforms.

[Source: rot.js tile-gl.ts](https://github.com/ondras/rot.js/blob/394b3e4/src/display/tile-gl.ts)

### 3. Texture Atlas for Glyphs

#### Building the Atlas

All reviewed implementations use a pre-built glyph atlas (a PNG image with glyphs laid out in a
grid). The typical layout is a CP437 or extended ASCII set, 16 glyphs per row, 16 rows = 256 glyphs.
doryen-rs uses this layout by default (e.g., `terminal_8x8.png` = 128x128 pixels).

For dynamic glyph sets (Unicode), xterm.js renders glyphs on-the-fly using a 2D Canvas
`measureText`/`fillText` to draw glyphs into a texture atlas canvas, then uploads that canvas as a
WebGL texture. The atlas starts at 512x512 and grows to multiple pages (up to 4096x4096). Glyphs are
packed row by row. An LRU cache manages eviction.
[xterm.js texture atlas PRs #4244, #4170](https://github.com/xtermjs/xterm.js/pull/4244)

For a Rust/WASM implementation with a fixed glyph set, the simplest path is:

1. **At build time**: embed a pre-rendered atlas PNG (e.g., CP437 in a 16x16 grid) as

   `include_bytes!`

1. **At init**: decode the PNG, upload to a `TEXTURE_2D` with `NEAREST` filtering
1. **At render**: the fragment shader indexes into the atlas using `glyph_index % columns` and
   `glyph_index / columns`

For dynamic/Unicode support:

1. **At runtime**: use an offscreen `<canvas>` (via `web-sys` `HtmlCanvasElement`) to render glyphs

   with `CanvasRenderingContext2D.fillText()`

1. **Pack glyphs into a texture atlas** (row-major, fixed cell size for monospace)
1. **Upload via `texSubImage2D`** for incremental updates (only upload new glyph rows)1. **Maintain
   a `HashMap<char, (u16, u16)>`** mapping characters to atlas coordinates

#### Uploading the Atlas

```rust
// Using web-sys directly:
gl.tex_image_2d_with_u32_and_u32_and_html_image_element(
    WebGl2RenderingContext::TEXTURE_2D,
    0,  // level
    WebGl2RenderingContext::RGBA as i32,
    WebGl2RenderingContext::RGBA,
    WebGl2RenderingContext::UNSIGNED_BYTE,
    &image_element,
)?;
```

Set `TEXTURE_MAG_FILTER` and `TEXTURE_MIN_FILTER` to `NEAREST` for pixel-perfect rendering (no
interpolation between glyphs). Use `CLAMP_TO_EDGE` wrapping.

#### Indexing

The shader converts a glyph index to atlas UV coordinates:

```glsl
vec2 glyph_pos = vec2(
    mod(glyph_index, glyphs_per_row),
    floor(glyph_index / glyphs_per_row)
);
vec2 atlas_uv = (glyph_pos + sub_cell_pos) * glyph_size_in_uv;
```

Where `glyph_size_in_uv = vec2(glyph_width / atlas_width, glyph_height / atlas_height)`.

### 4. Rust WASM + WebGL Integration Options

#### Option A: `web-sys` Direct (lowest level, most control)

Use `web_sys::WebGl2RenderingContext` directly. This is a 1:1 binding to the browser's WebGL2 API.
Requires enabling many features in `Cargo.toml`:

```toml
[dependencies.web-sys]
version = "0.3"
features = [
    "HtmlCanvasElement", "WebGl2RenderingContext",
    "WebGlBuffer", "WebGlProgram", "WebGlShader",
    "WebGlTexture", "WebGlUniformLocation",
    "WebGlVertexArrayObject",
]
```

The wasm-bindgen WebGL example demonstrates this pattern.
[Source: wasm-bindgen WebGL example](https://github.com/rustwasm/wasm-bindgen/blob/master/examples/webgl/src/lib.rs)

Pros: Zero abstraction overhead, full control, smallest binary size. Cons: Verbose, no code sharing
with native GL, lots of boilerplate.

#### Option B: `glow` (recommended for cross-platform)

`glow` ("GL on Whatever") wraps OpenGL, OpenGL ES, and WebGL2 behind a single `HasContext` trait. On
WASM, it uses `web-sys` internally. On native, it loads GL function pointers dynamically.

```rust
// Creating a glow context from WebGL2:
let gl = glow::Context::from_webgl2_context(webgl2_context);
// Then use the same API as native:
let program = gl.create_program().unwrap();
let shader = gl.create_shader(glow::VERTEX_SHADER).unwrap();
gl.shader_source(shader, VERTEX_SOURCE);
gl.compile_shader(shader);
gl.attach_shader(program, shader);
// ...
```

This is what doryen-rs uses under the hood (via `uni-gl` which wraps similar functionality). The
`glow` crate is actively maintained and used by major projects (egui's glow backend, wgpu
internally).

[Source: glow crate](https://github.com/grovesNL/glow)

Pros: Same rendering code compiles for native (OpenGL) and web (WebGL2). Thin wrapper, low overhead.
Cons: Still raw GL; you manage state, shaders, and buffers yourself.

#### Option C: `wgpu` with WebGL2 Backend

`wgpu` provides a high-level, safe GPU abstraction modeled on the WebGPU standard. It supports a
WebGL2 backend for WASM targets (when WebGPU is unavailable). The backend uses `glow` internally.

```rust
// wgpu auto-selects WebGL2 on WASM when WebGPU isn't available
let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
    backends: wgpu::Backends::GL, // WebGL2 on WASM
    ..Default::default()
});
```

[Source: wgpu docs](https://docs.rs/wgpu/latest/wasm32-unknown-unknown/index.html)

Pros: Safe, modern API. Future-proof (can target WebGPU when browsers support it). Handles state
management. Cons: Large binary size (~500KB+ WASM overhead). Slower compile times. Overkill for a 2D
grid renderer. WebGL2 backend has some limitations vs native. egui is considering switching from
glow to wgpu as default, indicating the ecosystem trend.
[egui issue #5889](https://github.com/emilk/egui/issues/5889)

#### Option D: `minwebgl` or `webglue` (WASM-only wrappers)

Lightweight WebGL2-specific wrappers that reduce boilerplate. `webglue` is built on top of `glow`.
`minwebgl` provides a minimal API directly over `web-sys`.

Pros: Less boilerplate than raw `web-sys`. Cons: WASM-only (no native support). Small ecosystem,
less battle-tested.

**Recommendation**: Use `glow` for the WebGL2 backend. It gives you cross-platform compatibility
(the same shader/rendering code can work with a native OpenGL backend), has a minimal abstraction
cost, and is well-maintained.

### 5. Performance: WebGL vs Canvas 2D

xterm.js provides the most rigorous comparison. From the WebGL renderer PR (#1790), benchmarking
frame render times:

| Scenario                 | Canvas 2D (ms/frame) | WebGL (ms/frame) | Speedup  |
| ------------------------ | -------------------- | ---------------- | -------- |
| Macbook 87x26, `ls -lR`  | 4.80                 | 0.69             | **7x**   |
| Macbook 300x80, `ls -lR` | 15.28                | 3.69             | **4x**   |
| Windows 87x26, `tree`    | 7.31                 | 0.73             | **10x**  |
| Windows 300x80, `tree`   | 19.34                | 2.06             | **9x**   |
| Macbook 87x26, CJK text  | 14.63                | 5.93             | **2.5x** |
| Macbook 87x26, Emoji     | 27.47                | 19.28            | **1.4x** |

[Source: xterm.js PR #1790](https://github.com/xtermjs/xterm.js/pull/1790)

Key performance drivers:

- **Batching**: Canvas 2D issues one `drawImage` per cell. WebGL batches everything into a single

  `drawElementsInstanced` call.

- **GPU parallelism**: Fragment shader runs across GPU cores. Canvas 2D is CPU-bound and

  synchronous.

- **Data transfer**: Canvas 2D for a 4K terminal transfers ~32MB/frame from CPU to GPU in small

  chunks. WebGL uploads compact buffers in bulk.
  [xterm.js issue #4175](https://github.com/xtermjs/xterm.js/issues/4175)

- **CJK/Emoji gap narrows**: The bottleneck shifts to glyph rasterization (rendering complex glyphs

  to the texture atlas), which is CPU-bound regardless.

VS Code switched to the WebGL renderer as the default terminal backend.
[VS Code PR #84440](https://github.com/microsoft/vscode/pull/84440)

### 6. Prior Art

| Project                    | Language   | Approach                         | Draw Calls       | Notes                                                                                                              |
| -------------------------- | ---------- | -------------------------------- | ---------------- | ------------------------------------------------------------------------------------------------------------------ |
| **doryen-rs**              | Rust       | Data textures + full-screen quad | 1                | GLSL 300 es, uni-gl abstraction. Ships vertex + fragment shaders. Supports RGBA, RGB, greyscale fonts.             |
| **libtcod** (GL2 renderer) | C          | Data textures + full-screen quad | 1                | 3 console textures (tile, fg, bg) + tileset atlas. Edge clamping to prevent bleeding.                              |
| **bracket-lib**            | Rust       | Per-cell vertex arrays           | 1                | Multiple shader programs: with-bg, no-bg, fancy (rotation/scale), sprites. Uses glow/OpenGL.                       |
| **xterm.js** (WebGL addon) | TypeScript | Float32Array + instanced draw    | 1                | WebGL2 `drawElementsInstanced`. Dynamic texture atlas with LRU cache. Multiple atlas pages (512x512 to 4096x4096). |
| **rot.js** (tile-gl)       | TypeScript | Per-tile `drawArrays`            | N (one per cell) | Simple but slow. Uses `texelFetch` for pixel-perfect sampling. Supports colorization.                              |
| **TojiCode tilemap**       | JavaScript | Data texture + full-screen quad  | 1-2              | Map encoded as image (R,G channels = tile x,y). Supports 65535 tile types, huge maps.                              |

### 7. Fallback Strategy When WebGL is Unavailable

WebGL2 support is effectively universal in modern browsers (Chrome, Firefox, Safari 15+, Edge). The
main failure cases are:

- Very old browsers / Safari < 15
- Browser privacy settings or extensions that disable WebGL
- GPU driver blacklists
- Headless/server-side rendering contexts
- `getContext("webgl2")` returning `null`

### Recommended fallback strategy

```rust
// Try WebGL2 first
let context = canvas.get_context("webgl2")?;
match context {
    Some(ctx) => {
        // Use WebGL2 renderer
        let gl = ctx.dyn_into::<WebGl2RenderingContext>()?;
        Ok(WebGlBackend::new(gl))
    }
    None => {
        // Fall back to Canvas 2D
        let ctx = canvas.get_context("2d")?
            .unwrap()
            .dyn_into::<CanvasRenderingContext2d>()?;
        Ok(Canvas2dBackend::new(ctx))
    }
}
```

xterm.js does exactly this: the WebGL renderer is an optional addon. If it fails to initialize
(context loss, creation failure), it falls back to the Canvas renderer, which in turn can fall back
to a DOM-based renderer. [xterm.js issue #3271](https://github.com/xtermjs/xterm.js/issues/3271)

Handle WebGL context loss with the `webglcontextlost` and `webglcontextrestored` events. On context
loss, you must recreate all GL resources (shaders, textures, buffers).

### 8. Trade-offs

| Factor                     | Data Textures (doryen-rs style)                      | Instanced Quads (xterm.js style)                       |
| -------------------------- | ---------------------------------------------------- | ------------------------------------------------------ | ----------------------------------------------------------- |
| **Complexity**             | Lower. One quad, 3-4 textures, one shader.           | Higher. Vertex buffer management, instance attributes. |
| **Draw calls**             | 1                                                    | 1 (when using instancing)                              |
| **Data upload**            | 3 full texture uploads per frame (~120KB for 200x50) | Vertex buffer update (can be partial)                  |
| **Partial updates**        | Possible via `texSubImage2D` on changed rows/cells   | Easier with buffer sub-data updates                    |
| **Variable cell sizes**    | Hard (assumes uniform grid)                          | Possible (different quad sizes per instance)           |
| **Shader complexity**      | All logic in fragment shader                         | Split between vertex and fragment                      |
| **Unicode/dynamic glyphs** | Need runtime atlas generation + upload               | Same, but atlas management is independent              |
| **Binary size (WASM)**     | Minimal with glow or web-sys                         | Same                                                   |
| **Cross-platform**         | Same shaders work native + web (via glow)            | Same                                                   |
| **Max grid size**          | Limited by max texture size (4096x4096 = 16M cells)  | Limited by vertex buffer size (practical: millions)    | **Additional trade-offs for the integration layer choice:** |

|                      | web-sys        | glow                          | wgpu                       |
| -------------------- | -------------- | ----------------------------- | -------------------------- |
| **WASM binary size** | ~50KB          | ~80KB                         | ~500KB+                    |
| **Native support**   | No             | Yes (OpenGL)                  | Yes (Vulkan/Metal/DX12/GL) |
| **API level**        | Raw WebGL2     | Raw GL (cross-platform)       | High-level, safe           |
| **Compile time**     | Fast           | Fast                          | Slow                       |
| **Future-proofing**  | WebGL2 only    | GL only                       | WebGPU + WebGL2 fallback   |
| **Ecosystem**        | Core Rust/WASM | Used by egui, wgpu internally | Major projects             |

## Recommended Architecture

For a Rust terminal/grid rendering library targeting WASM with a WebGL2 backend:

1. **Use `glow`** for the GL abstraction layer (cross-platform, thin, well-maintained)
2. **Use the data-texture approach** (doryen-rs/libtcod style) for fixed-grid rendering:
   - 3 data textures: glyph index, fg color, bg color (each grid_w x grid_h, RGBA)
   - 1 glyph atlas texture (pre-built PNG or runtime-generated)
   - 1 full-screen quad with a fragment shader that does all the work
3. **Pre-build the glyph atlas** as an embedded PNG for ASCII/CP437; add runtime atlas generation
   for Unicode support later

4. **Implement Canvas 2D fallback** behind a shared `Backend` trait4. **Handle context loss** by
   listening for `webglcontextlost`/`webglcontextrestored` events## Sources

- **Kept**: doryen-rs shaders (doryen_vs.glsl, doryen_fs.glsl, program.rs) - Primary reference for

  the data-texture grid rendering approach in Rust.
  [GitHub](https://github.com/jice-nospam/doryen-rs)

- **Kept**: xterm.js WebGL Renderer PR #1790 - Definitive benchmarks (Canvas 2D vs WebGL),

  architecture description, and practical implementation details.
  [GitHub PR](https://github.com/xtermjs/xterm.js/pull/1790)

- **Kept**: xterm.js texture atlas PRs (#4244, #4170, #4061) - Multi-page atlas strategy, LRU cache,

  incremental upload. [GitHub](https://github.com/xtermjs/xterm.js/pull/4244)

- **Kept**: libtcod renderer_gl2.c - Clean C implementation of the same data-texture approach with

  detailed shader code.
  [GitHub](https://github.com/libtcod/libtcod/blob/e9660659/src/libtcod/renderer_gl2.c)

- **Kept**: bracket-lib shader_strings.rs - Multiple shader approaches in Rust (with-bg, no-bg,

  fancy, sprites).
  [GitHub](https://github.com/amethyst/bracket-lib/blob/e2488ea/bracket-terminal/src/hal/native/shader_strings.rs)

- **Kept**: TojiCode: Sprite tile maps on the GPU - Foundational technique for GPU tilemap

  rendering. [Blog](https://blog.tojicode.com/2012/07/sprite-tile-maps-on-gpu.html)

- **Kept**: rot.js tile-gl.ts - Simple WebGL2 grid renderer with colorization support.

  [GitHub](https://github.com/ondras/rot.js/blob/394b3e4/src/display/tile-gl.ts)

- **Kept**: glow crate source (web_sys.rs) - How glow wraps web-sys WebGL2 behind the HasContext

  trait. [GitHub](https://github.com/grovesNL/glow/blob/main/src/web_sys.rs)

- **Kept**: wasm-bindgen WebGL example - Minimal Rust/WASM WebGL2 setup.

  [GitHub](https://github.com/rustwasm/wasm-bindgen/blob/master/examples/webgl/src/lib.rs)

- **Kept**: VS Code WebGL terminal PR #84440 - Confirms production viability; GPU timeline analysis.

  [GitHub](https://github.com/microsoft/vscode/pull/84440)

- **Dropped**: webglue, minwebgl, webgl2 crates - Niche, small ecosystem, insufficient adoption for

  recommendation

- **Dropped**: web-graphics-comparison (luciopaiva) - SVG/Canvas/WebGL comparison for generic

  shapes, not terminal-specific

- **Dropped**: rot-gl.js (uzudil) - Abandoned 2014 fork, not relevant

## Gaps

- **Runtime Unicode atlas generation in Rust/WASM**: How to call browser

  `CanvasRenderingContext2D.fillText()` from Rust to rasterize arbitrary Unicode glyphs into an
  atlas. This requires `web-sys` Canvas2D APIs and is well-documented but not covered in depth here.
  xterm.js's approach (render to offscreen canvas, upload as texture) is the proven pattern.

- **SDF (Signed Distance Field) fonts**: For resolution-independent glyph rendering. Not used by any

  of the surveyed projects for terminal rendering, but could improve quality at arbitrary zoom
  levels. Worth investigating if zoom/DPI flexibility is a requirement.

- **WebGPU path**: wgpu can target WebGPU natively on supporting browsers (Chrome 113+, Firefox

  141+). If WebGPU becomes a target, the shader language would switch from GLSL to WGSL, and the
  rendering API would change significantly. This is a future consideration, not a blocker.

- **Benchmarks for Rust/WASM specifically**: The xterm.js benchmarks are JavaScript. Rust/WASM may

  show different overhead characteristics for the buffer-building step (likely faster due to direct
  memory manipulation). No Rust-specific benchmarks were found.
