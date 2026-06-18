# rg

A terminal/grid rendering library for roguelikes, written in Rust.

`rg` provides a styled character grid, double-buffered rendering, and pluggable backends. You drive
the game loop; `rg` handles drawing efficiently and feeding you input events.

## Features

- **Grid API** — place styled characters with foreground/background colors and text modifiers (bold,
  italic, underline, …).
- **Double buffering** — `Terminal::present()` diffs the current and previous frames and sends only
  changed cells to the backend.
- **Pluggable backends** — swap rendering targets without touching game logic:
  - `Headless` — in-memory, no I/O. Used in unit and integration tests.
  - `Crossterm` (feature `crossterm`) — full terminal with raw mode, alternate screen, and mouse
    capture.
- **`no_std` compatible** — the core crate compiles without `std` when the `std` feature is disabled
  (requires an allocator).

## Quick start

```toml
[dependencies]
rg = { version = "0.1", features = ["crossterm"] }
```

```rust
use rg::{Terminal, backend::Crossterm, color::Color, event::{Event, KeyCode}};

fn main() -> std::io::Result<()> {
    let mut term = Terminal::new(Crossterm::new()?);
    loop {
        term.clear();
        term.fg(Color::GREEN);
        term.put(5, 5, '@');
        term.present();

        if let Some(Event::Key(k)) = term.poll(std::time::Duration::from_secs(1)) {
            if k.code == KeyCode::Char('q') { break; }
        }
    }
    Ok(())
}
```

Run the interactive demo:

```sh
cargo run --example crossterm_demo --features crossterm
```

## Crate layout

```text
src/
  backend/
    mod.rs          Backend trait
    headless.rs     In-memory backend (testing)
    crossterm.rs    Crossterm backend (feature-gated)
  cell.rs           Cell — a glyph + Style
  color.rs          Color enum (Default / ANSI / Indexed / RGB)
  grid.rs           Grid — 2-D cell buffer, diff iterator
  style.rs          Style, CellModifier
  event.rs          Event, KeyEvent, MouseEvent
  terminal.rs       Terminal<B> — stateful drawing API, double buffering
```

## Testing

### Unit and integration tests

```sh
just test          # run everything
just test-v        # with stdout (useful for snapshot review)
cargo test --lib   # unit tests only
```

Unit tests live alongside their modules. The integration suite in `tests/e2e.rs` drives
`Terminal<Headless>` through game-logic scenarios and asserts on the grid state directly.

### Snapshot tests (`insta`)

`Headless::format_view()` converts the in-memory grid to a text string where spaces are rendered as
`·`. Pair it with `insta::assert_snapshot!` for deterministic layout assertions:

```rust
use rg::{Terminal, backend::Headless};

let backend = Headless::new(20, 5);
let mut term = Terminal::new(backend);
term.put(2, 2, 'X');
term.present();
insta::assert_snapshot!(term.backend().format_view());
```

To review and accept new or changed snapshots:

```sh
cargo install cargo-insta   # one-time
cargo insta test            # run tests and open the review UI
cargo insta accept          # accept all pending snapshots
```

Snapshot files live in `tests/snapshots/` and are committed to version control. A failing snapshot
test means visible output changed — review the diff before accepting.

### E2E visual snapshots (crossterm backend)

`tests/e2e_snapshots.rs` spawns the compiled `crossterm_demo` binary in a real pseudo-terminal using
`portable-pty`, feeds it key input, then parses the raw ANSI byte stream with a VT100 emulator
(`vt100` crate) to reconstruct the final screen state. The screen is rendered to SVG and snapshotted
with `insta`.

```sh
# The crossterm_demo binary must be built first:
cargo build --example crossterm_demo --features crossterm

cargo test --test e2e_snapshots --all-features
```

Two files are written to `tests/snapshots/` on each run:

| File                                          | Purpose                                                 |
| --------------------------------------------- | ------------------------------------------------------- |
| `e2e_snapshots__crossterm_demo_snapshot.snap` | Insta snapshot (authoritative, diffed by CI)            |
| `crossterm_demo.svg`                          | Rendered SVG — open directly in a browser or Quick Look |

GitHub renders `.svg` files, so PR diffs show a visual before/after when the snapshot changes.

To view the current snapshot locally:

```sh
open tests/snapshots/crossterm_demo.svg
```

## Development workflow

```sh
just fmt        # format Rust + Markdown/JSON/YAML
just lint       # cargo clippy + markdownlint
just check      # fmt-check, lint, test, rustdoc, llms.txt freshness
just doc        # generate and open private rustdocs
just llms       # regenerate llms.txt / llms-full.txt
```

`just check` is the gate before every commit. All clippy lints (including `pedantic` and `nursery`)
are treated as errors.

## Feature flags

| Flag        | Default | Description                                                     |
| ----------- | ------- | --------------------------------------------------------------- |
| `std`       | on      | Enable `std`-dependent code. Disable for `no_std` builds.       |
| `crossterm` | off     | Enable the `Crossterm` backend. Pulls in the `crossterm` crate. |
