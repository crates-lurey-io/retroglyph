# Contributing to retroglyph

## Development

Prerequisites:

- Rust (latest stable)
- Node.js (v22.12.0 LTS via `.nvmrc`)

### Workflow

`just check` is the gate before every commit. All clippy lints (including `pedantic` and `nursery`)
are treated as errors.

| Command                | What it does                                                                     |
| ---------------------- | -------------------------------------------------------------------------------- |
| `just check`           | Full gate: fmt-check, lint, compile, test, doc                                   |
| `just clippy`          | Run clippy with `-D warnings` on all targets                                     |
| `just compile`         | `cargo check --all-features`                                                     |
| `just deny-advisories` | `cargo deny check advisories`                                                    |
| `just deny-licenses`   | `cargo deny check bans licenses sources`                                         |
| `just doc`             | Generate private rustdocs, regenerate `llms.txt` / `llms-full.txt`, open browser |
| `just fmt`             | Format Rust + Markdown/JSON/YAML files                                           |
| `just fmt-check`       | Verify formatting without modifying (for CI)                                     |
| `just lint`            | Clippy + markdownlint                                                            |
| `just test`            | Run all tests with all features                                                  |
| `just test-v`          | Run all tests with stdout visible                                                |

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
  style.rs          Style
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
use retroglyph::{Terminal, backend::Headless};

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

`tests/e2e_snapshots.rs` spawns the compiled `demo` binary (built with `--features crossterm`) in a
real pseudo-terminal using `portable-pty`, feeds it key input, then parses the raw ANSI byte stream
with a VT100 emulator (`vt100` crate) to reconstruct the final screen state. The screen is rendered
to SVG and snapshotted with `insta`.

```sh
# The demo binary must be built first

cargo build --example demo --features crossterm

cargo test --test e2e_snapshots --all-features
```

Two files are written to `tests/snapshots/` on each run:

| File                                | Purpose                                                 |
| ----------------------------------- | ------------------------------------------------------- |
| `e2e_snapshots__demo_snapshot.snap` | Insta snapshot (authoritative, diffed by CI)            |
| `demo.svg`                          | Rendered SVG — open directly in a browser or Quick Look |

GitHub renders `.svg` files, so PR diffs show a visual before/after when the snapshot changes.

To view the current snapshot locally:

```sh
open tests/snapshots/demo.svg
```

## Feature flags

| Flag                    | Default | Description                                                        |
| ----------------------- | ------- | ------------------------------------------------------------------ |
| `std`                   | on      | Enable `std`-dependent code. Disable for `no_std` builds.          |
| `crossterm`             | off     | Enable the `Crossterm` backend. Pulls in the `crossterm` crate.    |
| `software`              | off     | Software pixel backend (winit + softbuffer).                       |
| `software-tilesets`     | off     | PNG sprite sheet tilesets with alpha blending. Implies `software`. |
| `software-default-font` | off     | Include the embedded VGA 8x16 bitmap font. Implies `software`.     |
| `egc`                   | on      | Extended grapheme cluster support (combining marks, wide chars).   |
