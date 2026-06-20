# ADR 003: Crossterm Backend Implementation Plan

**Status:**Draft**Date:**2026-06-17**Parent:** [ADR 001: Architecture](001-architecture.md)

## Context

To build and play real games, the `rg` library needs a visual backend capable of drawing to the
terminal and reading real user input. As decided in ADR 001, we will implement this using the
`crossterm` crate.

This document breaks the Crossterm Backend into fine-grained, independently reviewable milestones,
similar to the foundations plan. Each milestone comes with explicit instructions and acceptance
criteria so that an implementing agent can follow it mechanically.

---

## Dependency graph

````yaml
M11: Crossterm Skeleton
 └─► M12: Terminal Setup, Teardown & Panic Hook
      └─► M13: Rendering & Color Mapping
           └─► M14: Input Handling
                └─► M15: Interactive End-to-End Game
```rust

---

## M11: Crossterm Skeleton

**Goal:** Add the `crossterm` dependency behind a feature flag and stub out the `Backend` trait
implementation.

### Instructions

1. **Cargo.toml Update:**
   - Add `crossterm = { version = "0.27", optional = true }` under `[dependencies]`.
   - Add a `[features]` section if one doesn't exist.
   - Define `default = []`.
   - Define `crossterm = ["dep:crossterm"]`.

1. **File Creation:**
   - Create `src/backend/crossterm.rs`.
   - Update `src/backend/mod.rs` to expose the new module (conditionally compiled with

     `#[cfg(feature = "crossterm")]`).

   - Create the `CrosstermBackend` struct. It should own a handle to the standard output, wrapped in

     a `std::io::BufWriter` (e.g., `BufWriter<std::io::Stdout>`).

1. **Trait Stubs:**
   - Implement the `Backend` trait for `CrosstermBackend` where every method just returns

     `unimplemented!()` or does nothing.

### Acceptance Criteria

- [ ] `cargo check --features crossterm` succeeds without errors.
- [ ] `CrosstermBackend` is publicly accessible when the feature is enabled.

---

## M12: Terminal Setup, Teardown & Panic Hook

**Goal:** Ensure the terminal enters raw mode safely and—most importantly—exits it safely even if
the game crashes.

### Instructions (2)

1. **Initialization (`CrosstermBackend::new`):**
   - Call `crossterm::terminal::enable_raw_mode()`.
   - Execute the following crossterm commands onto stdout (using `crossterm::execute!`):
     - `crossterm::terminal::EnterAlternateScreen`
     - `crossterm::cursor::Hide`
     - `crossterm::event::EnableMouseCapture`
   - Set up a custom panic hook using `std::panic::set_hook`. Inside the hook:
     - Catch the panic.
     - Call a `restore_terminal()` helper function (which disables raw mode, leaves the alternate

       screen, shows the cursor, and disables mouse capture).

     - Print the actual panic info so the developer can read the error stack trace on the normal

       terminal.

1. **Teardown (`Drop` trait):**
   - Implement `Drop` for `CrosstermBackend`.
   - In `drop`, call the exact same `restore_terminal()` helper function to ensure symmetrical

     cleanup on normal exits.

### Acceptance Criteria (2)

- [ ] Alternate screen, raw mode, and mouse capture are enabled on creation.
- [ ] A program creating the backend and immediately exiting leaves the terminal completely normal

      (no broken newlines or hidden cursors).

- [ ] A program that panics restores the terminal to normal _before_ dumping the panic text.

---

## M13: Rendering & Color Mapping

**Goal:** Implement the logic that draws characters, applies colors, and pushes updates to the
screen without flickering.

### Instructions (3)

1. **Color & Modifier Mapping:**
   - Create private helper functions mapping our types to crossterm types:
     - `map_color(c: rg::Color) -> crossterm::style::Color`
       - `Default` -> `Reset`
       - `Ansi(c)` -> map standard ANSI values (`Black`, `Red`, etc.)
       - `Indexed(i)` -> `AnsiValue(i)`
       - `Rgb { r, g, b }` -> `Rgb { r, g, b }`
     - `map_modifier(m: rg::style::CellModifier) -> crossterm::style::Attributes`

1. **`Backend::draw` Implementation:**
   - The method takes an iterator of `(x, y, &Cell)`.
   - Loop over the iterator. For each cell, use `crossterm::queue!` onto the internal `BufWriter`:
     - Queue `crossterm::cursor::MoveTo(x, y)`.
     - Queue `crossterm::style::SetForegroundColor(map_color(cell.style.fg))`.
     - Queue `crossterm::style::SetBackgroundColor(map_color(cell.style.bg))`.
     - Queue `crossterm::style::SetAttributes(map_modifier(cell.style.modifiers))`.
     - Queue `crossterm::style::Print(cell.glyph)`.
   - **Optimization:** Track the _last queued_ fg, bg, and attributes locally within the loop, and

     only queue the `Set*` commands if they actually changed from the previous cell.

1. **`Backend::flush` Implementation:**
   - Queue `crossterm::terminal::BeginSynchronizedUpdate` (DECSET 2026).
   - Flush the `BufWriter`.
   - Queue `crossterm::terminal::EndSynchronizedUpdate`.
   - Flush the `BufWriter` again.

1. **Remaining Trait Methods:**
   - Implement `clear()`, `size()`, `set_cursor_visible()`, and `set_cursor_position()` using the

     equivalent `crossterm` commands.

### Acceptance Criteria (3)

- [ ] `Backend::draw` and `Backend::flush` are fully implemented without using `unimplemented!()`.
- [ ] The draw loop caches color/attribute state to avoid redundant escape sequences.
- [ ] Flush correctly utilizes synchronized updates to prevent tearing.

---

## M14: Input Handling

**Goal:** Read crossterm events and translate them into our unified `rg::Event` system.

### Instructions (4)

1. **`Backend::poll_event` Implementation:**
   - Call `crossterm::event::poll(timeout)`.
   - If it returns `true`, call `crossterm::event::read()`.
   - Match the returned `crossterm::event::Event`:
     - `Event::Key(k)`: Map crossterm's `KeyCode` and `KeyModifiers` exactly to `rg::KeyCode` and

       `rg::KeyModifiers`. Return `rg::Event::Key(KeyEvent { ... })`.

     - `Event::Mouse(m)`: Map crossterm's `MouseButton`, `MouseEventKind`, and cell coordinates to

       `rg::MouseEvent`. Return `rg::Event::Mouse`.

     - `Event::Resize(w, h)`: Return `rg::Event::Resize(w, h)`.
     - Ignore `FocusGained`, `FocusLost`, and `Paste` for now (return `None` or loop to read the

       next event until the timeout expires).

### Acceptance Criteria (4)

- [ ] `poll_event` returns `Some(rg::Event)` correctly populated.
- [ ] `poll_event` respects the given timeout.
- [ ] Key modifiers (Shift, Ctrl, Alt) are correctly passed through.

---

## M15: Interactive End-to-End Game

**Goal:** Create a playable example to prove the backend works flawlessly.

### Instructions (5)

1. **Example Setup:**
   - Create `examples/crossterm_demo.rs`.
   - Instantiate a `CrosstermBackend` and wrap it in `Terminal::new(backend)`.

1. **Game Loop:**
   - Implement a standard `loop { ... }`.
   - Inside the loop, clear the screen, then draw:
     - A boundary box for a room using box-drawing characters (`┌`, `─`, `┐`, etc.).
     - An `@` player character at a tracked `(player_x, player_y)` coordinate.
     - An enemy character (e.g., a red `D`) standing in the room.
     - A status line (e.g., `HP: 100`).
   - Call `term.present()`.
   - Call `term.read()` (which blocks for input).
   - Match the input:
     - On Arrow Keys: update `(player_x, player_y)` (ensuring they don't walk through walls).
     - On `q` or `Esc`: `break` the loop to exit the game.

### Acceptance Criteria (5)

- [ ] Running `cargo run --example crossterm_demo --features crossterm` boots directly into a visual

      game.

- [ ] The player can move the `@` around seamlessly.
- [ ] There is no visual flickering (proving double buffering and synchronized output are working).
- [ ] Exiting the game restores the terminal perfectly.
````
