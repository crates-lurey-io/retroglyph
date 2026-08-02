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
| [`-widgets`](crates/widgets)             | Builder-struct widgets: panels, gauges, tables, sparklines, layout        | [![retroglyph-widgets version](https://img.shields.io/crates/v/retroglyph-widgets.svg)](https://docs.rs/retroglyph-widgets)                   |

## Features

<details open>
<summary><strong>Game loop</strong> — implement <code>App</code> once, run on every backend</summary>

Implement the `App` trait (the update-side dual of `Backend`) and run it with a single
feature-selected entry point. Terminal backends use the generic `run_blocking`/`run_blocking_with`
drivers; the software/winit backend uses its inverted driver; both present automatically after
`update` returns and share the same `App`, `Frame`, and `Flow` types, including `Flow::Idle` for
skipping a redraw on an unchanged frame. The zero-config `run_blocking` is event-driven by default:
on `Flow::Idle` it blocks on input via `Terminal::wait_for_input` instead of calling `update` again,
so a turn-based app that's idle most of the time costs approximately nothing.
`run_blocking_with(term, app, RunOptions::animated(60))` switches to a continuously-rendering loop
capped at a fixed rate using a `FrameClock` internally, for apps that animate from `Frame::delta`
and need `update` called every tick regardless of input. `FrameClock` is a pure fixed-timestep
accumulator (fed elapsed `dt`, so it is `no_std`-clean). The low-level `poll`/`present` API remains
for turn-based games and headless tests.

</details>

<details>
<summary><strong>Extended grapheme cluster support</strong> — combining marks, emoji, and CJK wide chars</summary>

With the `egc` feature (enabled by default), the library handles full Unicode grapheme clusters:
combining marks, ZWJ emoji sequences, and multi-codepoint characters. CJK characters and emoji
automatically occupy two grid columns with a transparent spacer in the adjacent cell.
Multi-codepoint graphemes are capped at 8 codepoints to prevent combining-mark bombs.

</details>

<details>
<summary><strong>Diagnostics that ship nothing</strong> — development warnings compile out of release
builds</summary>

Warnings that only help while building a game (a sprite larger than the cells reserved for it, for
instance) sit behind `retroglyph_core::dev_only!`, which gates on `BuildMode::CURRENT`. That
resolves from `debug_assertions`, so `cargo run` reports and `cargo run --release` compiles the
check, the message, and the bookkeeping that dedupes it away entirely. A profiling build inherits
`release` and is treated as release, so what you profile is what you ship.

For an optimized build that still reports, enable `retroglyph-core`'s `dev` feature.

</details>

See [`crates/core`](crates/core) for the Grid API, double buffering, stateful drawing, text
layout/word wrapping, scrolling camera/map loading, input handling, and `no_std` support, and
[`crates/widgets`](crates/widgets) for panels, gauges, tables, and layout splitting. Every backend
in the [crates table](#crates) above links to its own README for what it adds over the `Backend`
trait (font chains, sprite tilesets, panic-safe raw mode, WASM bridging, ...).

## Quick start

The library is split into a `no_std` core plus per-backend crates. For a terminal app you need the
core and the crossterm backend:

```sh
cargo add retroglyph-core retroglyph-crossterm
```

```rust,no_run
use retroglyph_core::{Terminal, Color, Style, event::{Event, KeyCode}};
use retroglyph_crossterm::Crossterm;

fn main() -> std::io::Result<()> {
    let mut term = Terminal::new(Crossterm::new()?);
    loop {
        term.draw(|s| s.put((5, 5), '@', Style::new().fg(Color::GREEN)))?;

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

`examples/examples/*.rs` has 20 runnable examples, from a minimal `01_hello_world` up to
`20_overworld`, with `15_outpost_dashboard` a flagship dashboard exercising animation, touch-sized
controls, and a responsive layout. Every example runs on every backend unchanged:

```sh
cargo run --example 12_dungeon_scroll --features crossterm  # real terminal
cargo run --example 12_dungeon_scroll --features software   # native window
cargo run --bin runner                                      # interactive picker (all examples x all backends, incl. WASM)
```

Every windowed and crossterm example draws an `NNN fps  MM.M ms  <backend>` readout in the top-right
corner. Press `` ` `` (or `F1`) to toggle it while the example runs; in the browser, click the
floating `FPS` button. Set `RG_FPS=0` to start with it hidden.

Examples that animate over real elapsed time also honour `RG_TIME_SCALE`, a multiplier on the frame
delta they're handed (`RG_TIME_SCALE=20` runs an example twenty times faster, `0.25` a quarter
speed). It's a debugging and capture aid -- the PTY snapshot tests use it so a capture that waits on
an animation to settle doesn't have to spend real seconds waiting.

Every example is also built for WASM (Headless/Terminal/Software/WebGL variants) and published as an
interactive gallery at **[main.retroglyph.dev/examples](https://main.retroglyph.dev/examples/)** --
no local toolchain required to try one in a browser.

## How retroglyph compares

There's no shortage of Rust terminal/ASCII libraries; here's where retroglyph sits relative to the
two closest:

- **[ratatui](https://ratatui.rs)** is the standard for terminal UIs, with a much larger widget
  ecosystem. It only draws to a real terminal (through `crossterm`/`termion`/`termwiz`), and has no
  pixel or WASM backend. retroglyph's widget/layout crate borrows ratatui's constraint-based layout
  ergonomics, but retroglyph's `Terminal<B>` also runs against a native pixel-rendered window or a
  browser canvas without changing a line of game logic: pick ratatui if a real terminal is always
  the target and you want its wider widget catalog, including text attributes (bold, italic,
  underline, ...): `Style` is fg/bg color only, on purpose, so that every backend (including the
  pixel and GL renderers) behaves identically: see `Style`'s doc comment in
  `crates/core/src/style.rs`. Rich-attribute dashboards and TUIs are ratatui's turf; retroglyph
  stays the game-grid library.
- **[bracket-lib](https://github.com/amethyst/bracket-lib)** (the maintained successor to RLTK) is
  the closest match in spirit: one virtual ASCII terminal, several swappable backends including
  crossterm. Its non-terminal backends go through OpenGL or WebGPU, though, which pulls in a GPU
  stack; retroglyph's software backend is pure CPU rasterization (`softbuffer`, no GPU dependency),
  and its core crate is `no_std`-compatible for embedded/kernel-space use, which bracket-lib doesn't
  target.

If neither of those trade-offs match what you need, retroglyph is probably not the right choice
either: these are the two libraries actually worth comparing against, not exhaustive coverage of the
space.

## Documentation

- docs.rs links for every crate are in the [crates table](#crates) above.
- [main.retroglyph.dev](https://main.retroglyph.dev/) is the full docs site: rustdoc for every
  crate, plus the [examples gallery](https://main.retroglyph.dev/examples/).
- Each crate publishes an `llms.txt` / `llms-full.txt` pair on
  [main.retroglyph.dev](https://main.retroglyph.dev/), a machine-readable summary of its public
  modules and types generated by `just doc`: useful context for AI coding agents working against
  these crates.
- [`STYLE_GUIDE.md`](STYLE_GUIDE.md) documents this project's Rust API and code style conventions,
  for anyone contributing.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for the development setup, and [AGENTS.md](AGENTS.md) /
[`STYLE_GUIDE.md`](STYLE_GUIDE.md) for the conventions this workspace holds itself to (`just check`
must pass before any commit).

## License

Licensed under the [MIT license](LICENSE).
