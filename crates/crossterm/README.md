# retroglyph-crossterm

[![crates.io](https://img.shields.io/crates/v/retroglyph-crossterm.svg)](https://crates.io/crates/retroglyph-crossterm)
[![docs.rs](https://img.shields.io/docsrs/retroglyph-crossterm)](https://docs.rs/retroglyph-crossterm)
[![coverage](https://codecov.io/gh/crates-lurey-io/retroglyph/graph/badge.svg?token=z8BBUp8fiY&flag=crossterm)](https://app.codecov.io/gh/crates-lurey-io/retroglyph/flags)
[![license](https://img.shields.io/crates/l/retroglyph-crossterm.svg)](https://github.com/crates-lurey-io/retroglyph/blob/main/LICENSE)

A `Backend` implementation for [retroglyph](https://github.com/crates-lurey-io/retroglyph) that
renders to a real terminal via [`crossterm`](https://crates.io/crates/crossterm). Owns the OS/TTY-
specific parts (raw mode, the alternate screen, the Kitty keyboard protocol, input polling); cell
diffing and ANSI/SGR output are delegated to
[`retroglyph-terminal`](https://crates.io/crates/retroglyph-terminal). Registers a process-wide
panic hook (once, across all instances) to safely restore the terminal if the app panics while raw
mode/the alternate screen is active.

## Quick start

```sh
cargo add retroglyph-core retroglyph-crossterm
```

```rust,no_run
use retroglyph_core::color::{Color, Style};
use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::terminal::Terminal;
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

## Features

<!-- gen-features:start -->

This crate has no default features; every feature below is optional and off unless enabled.

### `dev`

⚪ Optional.

Forwards `retroglyph-core`'s `dev` feature, which forces development diagnostics on in a build that
would otherwise compile them out (see `retroglyph_core::dev`).

### `egc`

⚪ Optional.

Forwards to `retroglyph-terminal`'s `egc` feature (which forwards to `retroglyph-core`'s), enabling
grapheme-cluster-aware cell diffing.

This crate has no code of its own gated on the flag; it exposes it so callers don't need to know
which crate in the terminal family actually implements it.

### `tracing`

⚪ Optional.

Instruments `draw`, `flush`, and `poll_event` with `tracing` spans for profiling render/input time.

See where time is spent with any `tracing` subscriber (e.g. `tracing-subscriber`'s fmt layer, or a
flamegraph via `tracing-flame`).

<!-- gen-features:end -->

## Rendering to a non-stdout sink

`Crossterm<W>` is generic over its content writer (default `BufWriter<Stdout>`). Use
`Crossterm::with_writer`/`CrosstermOptions::build_with_writer` to render into a file, a pipe, or an
in-memory buffer: useful for capturing/asserting on the emitted ANSI output in tests without a real
TTY. Terminal-protocol setup (raw mode, the alternate screen, mouse/focus/paste/kitty) still targets
the real process stdout regardless of the writer; disable those via `CrosstermOptions` when
targeting a non-terminal sink:

```rust,no_run
use retroglyph_crossterm::Crossterm;

let mut buffer = Vec::new();
let term = Crossterm::builder()
    .raw_mode(false)
    .alt_screen(false)
    .mouse_capture(false)
    .focus_change(false)
    .bracketed_paste(false)
    .kitty_protocol(false)
    .build_with_writer(&mut buffer)?;
drop(term);
# Ok::<(), std::io::Error>(())
```

## RGB colors on 256-color terminals

`Color::Rgb` is written out as a truecolor SGR sequence with no quantization to a 256-color or
16-color palette: see
[`retroglyph-terminal`'s "RGB color fallback" docs](https://docs.rs/retroglyph-terminal) for the
full contract. On terminals that don't support truecolor, the emitted color depends on the
terminal/multiplexer's own handling of the extended SGR sequence; use `Color::Indexed` or
`Color::Ansi` instead of `Color::Rgb` if you need an unambiguous color on such a terminal.

See [docs.rs](https://docs.rs/retroglyph-crossterm) for the full API, or the
[workspace README](https://github.com/crates-lurey-io/retroglyph#readme) for the crate list and more
examples.
