# retroglyph-terminal-wasm

[![crates.io](https://img.shields.io/crates/v/retroglyph-terminal-wasm.svg)](https://crates.io/crates/retroglyph-terminal-wasm)
[![docs.rs](https://img.shields.io/docsrs/retroglyph-terminal-wasm)](https://docs.rs/retroglyph-terminal-wasm)
[![coverage](https://codecov.io/gh/crates-lurey-io/retroglyph/graph/badge.svg?token=z8BBUp8fiY&flag=terminal-wasm)](https://codecov.io/gh/crates-lurey-io/retroglyph)
[![license](https://img.shields.io/crates/l/retroglyph-terminal-wasm.svg)](https://github.com/crates-lurey-io/retroglyph/blob/main/LICENSE)

A WASM/browser terminal backend for [retroglyph](https://github.com/crates-lurey-io/retroglyph),
driven by pushed input and pulled ANSI output. `TerminalWasm` implements `Backend` directly (like
the core crate's `Headless`): there is no event loop here. A browser terminal emulator (e.g.
xterm.js -- this crate has no dependency on it and no opinion about which one is used) is driven
from JS, which calls into this crate once per animation frame (or on demand) to pull freshly
rendered ANSI bytes and push back any input it collected.

## Quick start

```toml
[dependencies]
retroglyph-core = "0.1"
retroglyph-terminal-wasm = "0.1"
```

```rust
use retroglyph_core::Terminal;
use retroglyph_terminal_wasm::TerminalWasm;

let backend = TerminalWasm::new(80, 24);
let mut term = Terminal::new(backend);
term.put(0, 0, '@');
term.present().unwrap();
let ansi = term.backend_mut().take_output();
assert!(ansi.contains('@'));
```

See [docs.rs](https://docs.rs/retroglyph-terminal-wasm) for the full API, including the JS-facing
side of the pushed-event/pulled-output contract.
