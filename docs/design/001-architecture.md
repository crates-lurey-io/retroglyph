# ADR 001: Architecture

**Status:**Draft**Date:** 2026-06-15

## Context

retroglyph is a Rust rewrite of [BearLibTerminal](https://github.com/tommyettinger/BearLibTerminal), a
pseudo-terminal window library for roguelike games. BearLibTerminal provides a grid of character
cells with Unicode support, tile/font handling, OpenGL rendering, and keyboard/mouse input. It is
unmaintained (last release ~2017).

retroglyph aims to provide the same core abstraction (a cell grid you draw to, with pluggable rendering
backends) while being idiomatic Rust, multi-backend from the start, and incrementally extensible.

This ADR captures the foundational architecture decisions.

## Decision

### What retroglyph is

A Rust library that gives you a **grid of character cells** with styled output, input handling, and
double-buffered presentation. You render to an in-memory grid; retroglyph presents it via a pluggable
backend.

### What retroglyph is not

A game engine, a widget toolkit, or a terminal emulator. It sits below those.

---

### 1. Crate structure: single crate, split later

Start as a single `retroglyph` crate. Split into workspace crates (`retroglyph-core`, `retroglyph-crossterm`, etc.) when the
API stabilizes and compile times or dependency isolation warrant it.

Backend selection is via feature flags:

```toml
[features]
default = []
crossterm = ["dep:crossterm"]
# future: wgpu, web, sdl, etc

```

**Rationale:** Premature workspace splits create friction (publishing order, version coordination,
cross-crate refactoring). A monolith is easier to iterate on while the API is forming. The internal
module structure should anticipate the split (e.g., `src/backend/`, `src/core/`) so it's mechanical
when the time comes.

### 2. API: three-layer design

The public API has three tiers, all operating on the same underlying `Grid`:

### Layer 1 — Direct buffer access (lowest level)

```rust
let grid = term.grid_mut();
grid.put(5, 3, Cell::new('@', Style::default().fg(Color::Rgb { r: 255, g: 0, b: 0 })));
```

### Layer 2 — Stateful convenience (BearLibTerminal-style)

```rust
term.fg(Color::Rgb { r: 255, g: 0, b: 0 });
term.put(5, 3, '@');
term.print(0, 0, "Hello");
```

Layer 2 is sugar over Layer 1. `term.put(x, y, ch)` writes the character with the current
foreground/background/layer state into the grid buffer.

### Layer 3 — High-level helpers (future, not in v0.1)

```rust
term.print_styled(0, 0, Line::from(vec![
    Span::styled("HP: ", Style::default()),
    Span::styled("100", Style::new().fg(Color::GREEN)),
]));
```

**Key invariant:** The grid buffer is always the source of truth. Backends read from the grid. Layer
2 and 3 are convenience over Layer 1. All three can be mixed freely.

### 3. Cell model: simple, extend later

```rust
pub struct Cell {
    pub(crate) glyph: char,
    pub(crate) style: Style,
    // (internal fields for wide chars and EGCs omitted for brevity)
}
```

`CellModifier` is a manual bitflag newtype over `u8` (no `bitflags` dependency). `KeyModifiers` (for
input events) uses the same pattern. The distinct names avoid collision.

No layers, no tile stacking, no sub-cell offsets in v0.1. These are future additions. The `Grid` is
a flat `Vec<Cell>` with `width * height` entries.

**Rationale:** Start with the simplest model that's useful. Layers and composition add significant
complexity (multi-layer z-ordering, per-tile offsets, alpha blending). The simple cell model covers
the vast majority of roguelike rendering needs. The internal `Grid` type can be extended to hold
`Vec<Layer>` without changing the backend trait.

**Dependencies:** Zero runtime dependencies. `CellModifier` and `KeyModifiers` are manual bitflag
newtypes over `u8`.

### 4. Color model: enum with Default, Ansi, Indexed, Rgb

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Color {
    /// Backend's default foreground/background.
    Default,
    /// One of the 16 standard ANSI colors (theme-aware on terminals).
    Ansi(AnsiColor),
    /// 256-color palette index.
    Indexed(u8),
    /// 24-bit RGB.
    Rgb { r: u8, g: u8, b: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum AnsiColor {
    Black = 0, Red = 1, Green = 2, Yellow = 3,
    Blue = 4, Magenta = 5, Cyan = 6, White = 7,
    BrightBlack = 8, BrightRed = 9, BrightGreen = 10, BrightYellow = 11,
    BrightBlue = 12, BrightMagenta = 13, BrightCyan = 14, BrightWhite = 15,
}
```

**Rationale:** `Default` is essential because there is no RGB value that means "the terminal's
configured default." `Ansi` gives terminal users theme-respecting colors. `Indexed` covers the
256-color palette. `Rgb` covers everything else. GPU backends resolve `Default` and `Ansi` to
configured RGB values. Terminal backends emit the appropriate escape sequences directly.

### 5. Text formatting: plain text now, typed spans later

v0.1 supports `print(x, y, &str)` with plain text only (uses the current fg/bg state).

Future versions will add typed styled text (`Span`, `Line`, `Text` types similar to ratatui). No
inline formatting tags (`[color=red]...`) -- styled text is constructed programmatically.

**Rationale:** Type-safe span construction is more Rust-idiomatic than string-embedded tags. It's
composable, testable, and has zero parsing overhead. A convenience parser
(`parse_tags("[color=red]...") -> Vec<Span>`) can be added later as a helper function, but it's not
part of the core API.

### 6. Input: user-owned loop with poll/read primitives

The library does not own the event loop. It provides:

```rust
impl Terminal {
    /// Non-blocking: returns the next event if one is available within `timeout`.
    pub fn poll(&mut self, timeout: Duration) -> Option<Event>;

    /// Blocking: waits until an event is available.
    pub fn read(&mut self) -> Event;

    /// Non-blocking: checks if an event is available without consuming it.
    pub fn has_input(&self) -> bool;
}
```

The user writes their own loop:

```rust
loop {
    // Render
    term.clear();
    draw_world(&mut term);
    term.present();

    // Input (blocking for turn-based, poll for real-time)
    let event = term.read();
    match event {
        Event::Key(k) => handle_key(k),
        Event::Close => break,
        _ => {}
    }
}
```

**Rationale:** A user-owned loop supports turn-based (blocking read), real-time (poll with timeout),
async (future event stream), and hybrid patterns without the library prescribing a model.
BearLibTerminal used this approach. bracket-lib's library-owned loop forces turn-based games into
awkward state machines.

### 7. Double-buffered presentation with diff

`Terminal` maintains two grids: current (written to by the user) and previous (what was last
presented). `present()` computes the diff and sends only changed cells to the backend.

```rust
impl Terminal {
    pub fn present(&mut self) {
        let diff = self.current.diff(&self.previous);
        self.backend.draw(diff);
        self.backend.flush();
        std::mem::swap(&mut self.current, &mut self.previous);
    }
}
```

The backend receives an iterator of `(x, y, &Cell)` for changed cells only.

### 8. Backend trait

```rust
pub trait Backend {
    /// Draw changed cells to the output surface.
    fn draw<'a, I>(&mut self, content: I)
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>;

    /// Flush buffered output to the display.
    fn flush(&mut self);

    /// Report the current grid dimensions.
    fn size(&self) -> Size;

    /// Clear the entire display.
    fn clear(&mut self);

    /// Poll for an input event, waiting up to `timeout`.
    fn poll_event(&mut self, timeout: std::time::Duration) -> Option<Event>;

    /// Show or hide the cursor.
    fn set_cursor_visible(&mut self, visible: bool);

    /// Move the cursor to a position.
    fn set_cursor_position(&mut self, position: Position);
}
```

v0.1 ships with `HeadlessBackend` (in-memory buffer, no visual output). v0.2 adds `CrosstermBackend`
(ANSI terminal).

### 9. Headless backend with Display debug output

The headless backend stores the presented grid in memory. It is always available (no feature flag).
It is the primary test harness.

`Grid` implements `Display` for plain ASCII debug output:

```rust
let grid = term.grid();
println!("{grid}");
// Output:
// @·········
// ··········
// ··HP: 100·
```

No ANSI color codes in the Display output. Colored debug output is deferred to the crossterm
backend's non-TTY mode.

### 10. v0.1 scope

| In scope                                               | Out of scope (future)            |
| ------------------------------------------------------ | -------------------------------- |
| `Grid`, `Cell`, `Color`, `Style`, `CellModifier` types | Layers (multi-plane z-ordering)  |
| `Terminal` with stateful convenience API               | Tile composition (stacking)      |
| Double-buffered `present()` with diff                  | Sub-cell pixel offsets           |
| `Event` enum (key, mouse, resize, close)               | Styled text spans                |
| `HeadlessBackend`                                      | Inline formatting tags           |
| `Grid` Display impl (ASCII debug output)               | Font/tileset loading             |
| Backend trait                                          | Word wrapping / text alignment   |
|                                                        | Named colors (`color_from_name`) |
|                                                        | Any visual backend               |

### 11. Second backend: crossterm (v0.2)

After v0.1 stabilizes the core types and API, v0.2 adds a crossterm-based ANSI terminal backend
behind a `crossterm` feature flag. This validates the backend trait against a real rendering target
and produces a usable, playable terminal for roguelike development.

---

## Module structure (anticipated)

````rust
src/
├── lib.rs              # Public API re-exports
├── terminal.rs         # Terminal struct (stateful API, double buffering)
├── grid.rs             # Grid struct (2D cell buffer, diff)
├── cell.rs             # Cell type
├── color.rs            # Color, AnsiColor
├── style.rs            # Style, CellModifier
├── event.rs            # Event, KeyEvent, MouseEvent types
├── backend/
│   ├── mod.rs          # Backend trait
│   └── headless.rs     # HeadlessBackend
└── (future)
    ├── backend/
    │   ├── crossterm.rs
    │   ├── wgpu.rs
    │   └── web.rs
    ├── text.rs          # Span, Line, Text types
    └── layout.rs        # Word wrapping, alignment
```rust

## Consequences

- Games built against v0.1 use only the headless backend; no visual output until v0.2.
- The simple cell model means no tile stacking or layers initially. Games needing these will wait or

  work around by manual compositing.

- The user-owned loop means no built-in frame timing or vsync. Users manage their own sleep/poll

  timing.

- Starting as a monolith means all code is in one crate. Compile times will grow but are manageable

  at this scale.

- The full `Color` enum (with Ansi/Indexed) adds a few match arms to each backend but gives terminal

  users theme-aware colors from day one.

- Bare `(x, y)` coordinates for cell operations; `Position` struct for structured data (cursor,

  mouse events).

- `CellModifier` (not `Modifier`) avoids naming collision with `KeyModifiers`.
- Zero runtime dependencies. Manual bitflag newtypes instead of `bitflags` crate.

## References

- `docs/references/core/bearlibterminal-api.md` — full BearLibTerminal API reference
- `docs/references/core/crate-architecture.md` — workspace patterns from ratatui, wgpu, bevy
- `docs/references/core/game-loop-patterns.md` — loop pattern analysis and recommendation
- `docs/references/core/input-systems.md` — unified event model design
- `docs/references/core/error-handling.md` — error type design
- `docs/references/backends/headless.md` — headless backend patterns
- `docs/references/backends/ansi-terminal.md` — crossterm backend research
````
