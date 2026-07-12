# retroglyph-core

The `no_std`-compatible foundation of [retroglyph](https://github.com/crates-lurey-io/retroglyph):
grid, tile, style, color, text, terminal, and event types, plus the `Backend` trait and a
dependency-free `Headless` test backend. Platform backends
([`retroglyph-crossterm`](https://crates.io/crates/retroglyph-crossterm),
[`retroglyph-software`](https://crates.io/crates/retroglyph-software)) and drawing helpers
([`retroglyph-widgets`](https://crates.io/crates/retroglyph-widgets)) are separate crates that
depend on this one.

```rust
use retroglyph_core::{Terminal, Color, backend::Headless};

let mut term = Terminal::new(Headless::new(80, 24));
term.fg(Color::GREEN);
term.put(5, 5, '@');
term.present().unwrap();
```

See [docs.rs](https://docs.rs/retroglyph-core) for the full API, or the
[workspace README](https://github.com/crates-lurey-io/retroglyph#readme) for the crate list and a
real backend quick start.
