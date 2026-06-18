# ADR 002: Foundations Implementation Plan

**Status:** Draft **Date:** 2026-06-15 **Parent:** [ADR 001: Architecture](001-architecture.md)

## Context

This document breaks the foundational work of the library into fine-grained, independently reviewable milestones with
acceptance criteria detailed enough for human or agent implementation. Each milestone is reviewed
before the next starts. No release will be made at this stage; the library will remain unreleased until a real backend is implemented and end-to-end games can be built.

## Decisions made during planning

These refine and extend ADR 001:

| Decision                         | Choice                                                             | Rationale                                                |
| -------------------------------- | ------------------------------------------------------------------ | -------------------------------------------------------- |
| Dependencies                     | Zero runtime deps (manual bitflags)                                | User preference for minimal deps                         |
| `read()` on empty headless queue | Panic with clear message                                           | It's a programmer bug; fail fast                         |
| Zero-size grids                  | Allowed                                                            | Consistent with silent no-op design                      |
| M8 Terminal scope                | Keep as one milestone                                              | Methods are individually trivial; review together        |
| Backend input                    | `poll_event` on `Backend` trait                                    | Simple; one trait, one impl per backend                  |
| Terminal generics                | `Terminal<B: Backend>`                                             | Zero-cost; users opt into `Box<dyn Backend>` if needed   |
| `print()` newlines               | `\n` advances to next row                                          | Convenient; BearLibTerminal does this                    |
| Coordinate convention            | Bare `(x, y)` for operations; `Position` struct for data           | Ergonomic hot path; less ceremony than tuples or structs |
| Text style flags naming          | `CellModifier` (not `Modifier`)                                    | Avoids collision with `KeyModifiers`                     |
| Input modifier naming            | `KeyModifiers`                                                     | Standard name                                            |
| MSRV                             | Edition 2024, Rust 1.85+                                           | New project, no legacy users                             |
| Test assertions                  | On `Grid` (not `HeadlessBackend`)                                  | Most reusable location                                   |
| Lints                            | `#![forbid(unsafe_code)]`, deny clippy::all, warn pedantic+nursery | Strict; keeps agents on rails                            |

---

## Dependency graph

```
M0: Skeleton
 └─► M1: Color, CellModifier, Style
      └─► M2: Cell
           ├─► M3: Grid
           │    ├─► M4: Grid diff + Display
           │    │    └──────────────────┐
           │    └─► M6: Backend trait   │
           │         └─► M7: Headless   │
           │              └─────────────┤
           └─────────────────────────── │
M5: Event types ────────────────────────┤
                                        ▼
                                   M8: Terminal
                                        └─► M9: Example + E2E
                                             └─► M10: Polish
```

M5 (events) is independent of M1-M4 and can be built in parallel.

---

## M0: Project skeleton

**Goal:** Empty project that compiles, lints, and passes CI. No library code yet.

### Files

```
rg/
├── .editorconfig
├── .gitignore
├── .markdownlint-cli2.jsonc
├── .github/workflows/ci.yml
├── Cargo.toml
├── Justfile
├── CONTRIBUTING.md
├── LICENSE-MIT
├── LICENSE-APACHE
├── README.md
├── clippy.toml
├── rustfmt.toml
├── docs/design/
│   ├── 001-architecture.md
│   └── 002-v0.1.0-plan.md
└── src/lib.rs
```

### Cargo.toml

```toml
[package]
name = "rg"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
license = "MIT OR Apache-2.0"
description = "A terminal/grid rendering library for roguelikes"
keywords = ["roguelike", "terminal", "grid", "gamedev"]
categories = ["game-development", "graphics"]

[lints.rust]
unsafe_code = "forbid"
missing_docs = "warn"
unreachable_pub = "warn"
unused_qualifications = "warn"

[lints.clippy]
all = { level = "deny", priority = -1 }
pedantic = { level = "warn", priority = -1 }
nursery = { level = "warn", priority = -1 }
must_use_candidate = "allow"
module_name_repetitions = "allow"
missing_errors_doc = "allow"
missing_panics_doc = "allow"
```

### src/lib.rs

```rust
//! rg: a terminal/grid rendering library for roguelikes.
//!
//! rg provides a grid of character cells with styled output, input handling,
//! and double-buffered presentation via pluggable backends.
```

### CI (`.github/workflows/ci.yml`)

Jobs:

1. **fmt** — `cargo fmt --all --check` (nightly formatter)
2. **clippy** — `cargo clippy --all-targets -- -D warnings` (stable)
3. **test** — `cargo test --all-features` on {ubuntu, macos, windows} x {MSRV, stable}
4. **doc** — `cargo doc --no-deps` (doc build + doc-tests)
5. **msrv** — `cargo check` on 1.85
6. **markdown** — markdownlint-cli2 on `**/*.md`

### Justfile

```just
check: fmt-check lint test doc
fmt:
    cargo fmt --all
fmt-check:
    cargo fmt --all -- --check
lint:
    cargo clippy --all-targets -- -D warnings
test:
    cargo test --all-features
test-v:
    cargo test --all-features -- --nocapture
doc:
    cargo doc --no-deps --document-private-items
clean:
    cargo clean
```

### Acceptance criteria

- [ ] `cargo check` passes
- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo test` passes (no tests yet, but harness runs)
- [ ] `cargo doc --no-deps` passes
- [ ] CI workflow runs green on push
- [ ] `#![forbid(unsafe_code)]` is set in `lib.rs`
- [ ] README has project description and "under construction" notice
- [ ] CONTRIBUTING.md has prerequisites, build, test, and lint instructions
- [ ] Dual license files present
- [ ] `.editorconfig`, `rustfmt.toml`, `clippy.toml` present

---

## M1: Color, CellModifier, Style

**Goal:** Core styling types with full test coverage. Zero runtime dependencies.

### Files

- `src/color.rs` — `Color`, `AnsiColor`
- `src/style.rs` — `Style`, `CellModifier`
- Update `src/lib.rs` — add modules, re-exports

### Types

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Color {
    #[default]
    Default,
    Ansi(AnsiColor),
    Indexed(u8),
    Rgb { r: u8, g: u8, b: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum AnsiColor {
    Black = 0, Red, Green, Yellow, Blue, Magenta, Cyan, White,
    BrightBlack, BrightRed, BrightGreen, BrightYellow,
    BrightBlue, BrightMagenta, BrightCyan, BrightWhite,
}
```

`Color` convenience constants: `Color::RED`, `Color::GREEN`, etc. as `const` associated values
mapping to `Color::Ansi(AnsiColor::Red)`, etc.

````rust
/// Text attributes applied to a cell (bold, italic, etc.).
///
/// Implemented as a manual bitflag over `u8`. Combine with `|`:
/// ```
/// let attrs = CellModifier::BOLD | CellModifier::ITALIC;
/// assert!(attrs.contains(CellModifier::BOLD));
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CellModifier(u8);

impl CellModifier {
    pub const NONE:          Self = Self(0);
    pub const BOLD:          Self = Self(1 << 0);
    pub const DIM:           Self = Self(1 << 1);
    pub const ITALIC:        Self = Self(1 << 2);
    pub const UNDERLINE:     Self = Self(1 << 3);
    pub const BLINK:         Self = Self(1 << 4);
    pub const REVERSE:       Self = Self(1 << 5);
    pub const HIDDEN:        Self = Self(1 << 6);
    pub const STRIKETHROUGH: Self = Self(1 << 7);
}
````

Implement: `BitOr`, `BitOrAssign`, `BitAnd`, `BitAndAssign`, `Not`, `contains()`, `is_empty()`,
`Debug` (prints flag names like `BOLD | ITALIC`).

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Style {
    pub fg: Color,
    pub bg: Color,
    pub modifiers: CellModifier,
}
```

Builder methods on `Style`: `Style::new()`, `.fg(Color)`, `.bg(Color)`, `.bold()`, `.italic()`,
`.underline()`, etc. Each returns `Self` for chaining.

`Style::patch(other: Style)` — overlay: non-default fields from `other` override `self`.

### Tests

- `Color::default() == Color::Default`
- `AnsiColor::Red as u8 == 1`
- `CellModifier` bitflag operations: combine, check, negate, empty
- `CellModifier` Debug shows flag names
- `Style` builder chaining
- `Style::patch` overlay behavior
- `Color` constants: `Color::RED == Color::Ansi(AnsiColor::Red)`

### Acceptance criteria

- [ ] `Color`, `AnsiColor`, `CellModifier`, `Style` are public
- [ ] All types derive `Debug, Clone, Copy, PartialEq, Eq, Hash, Default`
- [ ] `CellModifier` supports `|`, `&`, `!`, `contains()`, `is_empty()` without `bitflags`
- [ ] `CellModifier` Debug prints flag names (e.g., `BOLD | ITALIC`)
- [ ] `Style` builder chaining works
- [ ] `Style::patch` merges correctly
- [ ] Every public item has a doc comment
- [ ] All tests pass, CI green

---

## M2: Cell

**Goal:** The `Cell` type representing one character position in the grid.

### Files

- `src/cell.rs`
- Update `src/lib.rs`

### Type

```rust
/// A single cell in the terminal grid.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Cell { /* private fields */ }
```

Private fields: `ch: char`, `style: Style`.

Methods:

- `Cell::new(ch: char) -> Self` — default style
- `Cell::styled(ch: char, style: Style) -> Self`
- `Cell::default()` — space, default style
- Getters: `ch()`, `style()`, `fg()`, `bg()`, `modifiers()`
- Setters: `set_char()`, `set_style()`, `set_fg()`, `set_bg()`, `add_modifier()`,
  `remove_modifier()`
- `Cell::reset()` — reset to default (space, default style)

### Tests

- `Cell::default()` is space with default style
- `Cell::new('A')` round-trips character
- Setters mutate correctly
- `Cell::reset()` returns to default
- Document `size_of::<Cell>()` in a test (not a hard cap, just visible)

### Acceptance criteria

- [ ] `Cell` has private fields, public accessor API
- [ ] Derives `Debug, Clone, PartialEq, Eq, Hash`, implements `Default`
- [ ] All accessors and mutators tested
- [ ] Doc comments on all public items
- [ ] CI green

---

## M3: Grid

**Goal:** 2D cell buffer with bounds-checked access and bulk operations.

### Files

- `src/grid.rs` — `Grid`, `Position`, `Size`, `Rect`
- Update `src/lib.rs`

### Types

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Position { pub x: u16, pub y: u16 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Size { pub width: u16, pub height: u16 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rect { pub x: u16, pub y: u16, pub width: u16, pub height: u16 }
```

```rust
/// A 2D grid of cells, stored row-major.
/// Coordinates are (x, y) where x = column (0 = left), y = row (0 = top).
pub struct Grid { /* cells: Vec<Cell>, width: u16, height: u16 */ }
```

Methods:

- `Grid::new(width, height)` — all cells default. Zero-size allowed.
- `Grid::filled(width, height, cell: Cell)`
- `Grid::size() -> Size`, `width() -> u16`, `height() -> u16`
- `Grid::cell(x, y) -> Option<&Cell>` — bounds-checked
- `Grid::cell_mut(x, y) -> Option<&mut Cell>` — bounds-checked
- `Grid::clear()` — all cells to default
- `Grid::clear_region(rect: Rect)`
- `Grid::fill(cell: Cell)` — all cells to `cell`
- `Grid::fill_region(rect: Rect, cell: Cell)`
- `Grid::resize(width, height)` — preserves content in overlap
- `Grid::cells() -> &[Cell]` — flat slice
- `Grid::iter() -> impl Iterator<Item = (u16, u16, &Cell)>` — with positions
- `Index<(u16, u16)>` — panics on OOB (like Vec)

Test assertions (on Grid, for reuse):

- `Grid::assert_cell(&self, x, y, expected_char)` — panics with clear message on mismatch
- `Grid::assert_cell_style(&self, x, y, expected_char, expected_style)`

### Tests

- `Grid::new(80, 24)` has 1920 cells, all default
- `Grid::new(0, 0)` produces empty grid, all ops are no-ops
- `cell(0, 0)` returns `Some`, `cell(80, 0)` returns `None`
- `cell_mut` + setter round-trips
- `clear()` resets all cells
- `fill_region` only affects the specified rect
- `resize` preserves content in overlap; new cells are default
- Index panics on OOB (`#[should_panic]`)
- Grid 1x1 works
- `assert_cell` passes on correct content, panics on mismatch

### Acceptance criteria

- [ ] `Grid`, `Position`, `Size`, `Rect` are public
- [ ] Bounds-checked access via `cell()` / `cell_mut()`
- [ ] Zero-size grids work
- [ ] All bulk operations tested
- [ ] Test assert helpers on `Grid`
- [ ] Doc comments on all public items
- [ ] CI green

---

## M4: Grid diff + Display

**Goal:** Compute changed cells between two grids. Display impl for debug output.

### Files

- Add to `src/grid.rs`

### Diff

```rust
impl Grid {
    /// Yield positions where `self` differs from `other`.
    /// If dimensions differ, all cells in `self` are considered changed.
    pub fn diff<'a>(&'a self, other: &'a Grid) -> impl Iterator<Item = (u16, u16, &'a Cell)>;
}
```

### Display

One line per row. Each cell's character printed. Default/space cells shown as `·` (middle dot) for
visibility.

```
@·········
··········
··HP: 100·
```

### Tests

- Identical grids: diff yields nothing
- One cell changed: yields exactly that cell
- All cells different: yields all
- Different dimensions: yields all from `self`
- Display matches expected string
- Display on empty grid (0x0): empty string
- Display with Unicode characters

### Acceptance criteria

- [ ] `Grid::diff()` returns correct changed cells
- [ ] `Display` produces readable ASCII output
- [ ] Edge cases tested
- [ ] CI green

---

## M5: Event types

**Goal:** Input event data types. Independent of M1-M4; can be built in parallel.

### Files

- `src/event.rs`
- Update `src/lib.rs`

### Types

```rust
pub enum Event {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
    Close,
}

pub struct KeyEvent {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

pub enum KeyCode {
    Char(char), F(u8), Backspace, Enter, Left, Right, Up, Down,
    Home, End, PageUp, PageDown, Tab, BackTab, Delete, Insert, Escape,
}

/// Keyboard modifier flags.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct KeyModifiers(u8);
impl KeyModifiers {
    pub const NONE:    Self = Self(0);
    pub const SHIFT:   Self = Self(1 << 0);
    pub const CONTROL: Self = Self(1 << 1);
    pub const ALT:     Self = Self(1 << 2);
}

pub struct MouseEvent {
    pub kind: MouseEventKind,
    pub position: Position,
    pub modifiers: KeyModifiers,
}

pub enum MouseEventKind { Down(MouseButton), Up(MouseButton), Moved, ScrollUp, ScrollDown }
pub enum MouseButton { Left, Right, Middle }
```

`KeyModifiers` is a manual bitflag (same pattern as `CellModifier`). Implement `BitOr`,
`BitOrAssign`, `contains()`, `is_empty()`, `Debug`.

Note: `MouseEvent` uses `Position` struct for its coordinates (data, not operation).

### Tests

- All variants constructible
- `KeyModifiers` bitflags operations
- All types are `Clone + PartialEq + Debug`

### Acceptance criteria

- [ ] All event types are public
- [ ] All derive `Debug, Clone, PartialEq, Eq, Hash`
- [ ] `KeyModifiers` supports bitflags ops without `bitflags` crate
- [ ] Doc comments on all public items
- [ ] CI green

---

## M6: Backend trait

**Goal:** The rendering backend interface.

### Files

- `src/backend/mod.rs`
- Update `src/lib.rs`

### Trait

```rust
/// A rendering backend that presents grid content to a display
/// and provides input events.
pub trait Backend {
    /// Draw changed cells to the output surface.
    fn draw<'a, I>(&mut self, content: I)
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>;

    /// Flush buffered output to the display.
    fn flush(&mut self);

    /// Return current display dimensions.
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

### Tests

- `Backend` is object-safe: `let _: Box<dyn Backend>;` compiles

### Acceptance criteria

- [ ] `Backend` trait is public and object-safe
- [ ] Doc comments on trait and every method
- [ ] CI green

---

## M7: Headless backend

**Goal:** In-memory backend for testing.

### Files

- `src/backend/headless.rs`
- Update `src/backend/mod.rs`

### HeadlessBackend

```rust
/// In-memory backend for testing. Stores presented content
/// and allows injecting synthetic events.
pub struct HeadlessBackend { /* grid, cursor state, event queue */ }
```

Methods:

- `HeadlessBackend::new(width, height) -> Self`
- `HeadlessBackend::grid() -> &Grid` — inspect presented content
- `HeadlessBackend::cursor_visible() -> bool`
- `HeadlessBackend::cursor_position() -> Position`
- `HeadlessBackend::push_event(event: Event)` — inject synthetic event
- `HeadlessBackend::push_events(events: impl IntoIterator<Item = Event>)` — inject multiple

Backend impl:

- `draw()` writes cells into internal grid
- `flush()` no-op
- `size()` returns grid dimensions
- `clear()` clears internal grid
- `poll_event()` pops from queue; returns `None` if empty (ignores timeout)
- `set_cursor_visible()` / `set_cursor_position()` store state

### Tests

- Create, draw cells, inspect via `grid()`
- `push_event` + `poll_event` round-trips
- `poll_event` on empty queue returns `None`
- `clear()` resets grid
- Multiple `draw()` calls: only changed cells overwrite

### Acceptance criteria

- [ ] `HeadlessBackend` implements `Backend`
- [ ] Event injection and retrieval work
- [ ] Grid inspection works
- [ ] Doc comments on all public items
- [ ] CI green

---

## M8: Terminal

**Goal:** Main `Terminal` struct: stateful API, double buffering, diff-based presentation, input
forwarding.

### Files

- `src/terminal.rs`
- Update `src/lib.rs`

### Terminal

```rust
/// The main entry point for rg.
///
/// Generic over the backend. Owns a double-buffered grid and provides
/// a stateful drawing API.
pub struct Terminal<B: Backend> { /* current, previous grids; backend; drawing state */ }
```

### Construction

```rust
impl<B: Backend> Terminal<B> {
    /// Create a terminal with the given backend.
    /// Grid dimensions are queried from the backend.
    pub fn new(backend: B) -> Self;
}
```

### Stateful drawing (Layer 2)

```rust
pub fn fg(&mut self, color: Color) -> &mut Self;
pub fn bg(&mut self, color: Color) -> &mut Self;
pub fn modifier(&mut self, modifier: CellModifier) -> &mut Self;
pub fn reset_style(&mut self) -> &mut Self;
pub fn style(&self) -> Style;

/// Place a character at (x, y) with the current style.
/// Out-of-bounds coordinates are silently ignored.
pub fn put(&mut self, x: u16, y: u16, ch: char);

/// Place a character with an explicit style, ignoring current state.
pub fn put_styled(&mut self, x: u16, y: u16, ch: char, style: Style);

/// Print a string starting at (x, y) with the current style.
/// Characters beyond grid width are clipped. `\n` advances to the
/// next row at the original x.
pub fn print(&mut self, x: u16, y: u16, text: &str);

/// Clear the entire grid.
pub fn clear(&mut self);

/// Clear a rectangular region.
pub fn clear_region(&mut self, rect: Rect);
```

### Direct buffer access (Layer 1)

```rust
pub fn grid(&self) -> &Grid;
pub fn grid_mut(&mut self) -> &mut Grid;
pub fn backend(&self) -> &B;
pub fn backend_mut(&mut self) -> &mut B;
```

### Presentation

```rust
/// Present the current frame. Computes diff, sends changed cells
/// to the backend, flushes, then swaps buffers.
pub fn present(&mut self);
```

### Input

```rust
/// Poll for an event with timeout. Returns `None` on timeout.
pub fn poll(&mut self, timeout: Duration) -> Option<Event>;

/// Block until an event is available.
/// Panics if the backend has no events (headless with empty queue).
pub fn read(&mut self) -> Event;

/// Check if an event is available.
pub fn has_input(&mut self) -> bool;
```

`read()` implementation: calls `poll(Duration::MAX)` and
`.expect("read() called but no events available")`.

### Tests

- Create `Terminal<HeadlessBackend>`, put a char, present, inspect backend grid
- Stateful fg/bg: put verifies cell has the set colors
- `put` out-of-bounds: silently ignored
- `print` writes consecutive characters
- `print` clips at grid boundary
- `print` handles `\n` (advances to next row at original x)
- `clear` resets all cells
- `present` sends only changed cells
- Second `present` with no changes: backend grid unchanged
- `reset_style` returns to defaults
- Style chaining: `term.fg(RED).bg(BLACK).modifier(BOLD)`
- Input: push event, `poll()` returns it
- Input: `has_input()` reflects queue state
- `read()` on empty queue panics with clear message
- `grid()` / `grid_mut()` provide direct buffer access
- `put_styled` overrides current drawing state

### Acceptance criteria

- [ ] `Terminal<B: Backend>` compiles with all methods
- [ ] Stateful drawing API works
- [ ] Direct buffer access works
- [ ] `present()` diffs and sends only changed cells
- [ ] `print()` handles `\n` correctly
- [ ] Input forwarding works
- [ ] `read()` panics on empty headless queue with clear message
- [ ] Out-of-bounds puts silently ignored
- [ ] All methods documented
- [ ] CI green

---

## M9: Example + E2E tests

**Goal:** Runnable example exercising the full API, doubling as an E2E test.

### Files

- `examples/headless_demo.rs`
- `tests/e2e.rs`

### Example

A tiny "roguelike" that:

1. Creates a `Terminal<HeadlessBackend>` (40x15)
2. Draws a room with box-drawing characters (`─`, `│`, `┌`, `┐`, `└`, `┘`)
3. Places player `@` at (5, 5) with green foreground
4. Places enemies `g`, `D` at various positions
5. Prints status line: `HP: 100  Level: 1`
6. Calls `present()`
7. Injects arrow-key event, moves player
8. Presents again
9. Prints both frames via `println!("{}", term.grid())`

### E2E test

Same logic, but asserts instead of printing:

- Grid dimensions correct
- Player at expected position after each frame
- Walls at correct positions
- Status line text correct
- After move: old position cleared, new position has `@`

### Acceptance criteria

- [ ] `cargo run --example headless_demo` runs and prints two frames
- [ ] `cargo test --test e2e` passes
- [ ] Example exercises: put, put_styled, print, clear, present, poll, fg, bg, modifier
- [ ] CI runs both
- [ ] CI green

---

## M10: Documentation + polish

**Goal:** Final polish of the foundational types.

### Tasks

1. **Doc review** — every public item has a doc comment. Module-level docs explain purpose. Key
   types have `# Examples` sections in their doc comments.

2. **README update** — replace "under construction":
   - What rg is (one paragraph)
   - Usage example (headless)
   - Status (Unreleased foundation, crossterm backend coming next)
   - API overview
   - License

3. **Lint audit** — final `cargo clippy`, fix all warnings.

4. **Dependency audit** — confirm zero runtime dependencies.

6. **Size audit** — test that prints `size_of::<Cell>()`, `size_of::<Style>()`,
   `size_of::<Color>()`. Document values.

7. **MSRV verification** — `cargo +1.85 check`.

8. **Tag** — create v0.1.0 tag.

### Acceptance criteria

- [ ] `cargo doc --no-deps` builds with zero warnings
- [ ] Every public item has a doc comment
- [ ] `cargo clippy -- -D warnings` clean
- [ ] `cargo test --all-features` green
- [ ] README is current
- [ ] CHANGELOG.md exists
- [ ] MSRV check passes
- [ ] No `unsafe` anywhere
- [ ] Zero runtime dependencies
- [ ] Tagged v0.1.0

---

## Summary

| Milestone | Description                                   | Est. complexity |
| --------- | --------------------------------------------- | --------------- |
| M0        | Project skeleton                              | Low             |
| M1        | Color, CellModifier, Style                    | Low             |
| M2        | Cell                                          | Low             |
| M3        | Grid + Position/Size/Rect                     | Medium          |
| M4        | Grid diff + Display                           | Medium          |
| M5        | Event types (parallel with M1-M4)             | Low             |
| M6        | Backend trait                                 | Low             |
| M7        | Headless backend                              | Low-Medium      |
| M8        | Terminal (stateful API + double buffer + I/O) | Medium-High     |
| M9        | Example + E2E tests                           | Medium          |
| M10       | Polish + tag v0.1.0                           | Low             |

**Critical path:** M0 → M1 → M2 → M3 → M4 → M6 → M7 → M8 → M9 → M10

**Parallel opportunity:** M5 can run alongside M1-M4.

## Consequences

- Zero runtime dependencies means ~80 lines of manual bitflag code instead of `bitflags` macro
  invocations.
- `CellModifier` naming is non-standard (ratatui uses `Modifier`, crossterm uses `Attribute`) but
  unambiguous in context.
- `read()` panicking on empty headless queue means tests must always inject events before calling
  `read()`. This catches bugs early but requires slightly more test setup.
- Bare `(x, y)` coordinates mean parameter names are the only signal for coordinate order. Doc
  comments must be clear about x=column, y=row.
- `print()` interpreting `\n` adds a small amount of complexity but significantly improves usability
  for multi-line text.
