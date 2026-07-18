# retroglyph-terminal

[![crates.io](https://img.shields.io/crates/v/retroglyph-terminal.svg)](https://crates.io/crates/retroglyph-terminal)
[![docs.rs](https://img.shields.io/docsrs/retroglyph-terminal)](https://docs.rs/retroglyph-terminal)
[![coverage](https://codecov.io/gh/crates-lurey-io/retroglyph/graph/badge.svg?token=z8BBUp8fiY&flag=terminal)](https://codecov.io/gh/crates-lurey-io/retroglyph)
[![license](https://img.shields.io/crates/l/retroglyph-terminal.svg)](https://github.com/crates-lurey-io/retroglyph/blob/main/LICENSE)

Shared ANSI/SGR cell-diff renderer for [retroglyph](https://github.com/crates-lurey-io/retroglyph)'s
terminal-family backends. `TerminalRenderer` converts tile content into standard ANSI/CSI escape
sequences and writes them to any `std::io::Write` sink; it has no opinion about where those bytes
end up or how input arrives. Two crates plug it into a concrete environment:
[`retroglyph-crossterm`](https://crates.io/crates/retroglyph-crossterm) (a real TTY) and
[`retroglyph-terminal-wasm`](https://crates.io/crates/retroglyph-terminal-wasm) (a browser terminal
emulator such as xterm.js).

## Quick start

Most consumers don't depend on this crate directly -- use `retroglyph-crossterm` or
`retroglyph-terminal-wasm` instead, both of which re-export the pieces of a real app's quick start.
This crate's own surface is the lower-level `Tile` -> ANSI bytes transform those two backends share:

```toml
[dependencies]
retroglyph-terminal = "0.1"
retroglyph-core = "0.1"
```

```rust
use retroglyph_core::grid::Pos;
use retroglyph_core::style::Style;
use retroglyph_core::tile::Tile;
use retroglyph_terminal::TerminalRenderer;

// Any `std::io::Write` sink works -- a `Vec<u8>` here, `Stdout` in
// `retroglyph-crossterm`, a `String` buffer in `retroglyph-terminal-wasm`.
let mut renderer = TerminalRenderer::new(Vec::new());
let tile = Tile::new('@', Style::default());
renderer
    .draw(core::iter::once((Pos::new(0, 0), &tile, None)))
    .unwrap();
renderer.flush().unwrap();

let ansi = String::from_utf8(renderer.into_writer()).unwrap();
assert!(ansi.contains('@'));
```

## RGB color fallback on 256-color terminals

`Color::Rgb` tiles are written out as a 24-bit truecolor SGR sequence (`38;2;r;g;b`) with no
quantization down to the 256-color or 16-color ANSI palettes -- neither this crate nor
`retroglyph-core` guarantees a `to_indexed()`-style quantizer. This matches `crossterm`'s own
color-writing behavior: the terminal (or an in-between multiplexer) is responsible for interpreting
or degrading truecolor codes it doesn't natively support. See the crate-level docs for the full
contract and its known limitations; use `Color::Indexed`/`Color::Ansi` instead of `Color::Rgb` if
you need a specific, unambiguous color on a known-limited terminal.

See [docs.rs](https://docs.rs/retroglyph-terminal) for the API, or the
[workspace README](https://github.com/crates-lurey-io/retroglyph#readme) for a real backend quick
start.
