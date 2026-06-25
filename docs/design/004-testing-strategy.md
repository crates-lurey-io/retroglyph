# ADR 004: E2E and Screenshot Testing Strategy

**Status:** Accepted **Date:** 2026-06-17 **Parent:** [ADR 001: Architecture](001-architecture.md)

## Context

To ensure the `retroglyph` library remains robust and its visual output does not regress, we need a
rigorous testing strategy. While traditional end-to-end (E2E) UI testing relies on pixel-based
screenshots (PNGs via Xvfb or Docker), this is highly discouraged for terminal applications due to
flakiness, system dependencies (fonts, GPU rendering), and unreviewable binary diffs.

This ADR defines the standard testing approach for `retroglyph` using Snapshot Testing via `insta`.
We will utilize a dual strategy: a `TestBackend` for fast logic validation, and a PTY-based SVG
generator for true visual regression testing.

---

## Strategy Overview

We will use a two-pronged approach for visual and functional testing:

### 1. Component & Game Logic Testing (`TestBackend`)

For testing UI components, state management, and general game logic, we will build a `TestBackend`.

- **Mechanism:** `TestBackend` implements the `Backend` trait, but instead of emitting ANSI escape

  sequences, it writes characters and styles into a 2D memory array (e.g., `Vec<Cell>`).

- **Assertion:** Tests will use `insta::assert_snapshot!` to record a text-based representation of

  this grid.

- **Benefits:** Extremely fast (runs in milliseconds), deterministic, and perfect for unit testing.

### 2. End-to-End Visual Testing (PTY + SVG Screenshots)

For testing the actual terminal integrations (e.g., the `CrosstermBackend`) and fully interactive
examples (like `crossterm_demo`), we need to verify that the raw ANSI escape sequences manipulate
the terminal correctly.

- **Mechanism:** We will use a pseudo-terminal (PTY) crate (e.g., `term_transcript` or

  `portable-pty`) to spawn the compiled binary. A PTY tricks the process into thinking it is running
  in a real terminal, ensuring raw mode and color sequences are correctly emitted.

- **Assertion:** We will capture the output and generate an SVG file that accurately visualizes the

  terminal state. We will then snapshot this SVG using `insta`.

- **Benefits:** Acts exactly like a visual "screenshot test" that can be viewed in GitHub pull

  requests. Because SVG is an XML text format, it produces readable `git diff`s and handles version
  control gracefully.

---

## Dependency Graph

````text
[From M15: Interactive End-to-End Game]
 ├─► M16: TestBackend Implementation
 └─► M17: E2E SVG Snapshot Harness
```rust

---

## M16: TestBackend Implementation

**Goal:** Create an in-memory backend for unit testing component logic.

### Instructions

1. **File Creation:**
   - Create `src/backend/test.rs` and expose it.
1. **Struct Definition:**
   - Define a `TestBackend` struct that holds a width, a height, and a linear buffer of `retroglyph::Cell`

     elements to represent the screen.

1. **Trait Implementation:**
   - Implement `Backend` for `TestBackend`. `draw()` should update the internal cell buffer instead

     of writing to stdout. `flush()` can be a no-op or used to sync double-buffering if implemented.

1. **String Conversion:**
   - Add a `format_view(&self) -> String` method to `TestBackend` that converts the buffer into a

     readable string (e.g., stripping styles or printing them as inline markdown/tags for
     debugging).

1. **Validation:**
   - Add `insta = "1.0"` to `[dev-dependencies]`.
   - Write a small unit test `test_backend_rendering` that draws to the `TestBackend` and uses

     `insta::assert_snapshot!` to verify the output.

### Acceptance Criteria

- [ ] `TestBackend` implements `Backend` successfully.
- [ ] Drawing text to `TestBackend` properly updates the in-memory buffer.
- [ ] A unit test proves that `TestBackend` states can be cleanly snapshotted by `insta`.

---

## M17: E2E SVG Snapshot Harness

**Goal:** Build a test harness for SVG screenshot testing of interactive binaries.

### Instructions (2)

1. **Dependencies:**
   - Add `term_transcript` (or your chosen PTY/SVG crate) to `[dev-dependencies]`.
1. **Test Setup:**
   - Create a new integration test file: `tests/e2e_snapshots.rs`.
1. **Harness Execution:**
   - Write a test that executes the `crossterm_demo` binary (built in M15) inside the PTY harness.
   - Send a sequence of bytes to simulate user input (e.g., moving the player character).
1. **Snapshot Capture:**
   - Capture the terminal output as an SVG transcript.
   - Use `insta::assert_snapshot!` to save and assert against the SVG text.

### Acceptance Criteria (2)

- [ ] The `crossterm_demo` can be launched in a headless PTY environment via `cargo test`.
- [ ] An SVG file is successfully generated reflecting the final state of the terminal.
- [ ] `insta` captures the SVG, establishing a baseline snapshot for future visual regression

      testing.
````
