# retroglyph-crossterm

[![crates.io](https://img.shields.io/crates/v/retroglyph-crossterm.svg)](https://crates.io/crates/retroglyph-crossterm)
[![docs.rs](https://img.shields.io/docsrs/retroglyph-crossterm)](https://docs.rs/retroglyph-crossterm)
[![coverage](https://codecov.io/gh/crates-lurey-io/retroglyph/graph/badge.svg?token=z8BBUp8fiY&flag=crossterm)](https://codecov.io/gh/crates-lurey-io/retroglyph)
[![license](https://img.shields.io/crates/l/retroglyph-crossterm.svg)](https://github.com/crates-lurey-io/retroglyph/blob/main/LICENSE)

A `Backend` implementation for [retroglyph](https://github.com/crates-lurey-io/retroglyph) that
renders to a real terminal via [`crossterm`](https://crates.io/crates/crossterm). Owns the OS/TTY-
specific parts (raw mode, the alternate screen, the Kitty keyboard protocol, input polling); cell
diffing and ANSI/SGR output are delegated to
[`retroglyph-terminal`](https://crates.io/crates/retroglyph-terminal).

## Quick start

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

## RGB colors on 256-color terminals

`Color::Rgb` is written out as a truecolor SGR sequence with no quantization to a 256-color or
16-color palette -- see
[`retroglyph-terminal`'s "RGB color fallback" docs](https://docs.rs/retroglyph-terminal) for the
full contract. On terminals that don't support truecolor, the emitted color depends on the
terminal/multiplexer's own handling of the extended SGR sequence; use `Color::Indexed` or
`Color::Ansi` instead of `Color::Rgb` if you need an unambiguous color on such a terminal.

See [docs.rs](https://docs.rs/retroglyph-crossterm) for the full API, or the
[workspace README](https://github.com/crates-lurey-io/retroglyph#readme) for the crate list and more
examples.
