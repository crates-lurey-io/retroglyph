# retroglyph-crossterm

A `Backend` implementation for [retroglyph](https://github.com/crates-lurey-io/retroglyph) that
renders to a real terminal via [`crossterm`](https://crates.io/crates/crossterm). Owns the OS/TTY-
specific parts (raw mode, the alternate screen, the Kitty keyboard protocol, input polling); cell
diffing and ANSI/SGR output are delegated to
[`retroglyph-terminal`](https://crates.io/crates/retroglyph-terminal).

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

See [docs.rs](https://docs.rs/retroglyph-crossterm) for the full API, or the
[workspace README](https://github.com/crates-lurey-io/retroglyph#readme) for the crate list and more
examples.
