# Design

Retroglyph is a 2D psuedographic terminal suitable for TUIs and retro-inspired
games and apps.

The frontend API is designed to be as close as possible to BearLibTerminal's
API, while still being idiomatic Rust.

## Architecture

The base traits, `TermWrite` and `TermRead`, are designed to be implemented by
backends, and used by the frontend API.

Each cell in a terminal is represented by a glyph, foreground, and background.

By default, Glyphs represent an index into a tileset (e.g. code page 437), but
the frontend API will also support custom glyphs, which can be rendered by
backends that support them.

## Supported Backends

The following backends are planned:

1. Headless rendering to an in-memory buffer, for testing or use in an 
   implementation of a custom backend.
1. Rendering to a TTY, using ANSI escape codes, using [`crossterm`][] (doesn't
   support custom fonts or tilesets).
1. Rendering to the web, using either pure HTML (e.g. a `<pre>` or `<table>`) or
  a `<canvas>` with WebGL or 2D rendering.
1. Software rendering to a window (or HTML canvas), using [`softbuffer`][].
1. Hardware-accelerated rendering to a window (or HTML canvas), using
   [`wgpu`][].

[`crossterm`]: https://crates.io/crates/crossterm
[`softbuffer`]: https://crates.io/crates/softbuffer
[`wgpu`]: https://crates.io/crates/wgpu
