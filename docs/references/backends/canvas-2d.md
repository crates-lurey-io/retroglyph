# Research: HTML Canvas 2D Backend for a Rust WASM Terminal/Grid Renderer

## Summary

Canvas 2D is a practical backend for rendering a terminal grid in the browser. The core technique,
proven by xterm.js and rot.js, is a glyph atlas: render each unique glyph (character + foreground +
background + style) once to an offscreen canvas via `fillText`, cache the result, and blit cached
glyphs to the visible canvas via `drawImage` or `putImageData`. This avoids the cost of per-frame
text shaping. Dirty-region tracking (per-row or per-cell) further reduces work by only redrawing
changed cells. From Rust/WASM, the `web-sys` crate provides full access to
`CanvasRenderingContext2d`, `OffscreenCanvas`, and `FontFace` through `wasm-bindgen`.

## Findings

### 1. Canvas 2D Text Rendering APIs

The Canvas 2D API provides two text-drawing methods and one measurement method:

- **`fillText(text, x, y [, maxWidth])`** -- draws filled text using the current `font`,

  `fillStyle`, `textAlign`, and `textBaseline` properties. This is the workhorse for glyph
  rendering.

- **`strokeText(text, x, y [, maxWidth])`** -- draws text outlines. Occasionally used for underline

  stroke effects around glyphs (xterm.js uses it to create an outline between text descenders and
  underlines).

- **`measureText(text)`** -- returns a `TextMetrics` object with `width`, `actualBoundingBoxAscent`,

  `actualBoundingBoxDescent`, `actualBoundingBoxLeft`, `actualBoundingBoxRight`. These metrics are
  essential for computing cell sizes and positioning glyphs.

Key state properties for text rendering:

- `ctx.font` -- CSS font shorthand string, e.g. `"italic bold 16px 'Fira Code'"`. Must be set before

  `measureText` and `fillText`.

- `ctx.textBaseline` -- controls vertical alignment. `"middle"` is used by rot.js for centering;

  xterm.js uses a configured baseline constant.

- `ctx.textAlign` -- `"center"` for grid-cell centering (rot.js), `"left"` for monospace

  positioning.

- `ctx.textRendering` -- newer property (`"optimizeSpeed"`, `"optimizeLegibility"`,

  `"geometricPrecision"`). Limited browser support as of 2026.

**Performance note**: Setting `ctx.font` is expensive because the browser must parse the CSS font
string and resolve the font. Avoid setting it per-cell; set it once per style variant per frame.

[Source: MDN Drawing text](https://developer.mozilla.org/en-US/docs/Web/API/Canvas_API/Tutorial/Drawing_text)
[Source: MDN TextMetrics](https://developer.mozilla.org/en-US/docs/Web/API/TextMetrics)

### 2. The Texture Atlas / Glyph Cache Approach

This is the central optimization for canvas-based terminal rendering. Both xterm.js and rot.js use
variants of this pattern.

#### 2a. rot.js: Simple Per-Glyph Canvas Cache

rot.js `Rect` backend (`rect.ts`) implements a straightforward cache:

```typescript
// Key = "char" + fg_color + bg_color
let hash = '' + ch + fg + bg;
if (hash in this._canvasCache) {
  canvas = this._canvasCache[hash];
} else {
  canvas = document.createElement('canvas');
  let ctx = canvas.getContext('2d');
  canvas.width = this._spacingX;
  canvas.height = this._spacingY;
  // Draw background
  ctx.fillStyle = bg;
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  // Draw character
  ctx.fillStyle = fg;
  ctx.font = this._ctx.font;
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillText(char, this._spacingX / 2, Math.ceil(this._spacingY / 2));
  this._canvasCache[hash] = canvas;
}
// Blit to main canvas
this._ctx.drawImage(canvas, x * this._spacingX, y * this._spacingY);
```

Each unique combination gets its own small HTMLCanvasElement. Blitting is via
`drawImage(canvas, dx, dy)`. Cache is cleared when options change (font, spacing, etc.).

**Pros**: Simple, easy to implement, each glyph is pixel-perfect. **Cons**: One HTMLCanvasElement
per unique glyph. For a 16-color terminal with 95 printable ASCII characters, that could be 95 _16 _
16 = 24,320 canvases in theory (though in practice far fewer are used). Memory overhead per canvas
is non-trivial.

[Source: rot.js rect.ts](https://github.com/ondras/rot.js/blob/master/src/display/rect.ts)

#### 2b. rot.js: Tile Backend

For bitmap/tileset-based rendering, rot.js `Tile` backend (`tile.ts`) uses `drawImage` with a source
tileset image:

```typescript
// Look up tile position from tileMap
let tile = this._options.tileMap[chars[i]];
// Blit from tileset to main canvas
this._ctx.drawImage(
  this._options.tileSet, // source image (spritesheet)
  tile[0],
  tile[1], // source x, y
  tileWidth,
  tileHeight, // source width, height
  x * tileWidth,
  y * tileHeight, // dest x, y
  tileWidth,
  tileHeight, // dest width, height
);
```

With colorization enabled, it uses a temporary canvas with composite operations (`source-atop` for
fg color, `destination-over` for bg color) to recolor tiles at draw time.

[Source: rot.js tile.ts](https://github.com/ondras/rot.js/blob/master/src/display/tile.ts)

#### 2c. xterm.js: Multi-Page Texture Atlas (Production-Grade)

xterm.js's `TextureAtlas` (shared between canvas and WebGL renderers) is the most sophisticated
open-source implementation of this pattern. Key design details:

### Atlas structure

- Multiple atlas pages, each an HTMLCanvasElement (starting at 512x512, up to 4096x4096).
- Glyphs are packed left-to-right, top-to-bottom in rows.
- When a row's height is too large for the next glyph, the row becomes "fixed" and a new row starts

  below.

- When pages fill up, the 4 most-used same-sized pages are merged into one page at 2x size

  (quad-merge).

- A maximum page count triggers merging to avoid unbounded GPU memory.

### Glyph rendering pipeline

1. A temporary canvas (`_tmpCanvas`) is used to render each glyph in isolation.
2. Background color is drawn first with `globalCompositeOperation = 'copy'`.
3. Font is set: `"${fontStyle} ${fontWeight} ${fontSize * devicePixelRatio}px ${fontFamily}"`.
4. Foreground color is resolved (considering inverse, dim, minimum contrast ratio).
5. `fillText(chars, padding, padding + deviceCharHeight)` draws the glyph.
6. Underline, overline, and strikethrough decorations are drawn.
7. Background color pixels are cleared to transparent (via `clearColor` which scans ImageData and

   sets matching pixels' alpha to 0).

8. Tight bounding box is found by scanning ImageData for non-transparent pixels (top, left, right,
   bottom).

9. The cropped ImageData is placed into the atlas page via `putImageData`.
### Caching

- `FourKeyMap<code, bg, fg, ext>` for single characters.
- Separate `FourKeyMap<string, bg, fg, ext>` for combined/multi-codepoint characters.
- Cache lookup is O(1) via the multi-key map.

### Warm-up

- ASCII 33-126 are pre-rendered during idle time via `IdleTaskQueue` (uses `requestIdleCallback`).
- This eliminates jank from rendering common glyphs on first appearance.

### Color handling

- Supports 256-color palette, true color (RGB), default colors, inverse video, dim.
- Minimum contrast ratio enforcement: adjusts foreground color to meet WCAG contrast requirements.
- Background clearing with threshold to handle anti-aliasing edge pixels.

[Source: xterm.js TextureAtlas.ts](https://github.com/xtermjs/xterm.js/blob/master/addons/addon-webgl/src/TextureAtlas.ts)

### 3. Performance Characteristics and Limits

### Canvas 2D rendering performance profile

| Operation                      | Relative Cost | Notes                                                                                        |
| ------------------------------ | ------------- | -------------------------------------------------------------------------------------------- |
| `fillText`                     | High          | Font shaping, rasterization, anti-aliasing. 10-100us per call depending on glyph complexity. |
| `drawImage` (canvas-to-canvas) | Low           | ~1-5us. GPU-accelerated blit in modern browsers.                                             |
| `putImageData`                 | Medium        | CPU-side pixel copy. No compositing. ~5-20us for cell-sized regions.                         |
| `getImageData`                 | Medium-High   | Forces GPU-to-CPU readback if canvas was GPU-accelerated. ~10-50us.                          |
| `fillRect`                     | Very Low      | Simple rectangle fill, highly optimized. <1us.                                               |
| Setting `ctx.font`             | Medium        | CSS font string parsing. Avoid per-cell.                                                     |
| `measureText`                  | Low-Medium    | Fast after font is resolved. ~1-5us.                                                         |

### Throughput for terminal rendering

- A typical 80x24 terminal = 1,920 cells per frame.
- With glyph caching, each cell is a `drawImage` blit: ~1920 \* 3us = ~6ms. Well within 16ms budget.
- A large 200x50 terminal = 10,000 cells. With dirty tracking, typically <20% cells change per frame

  = 2,000 blits = ~6ms.

- Without caching (raw `fillText` per cell): 10,000 \* 50us = 500ms. Completely unacceptable.

### Canvas 2D limits

- Maximum canvas size: browser-dependent. Chrome caps at 16384x16384 (268M pixels) or 256MB. Safari

  has lower limits.

- Canvas 2D is typically GPU-accelerated for draws but `getImageData`/`putImageData` force CPU-GPU

  synchronization.

- No instanced drawing; each `drawImage` is a separate draw call. This is the main bottleneck vs

  WebGL.

- Anti-aliasing of text is controlled by the browser; cannot be disabled or tuned precisely.
- Subpixel rendering behavior varies across browsers and OSes.

### 4. Rust WASM Integration

#### 4a. web-sys Canvas API

The `web-sys` crate provides direct bindings to all Canvas 2D APIs. Required Cargo features:

```toml
[dependencies]
wasm-bindgen = "0.2"
web-sys = { version = "0.3", features = [
    "Document",
    "Window",
    "HtmlCanvasElement",
    "CanvasRenderingContext2d",
    "OffscreenCanvas",
    "OffscreenCanvasRenderingContext2d",
    "ImageData",
    "ImageBitmap",
    "ImageBitmapRenderingContext",
    "FontFace",
    "FontFaceSet",
    "TextMetrics",
] }
js-sys = "0.3"
```

### Key API mappings (Rust)

```rust
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, OffscreenCanvas};

// Get canvas and context
let document = web_sys::window().unwrap().document().unwrap();
let canvas: HtmlCanvasElement = document
    .get_element_by_id("canvas")
    .unwrap()
    .dyn_into()
    .unwrap();
let ctx: CanvasRenderingContext2d = canvas
    .get_context("2d")
    .unwrap()
    .unwrap()
    .dyn_into()
    .unwrap();

// Set font and draw text
ctx.set_font("16px monospace");
ctx.set_fill_style_str("white");
ctx.fill_text("Hello", 0.0, 16.0).unwrap();

// Measure text
let metrics = ctx.measure_text("W").unwrap();
let char_width = metrics.width();

// Create offscreen canvas for glyph atlas
let atlas = OffscreenCanvas::new(512, 512).unwrap();
let atlas_ctx: OffscreenCanvasRenderingContext2d = atlas
    .get_context("2d")
    .unwrap()
    .unwrap()
    .dyn_into()
    .unwrap();

// drawImage from one canvas to another
ctx.draw_image_with_offscreen_canvas(&atlas, 0.0, 0.0).unwrap();

// drawImage with source/dest rectangles (9-arg form for atlas blitting)
ctx.draw_image_with_offscreen_canvas_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
    &atlas,
    src_x, src_y, src_w, src_h,  // source rect in atlas
    dst_x, dst_y, dst_w, dst_h,  // dest rect on screen
).unwrap();
```

### Important notes for Rust/WASM

- Every Canvas API call crosses the WASM-JS boundary. Each call has ~50-200ns overhead from

  `wasm-bindgen`.

- Batching is important: minimize the number of `drawImage` calls. Consider drawing multiple cells

  in a single JS call via a thin JS shim if profiling shows call overhead is dominant.

- `OffscreenCanvas` is preferred over `document.createElement("canvas")` for atlas pages since it

  avoids DOM interaction and can theoretically be used in Web Workers.

- `OffscreenCanvasRenderingContext2d` has the same API as `CanvasRenderingContext2d` but some

  features (like `fillText` font rendering) may behave differently across browsers.

- Canvas state changes (font, fillStyle) should be batched; avoid alternating between different

  fonts/colors per cell.

#### 4b. OffscreenCanvas

`OffscreenCanvas` provides a canvas without DOM attachment:

```rust
let offscreen = OffscreenCanvas::new(width, height).unwrap();
let ctx = offscreen.get_context("2d").unwrap().unwrap()
    .dyn_into::<OffscreenCanvasRenderingContext2d>().unwrap();
```

Two usage patterns:

1. **Synchronous** (same thread): Create OffscreenCanvas, render glyphs to it, blit to visible

   canvas via `drawImage`. Use `transferToImageBitmap()` for zero-copy transfer.

1. **Async** (Web Worker): Transfer control of visible canvas to worker via

   `transferControlToOffscreen()`. Render entirely in worker thread. Frees main thread for input
   handling.

Browser support for OffscreenCanvas is now universal in modern browsers (Chrome, Firefox, Safari
16.4+, Edge).

[Source: MDN OffscreenCanvas](https://developer.mozilla.org/en-US/docs/Web/API/OffscreenCanvas)
[Source: web-sys OffscreenCanvas](https://docs.rs/web-sys/latest/web_sys/struct.OffscreenCanvas.html)

### 5. Dirty-Region Tracking

Dirty tracking avoids redrawing unchanged cells. Three levels of granularity:

#### 5a. Per-Row Dirty Tracking (xterm.js approach)

xterm.js's `IRenderer` interface defines `renderRows(start: number, end: number)`. The terminal
buffer tracks which rows have changed since the last frame and only requests re-rendering of dirty
rows.

```rust
struct DirtyTracker {
    dirty_rows: Vec<bool>,  // one flag per row
}

impl DirtyTracker {
    fn mark_dirty(&mut self, row: usize) {
        self.dirty_rows[row] = true;
    }

    fn mark_range(&mut self, start: usize, end: usize) {
        for row in start..=end {
            self.dirty_rows[row] = true;
        }
    }

    fn drain_dirty(&mut self) -> impl Iterator<Item = usize> + '_ {
        self.dirty_rows.iter_mut().enumerate()
            .filter(|(_, dirty)| **dirty)
            .map(|(i, dirty)| { *dirty = false; i })
    }
}
```

For row-level dirty tracking, clearing is done with `fillRect` across the full row width, then each
dirty cell in the row is redrawn.

#### 5b. Per-Cell Dirty Tracking (rot.js approach)

rot.js tracks dirty cells individually. Its `Display` class maintains a data map and only calls
`backend.draw()` for cells whose content has changed.

```rust
struct CellGrid {
    cells: Vec<Cell>,     // current state
    prev: Vec<Cell>,      // previous frame state
    width: usize,
}

impl CellGrid {
    fn dirty_cells(&self) -> impl Iterator<Item = (usize, usize, &Cell)> {
        self.cells.iter().enumerate()
            .zip(self.prev.iter())
            .filter(|((_, current), prev)| current != prev)
            .map(|((i, cell), _)| (i % self.width, i / self.width, cell))
    }
}
```

#### 5c. Dirty Rectangle Coalescing

For large contiguous changes (scrolling, clear screen), coalesce dirty cells into rectangles:

```rust
fn coalesce_dirty_rect(dirty_rows: &[bool]) -> Option<(usize, usize)> {
    let first = dirty_rows.iter().position(|&d| d)?;
    let last = dirty_rows.iter().rposition(|&d| d)?;
    Some((first, last))
}
```

Then clear and redraw just that rectangle. For full-screen scrolls, skip per-cell logic entirely and
do a bulk `drawImage` shift + render new lines.

#### 5d. Scroll Optimization

When content scrolls by N lines, use canvas self-copy instead of redrawing everything:

```rust
// Scroll up by n_rows: copy the bottom portion up, then clear and redraw new rows
ctx.draw_image_with_html_canvas_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
    &canvas,
    0.0, (n_rows * cell_height) as f64,  // source: below scrolled region
    canvas_width, remaining_height,
    0.0, 0.0,                             // dest: top of canvas
    canvas_width, remaining_height,
)?;
// Clear the newly exposed rows at the bottom
ctx.clear_rect(0.0, remaining_height, canvas_width, (n_rows * cell_height) as f64);
// Render only the new rows
```

### 6. Font Handling

#### 6a. Web Fonts via FontFace API

The `FontFace` API allows programmatic loading of custom fonts, which is critical because canvas
`fillText` uses whatever fonts are available at draw time.

```javascript
// JavaScript (call from Rust via wasm-bindgen)
const font = new FontFace('TerminalFont', 'url("./fonts/FiraCode.woff2")', {
  style: 'normal',
  weight: '400',
});
await font.load();
document.fonts.add(font);
// Now ctx.font = '16px TerminalFont' will work
```

From Rust:

```rust
use web_sys::FontFace;

let font = FontFace::new_with_str("TerminalFont", "url('./fonts/FiraCode.woff2')")?;
let promise = font.load()?;
// Use wasm_bindgen_futures::JsFuture to await the promise
JsFuture::from(promise).await?;
let font_set = window.document().unwrap().fonts();
font_set.add(&font)?;
```

**Critical gotcha**: If you call `fillText` before the font has loaded, the browser silently falls
back to a default font. You **must** await `FontFace.load()` or use `document.fonts.ready` before
rendering. After loading, invalidate the entire glyph cache since all cached glyphs used the wrong
font.

xterm.js has a dedicated `@xterm/addon-web-fonts` addon that handles this lifecycle.

[Source: MDN FontFace](https://developer.mozilla.org/en-US/docs/Web/API/FontFace)

#### 6b. Bitmap Fonts / Tilesets

For pixel-art or roguelike games, bitmap tilesets avoid font-shaping complexity entirely:

1. Load a spritesheet image (e.g., a CP437 tileset PNG).
2. Define a mapping from character/tile ID to `(sx, sy)` coordinates in the spritesheet.
3. Blit tiles directly via `drawImage(tilesetImage, sx, sy, tw, th, dx, dy, tw, th)`.

rot.js `Tile` backend implements this directly. No glyph caching needed since the source is already
a pre-rendered bitmap.

For colorization of monochrome tilesets:

1. Draw the tile to a temporary canvas.
2. Apply foreground color via `globalCompositeOperation = 'source-atop'` + `fillRect`.
3. Apply background color via `globalCompositeOperation = 'destination-over'` + `fillRect`.
4. Blit the colorized result to the main canvas.

#### 6c. Monospace Enforcement

For terminal rendering, all cells must have uniform width. Measure the "W" character (widest ASCII
glyph) to determine cell width:

```rust
ctx.set_font(&format!("{}px {}", font_size, font_family));
let metrics = ctx.measure_text("W")?;
let cell_width = metrics.width().ceil() as u32;
let cell_height = font_size; // or compute from ascent + descent
```

rot.js does this in `_updateSize`:

```typescript
const charWidth = Math.ceil(this._ctx.measureText('W').width);
this._spacingX = Math.ceil(opts.spacing * charWidth);
this._spacingY = Math.ceil(opts.spacing * opts.fontSize);
```

### 7. How rot.js and xterm.js Implement This

#### 7a. rot.js Architecture

```text
Backend (abstract)
  └── Canvas (abstract, creates <canvas>, sets font, handles events)
        ├── Rect    -- monospace text grid, per-cell fillText with optional glyph cache
        ├── Hex     -- hexagonal grid layout
        └── Tile    -- spritesheet-based, drawImage from tileset
```text

- **Rect backend** has a static `cache` boolean flag. When enabled, creates one HTMLCanvasElement

  per unique `(char, fg, bg)` triple.

- Cell size computed from `measureText("W")` and `fontSize * spacing`.
- `draw(data, clearBefore)` is called per-cell. No frame batching; the Display class calls draw for

  each dirty cell individually.

- Font is set once in `setOptions()`, not per-draw.
- Uses `textAlign: "center"` and `textBaseline: "middle"` to center glyphs in cells.

#### 7b. xterm.js Architecture

xterm.js separates the texture atlas (glyph rendering) from the renderer (composition):

```text
TextureAtlas (shared)
  ├── Multi-page atlas with row packing
  ├── FourKeyMap cache: (code, bg, fg, ext) -> IRasterizedGlyph
  ├── Idle-time ASCII warm-up
  ├── Background color clearing for transparency support
  ├── Tight bounding box computation via pixel scanning
  └── Page merging (quad-merge of 4 same-sized pages into 1 at 2x)

IRenderer interface
  ├── renderRows(start, end) -- dirty row range
  ├── handleDevicePixelRatioChange() -- DPI awareness
  ├── handleResize() -- terminal size change
  ├── handleSelectionChanged() -- selection overlay
  └── clearTextureAtlas() -- invalidate all cached glyphs
```rust

The WebGL renderer uses the atlas as GPU textures and draws quads. A Canvas 2D renderer (which
existed in older versions as `addon-canvas`, now removed from mainline) would use `drawImage` to
blit from atlas pages.

**Key technique in xterm.js**: the `clearColor` function strips the background color from rendered
glyphs, making them transparent. This allows compositing the glyph over any background, which is why
the atlas can be shared between cells with different backgrounds. The glyph is rendered with a known
background, then those pixels are set to alpha=0.

### 8. Trade-offs: Canvas 2D vs WebGL vs DOM

| Aspect                     | Canvas 2D                                                          | WebGL                                                    | DOM (spans/divs)                                   |
| -------------------------- | ------------------------------------------------------------------ | -------------------------------------------------------- | -------------------------------------------------- |
| **Setup complexity**       | Low. 2D context, fillText, drawImage.                              | High. Shaders, texture uploads, vertex buffers.          | Lowest. Just HTML/CSS.                             |
| **Draw call overhead**     | Medium. One drawImage per cell.                                    | Low. Instanced/batched quads.                            | Very high. DOM mutation + layout + paint per cell. |
| **Text rendering quality** | Good. Browser's native text rasterizer.                            | Good (via atlas). Same rasterizer for atlas generation.  | Best. Full CSS text rendering, ligatures, hinting. |
| **Max throughput**         | ~10K-50K cells/frame at 60fps.                                     | ~100K+ cells/frame at 60fps.                             | ~1K-5K cells/frame at 60fps.                       |
| **Memory**                 | Medium. Atlas canvases + glyph cache.                              | Higher. GPU textures + atlas.                            | High. DOM nodes + style objects.                   |
| **Accessibility**          | Poor. Canvas is opaque to screen readers. Needs aria overlay.      | Poor. Same as Canvas 2D.                                 | Good. DOM nodes are accessible by default.         |
| **Selection/copy**         | Must implement custom selection. No native browser text selection. | Same as Canvas 2D.                                       | Native browser selection works.                    |
| **Ligatures**              | Difficult. Must detect and render multi-char sequences.            | Same difficulty, atlas-based.                            | Native CSS support.                                |
| **Browser compat**         | Universal.                                                         | WebGL2 is universal in modern browsers. WebGPU emerging. | Universal.                                         |
| **HiDPI**                  | Manual. Must scale canvas by devicePixelRatio.                     | Manual. Same scaling needed.                             | Automatic via CSS.                                 |
| **Fallback**               | Good fallback for WebGL.                                           | Primary choice for performance.                          | Good fallback for accessibility.                   |
| **Rust/WASM overhead**     | Each drawImage = 1 FFI call (~100ns).                              | Fewer FFI calls (batch uploads).                         | Each DOM mutation = 1+ FFI calls.                  |
| **Custom glyphs**| Easy. Draw anything on the atlas canvas.                           | Easy. Same atlas approach.                               | Hard. Limited to CSS/SVG.                          |**When to choose Canvas 2D:** |

- As a fallback when WebGL context creation fails.
- When targeting environments with WebGL restrictions.
- When implementation simplicity is prioritized over peak performance.
- For roguelike/tile-based games where cell counts are modest (<10K).
- When you want the same atlas infrastructure to serve both Canvas 2D and WebGL renderers (xterm.js

  pattern).

### When to prefer WebGL

- Large terminal grids (100+ columns, 50+ rows).
- High refresh rate requirements.
- When custom shader effects (glow, CRT simulation) are desired.
- When the atlas is already built (Canvas 2D fillText generates glyphs; WebGL just uploads

  textures).

### When to prefer DOM

- Accessibility is critical.
- Small grids where performance is not a concern.
- When native text selection and copy-paste are required.
- When CSS styling flexibility is needed.

### 9. Recommended Architecture for a Rust WASM Canvas 2D Backend

Based on this research, the recommended approach:

```text
┌─────────────────────────────────────────────┐
│                 GlyphAtlas                  │
│  - OffscreenCanvas pages (512x512 initial)  │
│  - HashMap<GlyphKey, GlyphEntry> cache      │
│  - Render glyph via fillText on tmp canvas  │
│  - Find bounding box, pack into atlas page  │
│  - Idle-warm ASCII 33-126                   │
└─────────────┬───────────────────────────────┘
              │ drawImage (atlas -> screen)
┌─────────────▼───────────────────────────────┐
│             Canvas2dRenderer                │
│  - Visible HtmlCanvasElement                │
│  - DirtyTracker (per-row or per-cell)       │
│  - renderFrame(): iterate dirty cells,      │
│    look up glyph in atlas, drawImage blit   │
│  - Scroll optimization via canvas self-copy │
│  - requestAnimationFrame loop               │
└─────────────────────────────────────────────┘
```text

**GlyphKey** should be: `(char_or_code: u32, fg: u32, bg: u32, flags: u16)` where flags encode
bold/italic/underline/strikethrough.

**Atlas packing**: Use xterm.js's row-packing strategy. Start with 512x512 pages. Create new pages
as needed. Optionally merge pages when count exceeds a threshold.

**Frame loop**:

1. `requestAnimationFrame` callback.
2. Diff current grid state against previous frame (or consume dirty events from the grid model).
3. For each dirty cell: look up glyph in atlas (render if missing), blit via `drawImage`.
4. Batch state changes: set `fillStyle` for background rectangles, then set for foreground text, to

   minimize context state switches.

## Sources

- **Kept**:

  [MDN Drawing text](https://developer.mozilla.org/en-US/docs/Web/API/Canvas_API/Tutorial/Drawing_text)
  -- canonical reference for fillText/strokeText/measureText

- **Kept**: [MDN TextMetrics](https://developer.mozilla.org/en-US/docs/Web/API/TextMetrics) --

  detailed metrics API documentation

- **Kept**: [MDN OffscreenCanvas](https://developer.mozilla.org/en-US/docs/Web/API/OffscreenCanvas)

  -- OffscreenCanvas API and usage patterns

- **Kept**: [MDN FontFace](https://developer.mozilla.org/en-US/docs/Web/API/FontFace) --

  programmatic font loading API

- **Kept**: [rot.js canvas.ts](https://github.com/ondras/rot.js/blob/master/src/display/canvas.ts)

  -- base canvas backend, font setup, event handling

- **Kept**: [rot.js rect.ts](https://github.com/ondras/rot.js/blob/master/src/display/rect.ts) --

  glyph cache implementation, cell sizing from measureText

- **Kept**: [rot.js tile.ts](https://github.com/ondras/rot.js/blob/master/src/display/tile.ts) --

  tileset blitting, colorization via composite operations

- **Kept**:

  [xterm.js TextureAtlas.ts](https://github.com/xtermjs/xterm.js/blob/master/addons/addon-webgl/src/TextureAtlas.ts)
  -- production-grade multi-page atlas, glyph rendering pipeline, cache warm-up, background clearing

- **Kept**:

  [xterm.js renderer Types.ts](https://github.com/xtermjs/xterm.js/blob/master/src/browser/renderer/shared/Types.ts)
  -- IRenderer interface with renderRows dirty-row API

- **Kept**:

  [web-sys OffscreenCanvas](https://docs.rs/web-sys/latest/web_sys/struct.OffscreenCanvas.html) --
  Rust bindings for OffscreenCanvas

- **Kept**:

  [web-sys CanvasRenderingContext2d](https://docs.rs/web-sys/latest/web_sys/struct.CanvasRenderingContext2d.html)
  -- Rust bindings for Canvas 2D context

- **Dropped**: [canvas-styled-text](https://github.com/loganzartman/canvas-styled-text) --

  multi-font text layout library, not relevant to monospace grid rendering

- **Dropped**: [canvas-hypertxt](https://github.com/glideapps/canvas-hypertxt) -- text wrapping

  library, not relevant

- **Dropped**: [Ben Nadel cross-browser text positioning](https://www.bennadel.com/blog/4320) --

  deals with cross-browser offset issues, marginal relevance

- **Dropped**: [TheLinuxCode Canvas guides](https://thelinuxcode.com/) -- SEO-heavy tutorial

  content, no novel information beyond MDN

## Gaps

1. **Canvas 2D renderer removal from xterm.js**: The `addon-canvas` was removed from xterm.js's

   mainline in favor of the WebGL-only addon. Could not access the historical canvas renderer source
   code to examine how it consumed the TextureAtlas for 2D blitting specifically. The WebGL addon's
   atlas is designed for GPU texture upload, but the same atlas can serve Canvas 2D via `drawImage`.

1. **WASM FFI call overhead benchmarks**: No concrete benchmarks found for the overhead of calling

   `drawImage` from Rust/WASM via wasm-bindgen at high frequency. The theoretical overhead is
   ~100-200ns per call, but real-world numbers with 10K+ calls/frame need profiling. A JS shim that
   batches draw commands into a typed array could reduce this.

1. **OffscreenCanvas font rendering caveats**: Some reports suggest that `fillText` on

   OffscreenCanvas may have different anti-aliasing behavior than on HTMLCanvasElement in some
   browsers. This needs testing, especially in Web Worker contexts where font loading APIs may
   behave differently.

1. **Web Worker rendering path**: While OffscreenCanvas supports Web Worker usage, the interaction

   between WASM running in a worker, canvas rendering, and input handling on the main thread needs
   architectural design work. SharedArrayBuffer or message passing for grid state synchronization
   would be needed.

1. **Canvas 2D performance with large atlases**: No data on whether `drawImage` from a very large

   (4096x4096) source canvas has performance implications compared to smaller source canvases. GPU
   texture size limits and upload costs may matter.
