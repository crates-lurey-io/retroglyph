# retroglyph

[![CI](https://github.com/crates-lurey-io/retroglyph/actions/workflows/ci.yml/badge.svg)](https://github.com/crates-lurey-io/retroglyph/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/crates-lurey-io/retroglyph/graph/badge.svg?token=z8BBUp8fiY)](https://codecov.io/gh/crates-lurey-io/retroglyph)
[![docs](https://github.com/crates-lurey-io/retroglyph/actions/workflows/docs.yml/badge.svg)](https://main.retroglyph.dev/)
[![Benchmarks](https://img.shields.io/badge/benchmarks-bencher.dev-blue)](https://bencher.dev/perf/retroglyph)
[![license](https://img.shields.io/crates/l/retroglyph-core.svg)](LICENSE)

A 2D pseudographic terminal library for Rust.

`retroglyph` provides a styled character grid, double-buffered rendering, and pluggable backends.
You drive the game loop; `retroglyph` handles drawing efficiently and feeding you input events.

The same game code runs unchanged against a real terminal, a native window, or a browser tab: swap
the `Backend` type parameter and nothing else changes. See
[How retroglyph compares](#how-retroglyph-compares) for how that's different from the alternatives.

<details>
<summary><strong>Table of contents</strong></summary>

- [Crates](#crates)
- [Features](#features)
- [Quick start](#quick-start)
- [Examples](#examples)
- [How retroglyph compares](#how-retroglyph-compares)
- [Documentation](#documentation)
- [Contributing](#contributing)
- [License](#license)

</details>

## Crates

`retroglyph-core` is the only required dependency; everything else is an optional backend or drawing
helper you pull in as needed. Each crate versions independently (see
[RELEASING.md](RELEASING.md#versioning)): a `core` change commonly cascades a bump to its
dependents, but a leaf-crate change bumps only that crate.

| Crate                                    | Description                                                               | Version                                                                                                                                       |
| ---------------------------------------- | ------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| [`-core`](crates/core)                   | `no_std`-compatible foundation: grid, tile, style, color, `Backend` trait | [![retroglyph-core version](https://img.shields.io/crates/v/retroglyph-core.svg)](https://docs.rs/retroglyph-core)                            |
| [`-terminal`](crates/terminal)           | Shared ANSI/SGR cell-diff renderer for the terminal-family backends       | [![retroglyph-terminal version](https://img.shields.io/crates/v/retroglyph-terminal.svg)](https://docs.rs/retroglyph-terminal)                |
| [`-crossterm`](crates/crossterm)         | Terminal backend via [`crossterm`](https://crates.io/crates/crossterm)    | [![retroglyph-crossterm version](https://img.shields.io/crates/v/retroglyph-crossterm.svg)](https://docs.rs/retroglyph-crossterm)             |
| [`-terminal-wasm`](crates/terminal-wasm) | Browser terminal backend (e.g. xterm.js) over pushed/pulled ANSI I/O      | [![retroglyph-terminal-wasm version](https://img.shields.io/crates/v/retroglyph-terminal-wasm.svg)](https://docs.rs/retroglyph-terminal-wasm) |
| [`-window`](crates/window)               | Shared `winit` windowing layer for windowed backends                      | [![retroglyph-window version](https://img.shields.io/crates/v/retroglyph-window.svg)](https://docs.rs/retroglyph-window)                      |
| [`-software`](crates/software)           | Pixel backend via `softbuffer`: native window or browser canvas           | [![retroglyph-software version](https://img.shields.io/crates/v/retroglyph-software.svg)](https://docs.rs/retroglyph-software)                |
| [`-gl`](crates/gl)                       | GPU backend via `glow`: OpenGL 3.3 (native) and WebGL2 (wasm)             | [![retroglyph-gl version](https://img.shields.io/crates/v/retroglyph-gl.svg)](https://docs.rs/retroglyph-gl)                                  |
| [`-widgets`](crates/widgets)             | Immediate-mode drawing: panels, gauges, tables, sparklines, layout        | [![retroglyph-widgets version](https://img.shields.io/crates/v/retroglyph-widgets.svg)](https://docs.rs/retroglyph-widgets)                   |

## Features

<details open>
<summary><strong>Grid API</strong> — place styled characters on a multi-layer grid with full color support</summary>

Up to 256 layers. Each cell carries a glyph, foreground/background color, text modifiers (bold,
italic, underline, blink, reverse, dim, hidden, strikethrough), and sub-cell pixel offsets. Layer 0
is always allocated; layers 1+ are allocated on first write — single-layer games pay zero overhead.

Colors cover the full spectrum: the terminal's default foreground/background, the 16 standard ANSI
colors, the 256-color palette, and 24-bit RGB.

</details>

<details>
<summary><strong>Double buffering</strong> — diff-based presentation sends only changed cells</summary>

`Terminal::present()` compares the current frame against the previous one and forwards only the
changed cells to the backend. Pixel-based backends (software renderer) request full frames because
sub-cell offsets can leave orphaned pixels from the previous frame.

</details>

<details>
<summary><strong>Stateful drawing API</strong> — chainable builder for everyday rendering</summary>

Set the active style with `fg()`, `bg()`, `modifier()`, then place characters with `put()`. Print
strings with `print()` (handles newlines and wide characters), render styled spans with
`print_styled()`, or lay out text in a bounded rectangle with `print_box()`. Clear the active layer,
all layers, or a rectangular region. Switch layers with `layer(id)`. Or bypass the builder and
access the grid directly via `grid_mut()`.

</details>

<details>
<summary><strong>Text layout and word wrapping</strong> — styled spans with configurable alignment</summary>

`Span` and `Line` provide styled text primitives. `TextLayout` is a builder that word-wraps a `Line`
to a bounded rectangle, then positions it with independent horizontal and vertical alignment
(left/center/right, top/middle/bottom). Measure the result before rendering with
`TextLayout::measure()`.

</details>

<details>
<summary><strong>Game loop</strong> — implement <code>App</code> once, run on every backend</summary>

Implement the `App` trait (the update-side dual of `Backend`) and run it with a single
feature-selected entry point. Terminal backends use the generic `run_blocking` driver; the
software/winit backend uses its inverted driver; both share the same `App`, `Frame`, and `Flow`
types. `FrameClock` is a pure fixed-timestep accumulator (fed elapsed `dt`, so it is
`no_std`-clean). The low-level `poll`/`present` API remains for turn-based games and headless tests.

</details>

<details>
<summary><strong>Scrolling camera and map loading</strong> — worlds larger than the screen</summary>

`Camera` is a viewport onto a larger world: it converts between world and screen coordinates, clamps
to the map edges while following a target, and iterates the visible cells as `(world, screen)`
pairs. `Grid::from_charmap` builds a styled grid from an ASCII map or level string, one tile per
character. Combined with multi-layer compositing, this drives scrolling roguelikes (see the
`12_dungeon_scroll` and `15_outpost_dashboard` examples).

</details>

<details>
<summary><strong>Extended grapheme cluster support</strong> — combining marks, emoji, and CJK wide chars</summary>

With the `egc` feature (enabled by default), the library handles full Unicode grapheme clusters:
combining marks, ZWJ emoji sequences, and multi-codepoint characters. CJK characters and emoji
automatically occupy two grid columns with a transparent spacer in the adjacent cell.
Multi-codepoint graphemes are capped at 8 codepoints to prevent combining-mark bombs.

</details>

<details>
<summary><strong>Pluggable backends</strong> — swap rendering targets without touching game logic</summary>

The `Backend` trait bundles three independent facets: `Output` (draw cells, flush, resize), `Input`
(poll/push events), and `Cursor` (show/hide, move). A backend implements whichever facets it
actually needs -- `Backend` itself has no methods of its own, and any type implementing all three
gets it for free.

- **Headless** (`retroglyph-core`) — in-memory with no I/O. The workhorse for unit and integration
  tests. Provides `format_view()` for snapshot testing with insta and `push_event()` for synthetic
  input -- see
  ["Driving `Headless` with synthetic events"](docs/testing.md#driving-headless-with-synthetic-events)
  for the full workflow.
- **Crossterm** (`retroglyph-crossterm`) — full terminal with raw mode, alternate screen, and mouse
  capture. Registers a panic hook to safely restore the terminal on crashes. Feature `tracing`
  instruments `draw`/`flush`/`poll_event` with spans for profiling. Generic over its content writer
  (`Crossterm<W>`, default `BufWriter<Stdout>`) — `Crossterm::with_writer` renders to a file, a
  pipe, or an in-memory buffer instead, useful for tests that want to inspect the emitted ANSI
  output without a real TTY.
- **Software** (`retroglyph-software`) — pixel-based rendering via winit + softbuffer. Uses a 1-bit
  bitmap font (embedded Unscii 16 with feature `default-font`), with sub-cell pixel offsets,
  multi-layer compositing, a configurable scale factor, and a headless mode for pixel-level testing.
  Runs unchanged as a native window or a browser `<canvas>` (WASM).
- **Terminal (WASM)** (`retroglyph-terminal-wasm`) — pushes ANSI output to a browser terminal
  emulator such as xterm.js instead of a native TTY.
- **Sprite tilesets** (feature `tilesets` on `retroglyph-software`) — PNG sprite sheets mapped to a
  codepage (CP437, Unicode range, or custom), rendered with RGBA alpha blending over bitmap font
  glyphs.

</details>

<details>
<summary><strong>Input handling</strong> — keyboard, mouse, resize, and close events</summary>

`Terminal::poll(timeout)` returns `Option<Event>` with support for keyboard (all standard keys +
modifier flags), mouse (buttons, movement, scroll), touch (synthesized into the same mouse events on
the software/WASM backend), window resize, and close events. `has_input()` checks for events without
blocking. Resize events are automatically applied to the grid before the event reaches your code.

</details>

<details>
<summary><strong><code>no_std</code> compatible</strong> — core crate compiles without <code>std</code></summary>

Disable the `std` feature (requires an allocator). Useful for embedded or kernel-space roguelikes.

</details>

<details>
<summary><strong>Widgets</strong> (crate <code>retroglyph-widgets</code>) — panels, gauges, tables, and a
layout splitter, built on <code>retroglyph-core</code></summary>

An optional crate: games that draw manually depend only on `retroglyph-core`. Every widget is
primarily a free function (`panel`, `gauge`, `table`, `sparkline`, `draw_box`, ...) that draws
directly to a `Terminal` and retains no state, plus a constraint-based `Rect` splitter
(`split_h`/`split_v`) with `Fixed`/`Percent`/`Fill`/`Min`/`Max` constraints and `Flex` alignment
(`Start`/`End`/`Center`/`SpaceBetween`/`SpaceAround`) -- deliberately similar to
[ratatui](https://ratatui.rs)'s layout system, for anyone coming from there. `Fill(weight)` claims a
share of the leftover space proportional to `weight` relative to the other `Fill`/`Min`/`Max` panes
in the same split (`Fill(1)` reproduces plain equal distribution).

Three optional layers build on that free-function core:

- `Widget`/`StatefulWidget` traits for callers who want to box or store heterogeneous widgets,
  backed by `ListState` for selection and scroll position.
- `BoxStyle`, a Lip-Gloss-style box model (padding, border, margin) rendered into a standalone
  `Grid`. `Paragraph` (behind the `egc` feature) word-wraps text via `retroglyph-core`'s
  `TextLayout` and implements a `Measure` trait so a caller can size a pane to fit before rendering.
- `join_h`/`join_v` to compose several `Grid`s -- e.g. `BoxStyle::render` output -- into one before
  drawing it.
- `Theme` (`Theme::DARK`/`Theme::LIGHT`, or a caller-built palette): named color roles (`border`,
  `accent`, `hover_bg`, ...) that every widget with a style knob can pick up via a `.theme(Theme)`
  builder method, optionally -- a manual `.border_style(...)`/etc. call after `.theme(...)` still
  wins, and nothing requires a `Theme` at all.

See the `09_widgets_dashboard` and `15_outpost_dashboard` examples for all of the above wired
together in one UI, `17_theme_switch` for `Theme::DARK`/`Theme::LIGHT` switched live at runtime by a
keypress, or `18_weighted_fill` for `Fill(weight)`'s proportional splits.

</details>

## Quick start

The library is split into a `no_std` core plus per-backend crates. For a terminal app you need the
core and the crossterm backend:

```toml
[dependencies]
retroglyph-core = "0.1"
retroglyph-crossterm = "0.1"
```

```rust,no_run
use retroglyph_core::{Terminal, Color, event::{Event, KeyCode}};
use retroglyph_crossterm::Crossterm;

fn main() -> std::io::Result<()> {
    let mut term = Terminal::new(Crossterm::new()?);
    loop {
        term.fg(Color::GREEN);
        term.put(5, 5, '@');
        term.present()?;

        if let Some(Event::Key(k)) = term.poll(std::time::Duration::from_secs(1)) {
            if k.code == KeyCode::Char('q') {
                break;
            }
        }
    }
    Ok(())
}
```

This exact snippet is compiled and run as a doctest on every `cargo test` (see
`crates/crossterm/src/lib.rs`), so it can't silently drift out of date.

Want a native window or a browser tab instead of a real terminal? See
[`retroglyph-software`](crates/software)'s quick start (same `Terminal`/`Backend` API, a different
`Backend` type). Every crate in the [table above](#crates) has its own tested quick start.

## Examples

`examples/examples/*.rs` has 18 runnable examples, from a minimal `01_hello_world` up to
`18_text_align`, with `15_outpost_dashboard` a flagship dashboard exercising animation, touch-sized
controls, and a responsive layout. Every example runs on every backend unchanged:

```sh
cargo run --example 12_dungeon_scroll --features crossterm  # real terminal
cargo run --example 12_dungeon_scroll --features software   # native window
cargo run --bin runner                                      # interactive picker (all examples x all backends, incl. WASM)
```

Every example is also built for WASM (Headless/Terminal/Software variants) and published as an
interactive gallery at **[main.retroglyph.dev/examples](https://main.retroglyph.dev/examples/)** --
no local toolchain required to try one in a browser.

## How retroglyph compares

There's no shortage of Rust terminal/ASCII libraries; here's where retroglyph sits relative to the
two closest:

- **[ratatui](https://ratatui.rs)** is the standard for terminal UIs, with a much larger widget
  ecosystem. It only draws to a real terminal (through `crossterm`/`termion`/`termwiz`), and has no
  pixel or WASM backend. retroglyph's widget/layout crate deliberately borrows ratatui's
  constraint-based layout ergonomics, but retroglyph's `Terminal<B>` also runs against a native
  pixel-rendered window or a browser canvas without changing a line of game logic -- pick ratatui if
  a real terminal is always the target and you want its wider widget catalog.
- **[bracket-lib](https://github.com/amethyst/bracket-lib)** (the maintained successor to RLTK) is
  the closest match in spirit: one virtual ASCII terminal, several swappable backends including
  crossterm. Its non-terminal backends go through OpenGL or WebGPU, though, which pulls in a GPU
  stack; retroglyph's software backend is pure CPU rasterization (`softbuffer`, no GPU dependency),
  and its core crate is `no_std`-compatible for embedded/kernel-space use, which bracket-lib doesn't
  target.

If neither of those trade-offs match what you need, retroglyph is probably not the right choice
either -- these are the two libraries actually worth comparing against, not exhaustive coverage of
the space.

## Documentation

- docs.rs links for every crate are in the [crates table](#crates) above.
- [main.retroglyph.dev](https://main.retroglyph.dev/) is the full docs site: rustdoc for every
  crate, plus the [examples gallery](https://main.retroglyph.dev/examples/).
- [`llms.txt`](llms.txt) / [`llms-full.txt`](llms-full.txt) are a machine-readable summary of every
  public module and type, generated by `just llms` -- useful context for AI coding agents working in
  this repo or against these crates.
- [`STYLE_GUIDE.md`](STYLE_GUIDE.md) documents this project's Rust API and code style conventions,
  for anyone contributing.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for the development setup, and [AGENTS.md](AGENTS.md) /
[`STYLE_GUIDE.md`](STYLE_GUIDE.md) for the conventions this workspace holds itself to (`just check`
must pass before any commit).

## License

Licensed under the [MIT license](LICENSE).
