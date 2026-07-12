# retroglyph-terminal

Shared ANSI/SGR cell-diff renderer for [retroglyph](https://github.com/crates-lurey-io/retroglyph)'s
terminal-family backends. `TerminalRenderer` converts tile content into standard ANSI/CSI escape
sequences and writes them to any `std::io::Write` sink; it has no opinion about where those bytes
end up or how input arrives. Two crates plug it into a concrete environment:
[`retroglyph-crossterm`](https://crates.io/crates/retroglyph-crossterm) (a real TTY) and
[`retroglyph-terminal-wasm`](https://crates.io/crates/retroglyph-terminal-wasm) (a browser terminal
emulator such as xterm.js).

Most consumers don't depend on this crate directly; use `retroglyph-crossterm` or
`retroglyph-terminal-wasm` instead. See [docs.rs](https://docs.rs/retroglyph-terminal) for the API,
or the [workspace README](https://github.com/crates-lurey-io/retroglyph#readme) for a real backend
quick start.
