# Research: HTML DOM Backend for a Rust Terminal/Grid Rendering Library

## Summary

An HTML DOM backend renders a cell grid as `<span>` elements within row containers (`<pre>` or
`<div>`), with per-cell inline CSS for colors, modifiers, and sizing. This approach is the simplest
to implement, provides native text selection, browser find (Ctrl+F), accessibility, and CSS styling
for free, but is the slowest rendering path for large or frequently-updating terminals. xterm.js
shipped a DOM renderer from v3 onwards but made canvas the default (then WebGL) because DOM
rendering could not keep up with high-throughput terminal output. Ratzilla (Rust/WASM) currently
ships a DomBackend alongside Canvas and WebGL2 backends, providing a direct reference
implementation.

## 1. Implementation Approaches

Three primary DOM layout strategies exist for rendering a terminal cell grid:

### 1a. Rows of inline spans (used by xterm.js and ratzilla)

The dominant approach. Each row is a block container (`<div>` or `<pre>`), containing inline
`<span>` elements for character runs. Adjacent cells sharing the same attributes (fg, bg, modifiers)
are merged into a single span.

```
<div class="xterm-rows">
  <div style="height: 20px; line-height: 20px;">
    <span class="xterm-fg-2">hello </span>
    <span class="xterm-fg-7 xterm-bold">world</span>
  </div>
  ...
</div>
```

Pros:

- Natural text flow; browser handles horizontal positioning
- Merging adjacent same-attribute cells reduces DOM node count
- `white-space: pre` preserves spaces without `&nbsp;` hacks
- Text selection works across merged spans

Cons:

- `display: inline-block` on spans creates ~20% render penalty (noted in xterm.js source as a TODO)
- `letter-spacing` corrections needed to match monospace grid alignment
- Each cell or merged run is a separate DOM node, leading to thousands of nodes for large terminals

### 1b. CSS Grid layout

Use `display: grid` with `grid-template-columns: repeat(cols, 1ch)` to position cells.

```
<div style="display: grid; grid-template-columns: repeat(80, 1ch);">
  <span style="grid-column: 1; grid-row: 1;">h</span>
  ...
</div>
```

Pros:

- Precise cell positioning without letter-spacing hacks
- Can skip empty cells (sparse rendering)
- No `inline-block` penalty

Cons:

- One DOM element per cell (no merging adjacent cells of the same style without spanning)
- Browser layout engine must solve the grid constraints, which is expensive for 80x24 = 1920+
  elements
- Text selection order may not follow visual reading order
- No real-world terminal emulator uses this approach for good reason

### 1c. Absolutely positioned elements

Each cell is `position: absolute` with computed `left`/`top` pixel offsets.

```
<div style="position: relative;">
  <span style="position: absolute; left: 0px; top: 0px;">h</span>
  <span style="position: absolute; left: 10px; top: 0px;">e</span>
  ...
</div>
```

Pros:

- Pixel-perfect positioning
- Can do sparse updates (only create/update changed cells)

Cons:

- Maximum DOM node count (one per cell)
- No text selection (elements not in document flow)
- Browser find (Ctrl+F) may not work correctly
- Layout thrashing from setting per-element positions
- Basically reinventing canvas with worse performance

**Recommendation**: Rows of inline spans is the proven approach. It balances DOM node count, text
selection, and implementation simplicity.

## 2. How xterm.js's DOM Renderer Works

Source: `src/browser/renderer/dom/DomRenderer.ts` and `DomRendererRowFactory.ts` (analyzed at HEAD,
MIT license).

### Architecture

The xterm.js DomRenderer has two main classes:

1. **`DomRenderer`**: Manages the overall DOM structure, CSS injection, selection overlays, and row
   element lifecycle.
2. **`DomRendererRowFactory`**: Generates `HTMLSpanElement[]` arrays for individual buffer lines.

### DOM structure

```
<div class="xterm-dom-renderer-owner-{id}">
  <div class="xterm-screen">
    <div class="xterm-rows" aria-hidden="true">
      <div>  <!-- row 0 -->
        <span>text</span>
        <span class="xterm-fg-2 xterm-bold">more text</span>
      </div>
      <div>  <!-- row 1 -->
        ...
      </div>
    </div>
    <div class="xterm-selection" aria-hidden="true">
      <!-- absolutely positioned selection highlight divs -->
    </div>
    <style>  <!-- theme CSS -->
    <style>  <!-- dimension CSS -->
  </div>
</div>
```

### Key implementation details

- **Row elements are `<div>`s**, pre-created for all visible rows. Row content is replaced via
  `element.replaceChildren(...spans)` on each render.
- **Cell merging**: `DomRendererRowFactory.createRow()` iterates cells left to right. If the next
  cell has identical `bg`, `fg`, `ext` attributes, same hover state, same letter-spacing, is not a
  cursor cell, and is not part of a ligature, its text is appended to the current `<span>` instead
  of creating a new one. This reduces DOM node count significantly.
- **CSS classes for palette colors**: `xterm-fg-{0-255}` and `xterm-bg-{0-255}` classes are injected
  via a `<style>` element. RGB colors use inline `style` attributes.
- **Font metrics**: Uses `display: inline-block` on spans with computed `letter-spacing` corrections
  from a `WidthCache` that measures actual glyph widths. The comment in the source explicitly notes
  inline-block creates "~20% render penalty" but no workaround has been found.
- **Selection rendering**: Handled via absolutely positioned `<div>`s in a separate container
  overlaid on the text, not via altering span styles.
- **Cursor**: Rendered via CSS classes (`xterm-cursor-block`, `xterm-cursor-bar`,
  `xterm-cursor-underline`) with CSS animations for blinking.
- **Character joiners / ligatures**: Supported via `JoinedCellData` which merges adjacent cells into
  a single span for ligature rendering, with special handling when the cursor is inside a ligature
  range.
- **Minimum contrast**: Adjusts foreground color to ensure WCAG contrast ratios against the resolved
  background, computed per-cell.

### Why xterm.js moved away from DOM rendering

The DOM renderer is described in the source as "the standard renderer and fallback for when the
webgl addon is slow. This is not meant to be particularly fast and will even lack some features such
as custom glyphs."

Key reasons for the move to canvas (then WebGL):

1. **Performance ceiling**: Each `renderRows()` call replaces innerHTML of affected rows. For a
   terminal with rapid output (e.g., `cat` of a large file), this means thousands of DOM mutations
   per second, each triggering layout/paint. Canvas can batch-draw an entire screen in a single
   frame.
2. **DOM node count**: Even with cell merging, a typical 80x24 terminal can have 500-2000 span
   nodes. At 200x50, this could be 5000+ nodes. Canvas has zero DOM overhead.
3. **Inline-block penalty**: The ~20% overhead from `display: inline-block` compounds with reflow
   costs.
4. **Custom glyphs**: Box-drawing characters, powerline symbols, and other special glyphs are drawn
   pixel-perfect on canvas; DOM relies on font rendering which varies across browsers.
5. **GPU acceleration**: WebGL renders text via texture atlases on the GPU. The glyph atlas approach
   means each character is drawn once, then blitted from cache, which is orders of magnitude faster.

The DOM renderer remains as a **fallback** for environments where WebGL/canvas is unavailable or
performs poorly. It is still maintained and tested.

## 3. Performance Characteristics

### DOM vs Canvas vs WebGL (from ratzilla's documented comparison)

| Metric                   | DomBackend                       | CanvasBackend              | WebGL2Backend  |
| ------------------------ | -------------------------------- | -------------------------- | -------------- |
| 60fps on large terminals | No                               | No                         | Yes            |
| Memory usage             | Highest                          | Medium                     | Lowest         |
| CPU per frame            | Highest (DOM mutations + layout) | Medium (canvas draw calls) | Lowest (<1ms)  |
| Browser support          | All                              | All                        | Modern (2017+) |

### Why DOM is slow

1. **Layout thrashing**: Setting `innerHTML`, `textContent`, `className`, and `style` attributes
   triggers the browser's layout engine. If reads (e.g., `getBoundingClientRect`) are interleaved
   with writes, this causes forced synchronous layouts.
2. **Garbage collection**: Creating new `<span>` elements every frame generates GC pressure.
   xterm.js mitigates this by reusing row `<div>` containers and only replacing their children.
3. **Style recalculation**: Each unique combination of inline styles creates a unique computed
   style. With 256 palette colors x modifiers, the style engine does significant work.
4. **Paint complexity**: The browser must composite potentially thousands of overlapping inline
   elements, each with their own background colors.

### When DOM performance is acceptable

- Small grids (under ~100x40 cells)
- Infrequent updates (not streaming output)
- Static or near-static displays (dashboards, forms, menus)
- Mobile/low-power devices where WebGL support may be spotty

### Optimization techniques for DOM rendering

- **Dirty tracking**: Only re-render rows that changed (xterm.js does this via
  `renderRows(start, end)`)
- **Cell merging**: Combine adjacent same-attribute cells into single spans (both xterm.js and
  ratzilla do this)
- **CSS class reuse**: Use CSS classes for the 256 ANSI colors rather than inline `rgb()` styles
  (xterm.js approach)
- **Document fragment batching**: Build row content in a DocumentFragment before appending to the
  live DOM
- **`requestAnimationFrame` throttling**: Batch multiple buffer updates into a single DOM update per
  frame
- **Avoid `innerHTML`**: Use `textContent` for text-only updates, `replaceChildren()` for structural
  changes

## 4. Advantages of DOM Rendering

### 4a. Native text selection

DOM-rendered text is real text nodes in the document. Users can click-and-drag to select, Ctrl+A to
select all, and copy text natively. Canvas/WebGL renderers must implement custom selection overlays
with invisible text layers (which is complex and never quite matches native behavior).

Ratzilla comparison: DomBackend supports linear text selection natively; CanvasBackend has no text
selection; WebGL2Backend implements its own linear/block selection.

### 4b. Accessibility

Screen readers can traverse DOM text nodes. With proper ARIA attributes, a DOM-rendered terminal can
be accessible. Canvas is an opaque bitmap to assistive technology; making it accessible requires a
parallel invisible DOM (which xterm.js does maintain separately).

### 4c. Browser find (Ctrl+F)

The browser's built-in find function searches DOM text nodes. Users can Ctrl+F to search terminal
output. This is impossible with canvas/WebGL without custom integration.

### 4d. CSS styling

External stylesheets and browser devtools can inspect and override terminal styling. Users can apply
custom fonts, adjust colors via CSS custom properties, or use browser extensions to restyle the
terminal. Canvas rendering is opaque to CSS.

### 4e. Hyperlinks

DOM elements can be `<a>` tags or have click handlers that feel native. Canvas hyperlinks require
hit-testing pixel coordinates back to the cell grid.

### 4f. Right-to-left and complex text layout

The browser's text shaping engine handles BiDi, combining characters, and complex scripts. Canvas
`fillText` has limited support and requires manual shaping.

### 4g. Simplicity

A DOM backend is the simplest to implement. No shader programs, no texture atlas management, no
canvas context state machine. The web platform does the heavy lifting.

## 5. Getting Rust Code Running in the Browser

### 5a. wasm-bindgen

The foundational crate for Rust-WASM interop. Generates JS glue code for calling Rust functions from
JS and vice versa. Handles type conversions between Rust and JS (strings, numbers, objects,
closures).

```rust
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn greet(name: &str) -> String {
    format!("Hello, {}!", name)
}
```

Key features:

- Import JS functions/classes into Rust
- Export Rust functions/structs to JS
- Automatic TypeScript binding generation
- Closure passing between Rust and JS

[Source: wasm-bindgen guide](https://wasm-bindgen.github.io/wasm-bindgen/)

### 5b. web-sys

Raw bindings to Web APIs, auto-generated from WebIDL specs. Provides typed access to DOM APIs
(`Document`, `Element`, `HtmlCanvasElement`, etc.), events, fetch, WebGL, and everything else in the
browser.

```rust
use web_sys::{window, Document, Element};

let document = window().unwrap().document().unwrap();
let element = document.create_element("span").unwrap();
element.set_inner_html("Hello");
element.set_attribute("style", "color: red;").unwrap();
```

Each API is behind a Cargo feature flag (e.g., `features = ["Document", "Element", "HtmlElement"]`),
so you only pay for what you use in binary size.

### 5c. gloo

Ergonomic wrappers around web-sys/js-sys. Makes browser API usage feel more Rust-native. Key
sub-crates:

- `gloo::events::EventListener` - RAII event listener management
- `gloo::timers` - setTimeout/setInterval wrappers
- `gloo::utils` - document(), window(), body() convenience functions
- `gloo::console` - console.log! macro

```rust
use gloo::events::EventListener;
use gloo::timers::callback::Timeout;

let listener = EventListener::new(&element, "click", move |_event| {
    Timeout::new(1_000, move || {
        // do something after 1 second
    }).forget();
});
```

[Source: gloo docs](https://docs.rs/gloo)

### 5d. Build tooling

- **trunk**: Build tool for Rust WASM apps. Handles compilation, asset bundling, dev server with hot
  reload. Ratzilla uses trunk as its primary build tool.
- **wasm-pack**: Alternative that produces NPM-publishable packages.
- Target: `wasm32-unknown-unknown` (added via `rustup target add wasm32-unknown-unknown`)

### 5e. How ratzilla wires it together

Ratzilla's `DomBackend` uses `web_sys` directly (not gloo) for DOM manipulation:

```rust
// Creating a span for a cell
let span = document.create_element("span")?;
span.set_inner_html(cell.symbol());
span.set_attribute("style", &get_cell_style_as_css(cell))?;
```

The backend implements ratatui's `Backend` trait, with `draw()` iterating over `(x, y, &Cell)`
tuples, updating corresponding DOM elements. Cell sizes are measured by creating a temporary probe
element (`<pre><span>` with a full-block character), measuring it with `getBoundingClientRect()`,
then removing it.

## 6. Prior Art

### 6a. xterm.js DOM Renderer

- **Status**: Maintained as fallback renderer, not the default
- **Architecture**: Row-based `<div>` containers with merged `<span>` children. CSS classes for
  palette colors, inline styles for RGB. Selection via overlay divs.
- **Strengths**: Mature, battle-tested, handles edge cases (ligatures, BiDi, minimum contrast,
  decorations)
- **Weaknesses**: Performance-limited for large terminals, ~20% inline-block penalty, cannot do
  custom glyphs

[Source: xtermjs/xterm.js DomRenderer.ts](https://github.com/xtermjs/xterm.js/blob/master/src/browser/renderer/dom/DomRenderer.ts)

### 6b. Ratzilla (Rust/WASM)

- **Status**: Active development (under ratatui org). The most mature Rust WASM TUI framework.
- **Architecture**: `DomBackend` creates a grid of `<span>` elements inside `<pre>` row elements
  inside a `<div>` grid. Each cell maps 1:1 to a span. Uses `display: inline-block; width: Nch;` for
  sizing.
- **Backends**: DomBackend (DOM), CanvasBackend (Canvas 2D), WebGL2Backend (WebGL2 via beamterm).
  DomBackend is the most compatible but slowest.
- **Cell styling**: Inline CSS computed per cell:
  `color: rgb(r,g,b); background-color: rgb(r,g,b); display: inline-block; width: 1ch;` plus
  modifier-specific styles (bold, italic, underline, etc.)
- **Key difference from xterm.js**: Ratzilla does NOT merge adjacent same-attribute cells. Each cell
  is always its own `<span>`. This simplifies updates (direct index into flat cell array) but
  produces more DOM nodes.
- **Resize handling**: On window resize, the entire grid is torn down and rebuilt. Cell size is
  re-measured via a probe element.
- **Dependencies**: `web-sys`, `wasm-bindgen`, `ratatui`, `unicode-width`

[Source: ratzilla DomBackend](https://docs.rs/ratzilla/latest/ratzilla/backend/dom/struct.DomBackend.html)

### 6c. Webatui

- **Status**: Active development
- **Architecture**: Integration between Yew (Rust web framework) and Ratatui. Renders ratatui output
  as HTML, using Yew's virtual DOM for diffing.
- **Approach**: Yew handles the DOM diffing/patching, so only changed elements are updated. This is
  potentially more efficient than raw DOM manipulation for incremental updates.
- **Features**: Index colors via base16-palettes, hyperlinks, mouse events, automatic screen
  resizing, scrolling

[Source: TylerBloom/webatui](https://github.com/TylerBloom/webatui)

### 6d. blessed / blessed-contrib (Node.js)

- **Status**: Unmaintained (last commit 2017)
- **Architecture**: Terminal UI library for Node.js. Had an experimental browser mode that rendered
  to DOM elements. Used absolutely positioned elements with computed pixel offsets.
- **Relevance**: Demonstrated that a TUI abstraction layer can target both real terminals and
  browser DOM, but the browser rendering was never production-quality.

### 6e. hterm (Google)

- Google's terminal emulator (used in Chrome OS terminal, Secure Shell extension)
- Uses DOM rendering with rows of `<x-row>` custom elements containing `<span>` runs
- Similar span-merging approach to xterm.js
- Has been gradually replaced by newer approaches in ChromeOS

## 7. Trade-offs: When DOM Makes Sense vs Canvas/WebGL

### Choose DOM when

- **Accessibility matters**: Screen readers, keyboard navigation, ARIA support
- **Users need text selection and browser find**: Copy/paste workflows, Ctrl+F search
- **The grid is small**: Under ~100x40 cells, DOM performance is fine
- **Updates are infrequent**: Dashboards, static content, form-like UIs
- **CSS theming is important**: Users or themes need to override styles via CSS
- **Maximum browser compatibility**: DOM works everywhere, including older browsers and restricted
  environments
- **Implementation speed**: DOM is the simplest backend to build
- **The UI is web-native, not a terminal emulator**: TUI-themed web apps (personal sites,
  interactive demos) where terminal faithfulness is secondary to web integration

### Choose Canvas/WebGL when

- **Performance is critical**: Streaming output, animations, large terminals (200+ cols)
- **Pixel-perfect rendering**: Custom glyphs, box-drawing, powerline symbols
- **Frame budget is tight**: Canvas renders an entire screen in <1ms (WebGL) vs 5-50ms (DOM)
- **Building a real terminal emulator**: Latency-sensitive, high-throughput use case
- **Memory efficiency matters**: Canvas/WebGL use fixed-size buffers, DOM node counts grow with
  content

### Hybrid approach

Both xterm.js and ratzilla demonstrate that you can support multiple backends behind a shared
interface. A practical strategy:

1. **Start with DOM** for initial development and correctness testing
2. **Add Canvas/WebGL** when performance profiling shows DOM is the bottleneck
3. **Keep DOM as fallback** for accessibility, testing, and environments where GPU rendering is
   unavailable

The backend abstraction should be a trait/interface that takes a cell grid (2D array of cells with
character, fg, bg, modifiers) and renders it. The DOM backend creates/updates DOM elements; the
canvas backend draws to a 2D context; the WebGL backend uses texture atlases. The cell grid data
model is the same regardless of backend.

## Sources

### Kept

- **xterm.js DomRenderer.ts**
  (<https://github.com/xtermjs/xterm.js/blob/master/src/browser/renderer/dom/DomRenderer.ts>) -
  Primary reference for mature DOM terminal rendering. Full source analyzed.
- **xterm.js DomRendererRowFactory.ts**
  (<https://github.com/xtermjs/xterm.js/blob/master/src/browser/renderer/dom/DomRendererRowFactory.ts>) -
  Cell merging logic, per-cell span generation. Full source analyzed.
- **Ratzilla DomBackend**
  (<https://docs.rs/ratzilla/latest/ratzilla/backend/dom/struct.DomBackend.html>) - Rust WASM DOM
  backend reference implementation. Full source analyzed via docs.rs.
- **Ratzilla backend comparison** (<https://docs.rs/ratzilla/latest/ratzilla/backend/index.html>) -
  Official comparison table of DOM vs Canvas vs WebGL2 backends.
- **Ratzilla utils.rs** (<https://docs.rs/ratzilla/latest/src/ratzilla/backend/utils.rs.html>) -
  Cell-to-CSS conversion, span creation utilities.
- **wasm-bindgen Guide** (<https://wasm-bindgen.github.io/wasm-bindgen/>) - Canonical docs for
  Rust-WASM interop.
- **Gloo toolkit** (<https://docs.rs/gloo>) - Ergonomic web-sys wrappers documentation.
- **Webatui** (<https://github.com/TylerBloom/webatui>) - Alternative Rust WASM TUI approach using
  Yew virtual DOM.
- **Ratzilla crates.io** (<https://crates.io/crates/ratzilla>) - Project overview, examples,
  deployment.

### Dropped

- GitHub issue #2267 on xterm.js - was about a user question on adding click events, not about DOM
  rendering architecture
- GitHub wiki pages for xterm.js - required authentication, couldn't fetch content

## Gaps

1. **Quantitative benchmarks**: No hard numbers comparing DOM vs Canvas frame times at specific grid
   sizes (e.g., "80x24 DOM renders in Xms vs Canvas Yms"). Ratzilla's comparison table says DOM
   can't hit 60fps on large terminals but doesn't define "large" with numbers. Would need to build a
   benchmark harness.

2. **Virtual DOM diffing**: Webatui uses Yew's virtual DOM, which should reduce DOM mutations. No
   performance comparison exists between raw DOM manipulation (ratzilla-style) and vdom-diffed
   updates. This could be a meaningful optimization path.

3. **`DocumentFragment` batching**: Neither xterm.js nor ratzilla use `DocumentFragment` for
   batching row-level updates (xterm.js uses `replaceChildren(...spans)`, ratzilla sets innerHTML
   per-cell). Testing whether fragment batching helps would require benchmarking.

4. **CSS containment**: Using `contain: content` or `content-visibility: auto` on row elements could
   help browsers skip layout/paint for off-screen rows. No terminal renderer appears to use this
   yet.

5. **Web Components / Shadow DOM**: Encapsulating the terminal renderer in a Shadow DOM could
   prevent style leakage and improve encapsulation. No prior art found for this in terminal
   emulators.

6. **WASM DOM manipulation overhead**: Each `web_sys` call crosses the WASM-JS boundary. For
   high-frequency updates, this overhead may be significant. Batching DOM operations into a single
   JS call (via `js_sys::Function` or a thin JS shim) could help. No benchmarks found for this
   specific concern.
