# AGENTS.md

`retroglyph` is a terminal/grid rendering library for roguelikes. It provides a double-buffered
`Terminal<B>` generic over a pluggable `Backend`, with styled cells, input events, and optional
software/crossterm/WASM backends.

## Correctness gate

**Run `just check` before every commit.** It runs fmt-check, clippy, compile, tests, doc, and
llms-check in one shot. All clippy lints (including `pedantic` and `nursery`) are errors; so is
`missing_docs`.

```sh
just check          # full gate — must pass before committing
just fmt            # auto-fix Rust + Markdown/JSON formatting
just test           # cargo test --all-features
just test-v         # same but with stdout (useful for snapshot review)
just clippy         # clippy only
just compile        # cargo check --all-features
just doc            # private rustdocs (opens in browser)
just llms           # regenerate llms.txt / llms-full.txt
```

For a quick iterative loop: `just compile` to catch type errors fast, then `just check` before
committing.

## Crate layout

```text
src/
  backend/
    mod.rs           Backend trait + re-exports
    headless.rs      In-memory backend (testing)
    crossterm.rs     Crossterm backend (feature-gated)
    software/        Pixel backend: winit + softbuffer, tilesets, bitmap fonts
  color.rs           Color (Default / ANSI / Indexed / RGB)
  event.rs           Event, KeyEvent, MouseEvent
  grid.rs            Grid, Pos, Rect, Size
  layout.rs          TextLayout, HAlign, VAlign (feature = "egc")
  style.rs           Style, CellModifier
  terminal.rs        Terminal<B> — stateful drawing API, double buffering
  text.rs            Line, Span
  tile.rs            Tile — glyph + Style + sub-cell offsets
```

## Feature flags

| Flag                    | Description                                                     |
| ----------------------- | --------------------------------------------------------------- |
| `std` (default)         | Enable std-dependent code; disable for `no_std`.                |
| `egc` (default)         | Extended grapheme cluster support via `unicode-segmentation`.   |
| `crossterm`             | Crossterm terminal backend.                                     |
| `software`              | Pixel backend (winit + softbuffer).                             |
| `software-tilesets`     | PNG sprite sheet tilesets + alpha blending. Implies `software`. |
| `software-default-font` | Embedded VGA 8×16 bitmap font. Implies `software`.              |

## Testing

Unit tests live alongside their modules. `tests/e2e.rs` drives `Terminal<Headless>` through
game-logic scenarios and asserts on grid state.

### Snapshot tests (insta)

`Headless::format_view()` renders the grid to text (spaces → `·`). Use it with
`insta::assert_snapshot!` for layout assertions. Snapshot files live in `tests/snapshots/` and are
committed.

```sh
cargo insta test    # run and open review UI
cargo insta accept  # accept pending snapshots
```

### E2E visual snapshots (crossterm)

`tests/e2e_snapshots.rs` spawns `crossterm_demo` in a pseudo-terminal, feeds key input, parses ANSI
via the `vt100` crate, and snapshots the result as SVG.

```sh
cargo build --example crossterm_demo --features crossterm
cargo test --test e2e_snapshots --all-features
open tests/snapshots/crossterm_demo.svg   # visual diff
```

## Key rules

- **No `eprintln!` in library code.** Use the `log` crate (feature-gated). Fatal backend init
  errors: `log::error!` + `event_loop.exit()`, not `panic!`.
- **`unsafe_code` is forbidden** (`Cargo.toml` lint).

## Pre-commit hooks

`hk` (configured in `hk.pkl`) runs on every `jj push` via `jj-hooks`:

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets -- -D warnings`

```sh
cargo bin hk run pre-commit   # run manually
JJ_HOOKS_SKIP=1 jj push       # bypass (use sparingly)
```

## Docs

- `docs/design/` — ADRs and milestone plans. Read the relevant plan before starting a feature.
- `docs/references/` — deep-dives on backends, Unicode, font rendering, and library comparisons.
- `docs/style/` — Rust API and code style guidelines. New modules must follow these.
