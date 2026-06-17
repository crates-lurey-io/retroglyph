# Research: Headless Backend for a Rust Terminal/Grid Rendering Library

## Summary

A headless backend is a backend implementation that renders to an in-memory cell buffer with no GPU,
window system, or terminal dependency. It implements the same trait as visual backends but stores
output in a `Vec<Cell>` grid. This is a well-established pattern: ratatui's `TestBackend`,
xterm.js's `@xterm/headless`, termwiz's `Surface`, and Playwright's headless mode all demonstrate
that decoupling rendering logic from display output is the right architectural move for testing, CI,
AI agents, and server-side use cases.

## 1. Use Cases

### Testing and CI

The primary use case. A headless backend lets you write deterministic assertions against rendered
output without needing a terminal or display server. CI environments (GitHub Actions, etc.) have no
TTY; a headless backend lets tests run anywhere.

- **Unit tests**: Assert that a widget renders specific characters/colors at specific positions.
- **Integration tests**: Render a full UI frame, then compare the buffer against expected output.
- **Snapshot testing**: Serialize the buffer to a stable text format and compare against golden
  files.
- **Regression tests**: Detect unintended visual changes in CI.

### AI Agents and Automation

AI agents operating in terminal environments need to "see" the screen without a display. A headless
backend provides programmatic read access to the rendered grid, which can be fed to an LLM as text
or as structured cell data.

### Server-Side Rendering

Generate terminal UI output on a server (e.g. for web-based terminal viewers, documentation
screenshots, or preview rendering). The buffer can be serialized to HTML with styled `<span>`
elements or to ANSI escape sequences for replay.

### Screenshot and Recording Generation

Render frames to the in-memory buffer, then export to PNG via software rasterization (using a font +
glyph renderer). This enables generating terminal screenshots for documentation without a running
terminal. Frame sequences can produce GIFs or video.

### Headless Game Servers / Simulation

For roguelike or grid-based games, run the simulation and produce rendered frames without any
display, useful for game AI training, replay validation, or dedicated servers.

## 2. Implementation Approach

The core idea is trivial: the backend is just a `Buffer` (a `Vec<Cell>` representing a width x
height grid) plus cursor state.

### Data Structure

```rust
pub struct HeadlessBackend {
    buffer: Buffer,           // width * height grid of Cells
    cursor_position: Position,
    cursor_visible: bool,
    // Optional:
    scrollback: Buffer,       // for scroll history
}
```

Where `Buffer` is:

```rust
pub struct Buffer {
    pub area: Rect,           // x, y, width, height
    pub content: Vec<Cell>,   // length == width * height
}
```

And `Cell` contains:

```rust
pub struct Cell {
    symbol: String,           // the grapheme displayed
    fg: Color,
    bg: Color,
    modifiers: Modifier,      // bold, italic, underline, etc.
}
```

### What It Does NOT Need

- No GPU context, no OpenGL/Vulkan/Metal
- No window system (no winit, no X11/Wayland/Win32)
- No terminal handle (no stdout, no crossterm/termion)
- No font loading or text shaping (unless exporting to PNG)
- No event loop

### What It Does

- Accepts draw calls (iterator of `(x, y, &Cell)`) and writes them into the buffer
- Tracks cursor position and visibility
- Implements clear/clear_region by resetting cells
- Reports a fixed size (configured at construction)
- `flush()` is a no-op
- Optionally tracks scrollback for `append_lines` semantics

## 3. How Other Libraries Handle Headless Mode

### ratatui's TestBackend (Rust)

The most directly relevant prior art. ratatui defines a `Backend` trait and provides `TestBackend`
as a concrete implementation.

**Key design points:**

- Implements the full `Backend` trait with `type Error = Infallible` (operations never fail).
- Internal state: `buffer: Buffer`, `scrollback: Buffer`, `cursor: bool`, `pos: (u16, u16)`.
- `draw()` iterates `(x, y, &Cell)` and clones each cell into the buffer.
- `flush()` is a no-op.
- `clear()` calls `buffer.reset()`.
- `clear_region()` handles all `ClearType` variants (All, AfterCursor, BeforeCursor, CurrentLine,
  UntilNewLine) by resetting slices of the content vector.
- `append_lines()` implements real scroll behavior, moving lines from the main buffer into the
  scrollback buffer.
- `scroll_region_up/down` implement ANSI-style scrolling regions (behind a feature flag).
- Provides assertion helpers: `assert_buffer_lines()`, `assert_scrollback_lines()`,
  `assert_cursor_position()`.
- Implements `Display` for human-readable buffer visualization.
- Supports `Serialize`/`Deserialize` via serde (behind feature flag).
- Implements `Clone`, `Eq`, `Hash`.

**What ratatui gets right:**

- The test backend is a _full_ backend, not a subset. Any code that works with the test backend
  works identically with a real backend.
- Assertion helpers make tests concise.
- Serde support enables snapshot testing.

**Source:**
[ratatui-core/src/backend/test.rs](https://github.com/ratatui/ratatui/blob/main/ratatui-core/src/backend/test.rs),
[docs.rs/ratatui](https://docs.rs/ratatui/latest/ratatui/backend/struct.TestBackend.html)

### xterm.js @xterm/headless (TypeScript)

xterm.js provides a separate `@xterm/headless` npm package that exposes the same `Terminal` API
without any DOM dependency.

**Key design points:**

- The headless terminal is a full terminal emulator (parses ANSI/VT sequences, manages a buffer grid
  with scrollback) but has no renderer.
- The public API is identical to the browser terminal: `write()`, `resize()`, `buffer` namespace,
  `parser`, event handlers (`onData`, `onLineFeed`, `onResize`, etc.).
- The `buffer` property exposes the same `IBufferNamespace` interface, letting callers read cell
  content, cursor position, and scrollback.
- Uses the same internal `TerminalCore` class as the browser version; the only difference is the
  absence of a DOM renderer.
- Supports addons (with a caveat: addons that call renderer APIs will break).

**What xterm.js gets right:**

- Single core, multiple surfaces. The headless package is not a reimplementation; it shares code
  with the visual terminal.
- The API contract is identical, so code tested against headless works in the browser.

**Source:**
[xterm.js/src/headless/public/Terminal.ts](https://github.com/xtermjs/xterm.js/blob/master/src/headless/public/Terminal.ts)

### termwiz Surface (Rust, from wezterm)

termwiz's `Surface` type is conceptually a headless terminal buffer:

- It represents screen contents as a grid of cells, not connected to any terminal device.
- It maintains a change log; you can call `get_changes()` to get an optimized diff since the last
  render.
- Surfaces can be composited together (layering/widget composition).
- `draw_from_screen()` computes minimal diffs between two surfaces.
- The Surface is used both as the internal screen model and for testing.

**What termwiz gets right:**

- The change-log/diff model is interesting for incremental rendering.
- Surface composition supports widget layering without a terminal.

**Source:**
[docs.rs/termwiz/surface](https://docs.rs/termwiz/latest/termwiz/surface/struct.Surface.html)

### Playwright Headless Mode (Browser Automation)

Playwright launches real browsers in headless mode (no visible window) for testing. The key insight:

- The browser engine runs the same rendering pipeline, just without displaying to a screen.
- Screenshots and page content are accessible via API.
- The headless mode is the _default_; headed mode is opt-in.

**What Playwright gets right:**

- Making headless the default normalizes it. Tests are headless first, visual second.
- Screenshot capture is a first-class API, not an afterthought.

**Source:** [Playwright docs](https://playwright.dev/docs/api/class-browsertype)

## 4. API Design Considerations

### Should It Match the Visual Backend API Exactly?

**Yes, with minor additions.** The headless backend must implement the same trait as visual
backends. This is the entire point: code that renders to the headless backend should work
identically when swapped to a GPU/terminal backend.

The `Backend` trait methods that must be implemented:

- `draw()` - write cells to the buffer
- `hide_cursor()` / `show_cursor()` - toggle cursor visibility flag
- `get_cursor_position()` / `set_cursor_position()` - read/write cursor position
- `clear()` / `clear_region()` - reset cells
- `size()` / `window_size()` - return configured dimensions
- `flush()` - no-op
- `append_lines()` - scroll behavior
- `scroll_region_up/down()` - scrolling region support

**Additional headless-only API** (not on the trait):

- `buffer() -> &Buffer` - read access to the cell grid
- `scrollback() -> &Buffer` - read access to scroll history
- `cursor_visible() -> bool` - query cursor state
- `resize(width, height)` - change dimensions (visual backends get this from the OS)
- `assert_buffer_lines(expected)` - testing helper
- `to_ansi() -> String` - export as ANSI escape sequences
- `to_html() -> String` - export as styled HTML
- `to_png(font, size) -> Vec<u8>` - software-rasterized screenshot

### Error Type

Use `Infallible` (or a custom never-type). The headless backend has no I/O, so operations cannot
fail. This matches ratatui's approach.

### Construction

```rust
// Fixed size
let backend = HeadlessBackend::new(80, 24);

// From existing content (for test setup)
let backend = HeadlessBackend::with_lines(["hello", "world"]);
```

## 5. Exporting / Serializing the Buffer

### ANSI Escape Sequences

Walk the buffer row by row. For each cell, emit SGR codes for foreground, background, and modifiers,
then the character. Optimize by only emitting SGR changes when the style actually differs from the
previous cell.

```rust
fn to_ansi(&self) -> String {
    let mut out = String::new();
    let mut prev_style = Style::default();
    for y in 0..self.height {
        for x in 0..self.width {
            let cell = &self.buffer[(x, y)];
            let style = cell.style();
            if style != prev_style {
                out.push_str(&style.to_ansi_sgr());
                prev_style = style;
            }
            out.push_str(cell.symbol());
        }
        out.push_str("\r\n");
    }
    out.push_str("\x1b[0m"); // reset
    out
}
```

This output can be written to a file and `cat`'d in a terminal for visual inspection, or piped to
tools that consume ANSI.

### HTML Export

Map each cell to a `<span>` with inline styles or CSS classes:

```html
<pre class="terminal">
<span style="color:#ff0000;background:#000000;font-weight:bold">H</span>
<span style="color:#00ff00">e</span>...
</pre>
```

Alternatively, group consecutive cells with the same style into a single `<span>` for smaller
output.

### Plain Text

Strip all styling and return just the characters. Useful for text-based assertions and
accessibility.

### JSON / Serde

Serialize the entire buffer (area + cells with styles) to JSON. Useful for:

- Snapshot test comparison
- Sending buffer state over a network (web viewer, AI agent)
- Recording frame sequences

ratatui's `Buffer` already derives `Serialize`/`Deserialize`.

### PNG via Software Rasterization

Render the buffer to a pixel image without a GPU:

1. **Font loading**: Use `ab_glyph` or `fontdue` to load a monospace font.
2. **Glyph rasterization**: For each cell, rasterize the glyph to a bitmap.
3. **Compositing**: Paint glyphs onto a pixel buffer with the cell's fg/bg colors.
4. **Encoding**: Use `png` crate to write the pixel buffer.

Alternatively, use `tiny-skia` (a pure-Rust 2D rendering library, no GPU needed) for higher quality
output with anti-aliasing.

```rust
fn to_png(&self, font: &Font, cell_size: (u32, u32)) -> Vec<u8> {
    let (cw, ch) = cell_size;
    let width = self.width as u32 * cw;
    let height = self.height as u32 * ch;
    let mut pixmap = tiny_skia::Pixmap::new(width, height).unwrap();

    for y in 0..self.height {
        for x in 0..self.width {
            let cell = &self.buffer[(x, y)];
            // Fill background
            fill_rect(&mut pixmap, x*cw, y*ch, cw, ch, cell.bg);
            // Render glyph
            render_glyph(&mut pixmap, font, cell.symbol(), x*cw, y*ch, cell.fg);
        }
    }

    pixmap.encode_png().unwrap()
}
```

**Crate options for software rasterization:**

- `tiny-skia` - Pure Rust, Skia subset, good for 2D rendering.
  [docs.rs/tiny-skia](https://docs.rs/tiny-skia/latest/tiny_skia/)
- `ab_glyph` - Font parsing and glyph rasterization.
  [docs.rs/ab_glyph](https://docs.rs/ab_glyph/latest/ab_glyph/)
- `fontdue` - Lightweight font rasterizer, faster than ab_glyph for simple cases.
- `resvg` - SVG renderer built on tiny-skia, if SVG export is desired.

### SVG Export

Generate an SVG with `<text>` elements and style attributes. This produces resolution-independent
output that can be embedded in documentation.

### Recording (GIF/Video)

Capture a sequence of buffer snapshots over time (frame + timestamp), then:

- Export frames as PNGs and stitch with `ffmpeg`
- Use the `gif` crate to produce animated GIFs directly
- Store as a sequence of ANSI frames playable by `asciinema` or similar

## 6. Prior Art in Rust

### ratatui TestBackend

See section 3. Full `Backend` trait implementation. The canonical example of this pattern in the
Rust terminal ecosystem. Part of `ratatui-core`, so it's available even without pulling in the full
ratatui crate.

### crossterm

crossterm itself does not provide a mock/headless backend. It operates directly on a `Write` handle
(typically stdout). However, you can pass any `Write` impl, including a `Vec<u8>`, to capture raw
ANSI output. This captures escape sequences rather than parsed cell state, making assertions harder.

### termwiz Surface

See section 3. Not labeled as a "test backend" but serves the same purpose. The change-log based
diff system is more sophisticated than ratatui's direct buffer model.

### alacritty/vte

A ANSI/VT parser crate. Not a backend per se, but relevant for a headless backend that needs to
_parse_ incoming ANSI sequences (as opposed to _producing_ them). If the headless backend needs to
accept raw terminal output (like xterm.js headless does), vte provides the parser state machine.

### BearLibTerminal

The original C library that inspired many roguelike terminals. It had no headless mode, which is a
gap this design addresses. BearLibTerminal's API was purely visual (create window, put character,
refresh). A Rust successor should learn from this limitation.

## 7. Trade-offs

### Simplicity vs. Fidelity

| Aspect             | Simple (ratatui-style)                        | Full emulator (xterm.js-style)                        |
| ------------------ | --------------------------------------------- | ----------------------------------------------------- |
| **Implementation** | Buffer + cursor, a few hundred lines          | Full VT parser, scrollback, modes, thousands of lines |
| **Use case**       | Testing rendering code                        | Testing terminal interaction, parsing ANSI input      |
| **Input handling** | None (caller writes cells directly)           | Parses ANSI/VT escape sequences                       |
| **Accuracy**       | Matches the library's rendering model exactly | Matches real terminal behavior                        |
| **Complexity**     | Minimal                                       | Substantial                                           |

**Recommendation**: Start with the simple model (direct cell writes). If ANSI-input parsing is
needed later, it can be layered on top using `vte` or a similar parser.

### Error Type: Infallible vs. Boxed

Using `Infallible` means the headless backend cannot fail, which is true and lets callers
`.unwrap()` freely. The trade-off is that if the `Backend` trait uses an associated error type, the
headless backend's error type doesn't unify with real backends. ratatui handles this by having
`TestBackend` use `Infallible`, which works because the `Terminal` struct is generic over the
backend.

### Scrollback Tracking

Tracking scrollback adds complexity but enables:

- Testing scroll behavior
- Full terminal recording
- AI agents reading scroll history

If scrollback is optional, make it opt-in via a builder method or feature flag.

### Buffer Export: Lazy vs. Eager

Should export methods (to_ansi, to_html, to_png) live on the backend, on the buffer, or as separate
utility functions?

**Recommendation**: Put them on the `Buffer` type or as free functions. The backend provides
`buffer() -> &Buffer`; export is a separate concern. This avoids bloating the backend with rendering
dependencies (fonts, image codecs) and lets users who only need testing avoid pulling in those
crates.

### Thread Safety

The headless backend should be `Send + Sync` if possible. ratatui's `TestBackend` is both. This
enables use in async test frameworks and multi-threaded simulations.

### Feature Gating

Consider feature flags:

- `headless` (or always-on) - the core headless backend
- `headless-ansi` - ANSI export
- `headless-html` - HTML export
- `headless-png` - PNG export (pulls in tiny-skia, ab_glyph)
- `headless-serde` - JSON serialization

This keeps the dependency tree small for users who only need the buffer for testing.

## Sources

- **Kept:**
  - ratatui TestBackend source
    ([test.rs](https://github.com/ratatui/ratatui/blob/main/ratatui-core/src/backend/test.rs)) -
    Primary implementation reference, full source reviewed
  - ratatui Backend trait
    ([backend.rs](https://github.com/ratatui/ratatui/blob/main/ratatui-core/src/backend.rs)) - Trait
    definition with all required methods
  - ratatui Buffer docs
    ([docs.rs](https://docs.rs/ratatui-core/latest/ratatui_core/buffer/struct.Buffer.html)) -
    Buffer/Cell data model
  - xterm.js headless Terminal
    ([Terminal.ts](https://github.com/xtermjs/xterm.js/blob/master/src/headless/public/Terminal.ts)) -
    Headless terminal with identical API
  - termwiz Surface
    ([docs.rs](https://docs.rs/termwiz/latest/termwiz/surface/struct.Surface.html)) - Change-log
    based surface model
  - tiny-skia ([docs.rs](https://docs.rs/tiny-skia/latest/tiny_skia/)) - Pure Rust software
    rasterizer for PNG export
  - ab_glyph ([docs.rs](https://docs.rs/ab_glyph/latest/ab_glyph/)) - Font/glyph rasterization for
    PNG export
  - alacritty/vte ([GitHub](https://github.com/alacritty/vte)) - VT parser if ANSI input parsing
    needed

- **Dropped:**
  - crossterm source - No mock/headless backend exists; only raw Write capture
  - Playwright docs - Useful analogy but not directly applicable to terminal grid rendering
  - hyperjson - Unrelated (JSON library, wrong search result)

## Gaps

1. **Real-world benchmarks** of headless backends under load (e.g., rendering thousands of frames
   for recording) could not be found. Performance of the simple buffer approach at scale is assumed
   good but unverified.
2. **AI agent integration patterns** for reading headless terminal buffers are not well-documented
   in the Rust ecosystem. This is a novel use case.
3. **SVG export implementations** in the terminal space were not found; this would need to be built
   from scratch.
4. **Animated recording formats** (asciicast, etc.) and their integration with headless backends
   were not deeply researched. The `asciinema` format is documented but no Rust library was found
   that produces it from a headless buffer.
5. **Input simulation** (sending keystrokes to the headless backend for full round-trip testing) is
   a separate concern not covered here. ratatui's TestBackend does not handle input; that would
   require an event source abstraction.
