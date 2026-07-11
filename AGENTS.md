# AGENTS.md

`retroglyph` is a 2D pseudographic terminal library. It provides a double-buffered `Terminal<B>`
generic over a pluggable `Backend`, with styled cells, input events, and optional
software/crossterm/WASM backends.

A workspace split into `retroglyph-core`, `retroglyph-crossterm`, `retroglyph-software`, and a
`retroglyph` facade crate is planned (ADR 014). The current single-crate structure with feature
flags will be preserved at the user-facing level.

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
just doc            # private rustdocs + llms.txt
just docs-preview   # build docs and open in browser
just llms           # regenerate llms.txt / llms-full.txt
```

For a quick iterative loop: `just compile` to catch type errors fast, then `just check` before
committing.

**Known Justfile gaps:** `clippy` does not pass `--all-features`, so backend code is only linted in
CI. The `doc` recipe swallows `cargo doc` failures due to `|| true` chaining. See
`.matan/improve-justfile.md` for the full list.

## Crate layout

```text
src/
  lib.rs             Root: module declarations, feature-gated re-exports
  color.rs           Color (Default / ANSI / Indexed / RGB)
  style.rs           Style (depends on: color)
  tile.rs            Tile — glyph + Style + sub-cell offsets (depends on: style)
  grid.rs            Grid, Pos, Rect, Size (depends on: tile, style; uses grixy)
  event.rs           Event, KeyEvent, MouseEvent (depends on: grid::Pos)
  text.rs            Line, Span (depends on: style)
  layout.rs          TextLayout, HAlign, VAlign (depends on: text, terminal; feature = "egc")
  terminal.rs        Terminal<B> — stateful drawing API, double buffering (depends on: all above)
  backend/
    mod.rs           Backend trait (depends on: event, grid, tile)
    headless.rs      In-memory backend for testing (no external deps)
    crossterm.rs     Crossterm backend (feature = "crossterm")
    software/
      mod.rs         SoftwareRenderer, winit event loop (feature = "software")
      config.rs      SoftwareBackend, SoftwareBackendBuilder
      bitmap_font.rs BitmapFont, embedded VGA 8x16
      windowed.rs    WindowedBackend trait
      tileset.rs     Codepage, TilesetBuilder (feature = "software-tilesets")
      sprite_cache.rs  SpriteCache, alpha blending (feature = "software-tilesets")
examples/            Runnable demos (crossterm, software, tileset, headless, WASM)
tests/
  e2e.rs             Terminal<Headless> integration tests
  e2e_snapshots.rs   PTY + vt100 SVG snapshots of the demo example (crossterm backend)
  software_renderer.rs  Pixel-level software backend tests
  snapshots/         insta snapshot files (committed)
```

The internal dependency flow is: `color -> style -> tile -> grid`, with `event` depending only on
`grid::Pos`, and `terminal` pulling everything together. Each backend depends on the core types but
not on other backends. This is the natural crate boundary for the planned workspace split (ADR 014).

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

`tests/e2e_snapshots.rs` spawns the `demo` binary (built with `--features crossterm`) in a
pseudo-terminal, feeds key input, parses ANSI via the `vt100` crate, and snapshots the result as
SVG.

```sh
cargo build --example demo --features crossterm
cargo test --test e2e_snapshots --all-features
open tests/snapshots/demo.svg   # visual diff
```

## Key rules

- **Comment/doc-comment line width: use the full ~100 cols, not ~80.** There's no `rustfmt.toml` in
  this repo, so rustfmt's default `max_width = 100` applies, and `wrap_comments` is `false` by
  default (rustfmt never rewraps prose comments for you). Don't hand-wrap doc comments to ~80 cols
  out of habit; wrap near the real 100-col budget instead.
- **No `eprintln!` in library code.** Use the `log` crate (feature-gated). Fatal backend init
  errors: `log::error!` + `event_loop.exit()`, not `panic!`.
- **`unsafe_code` is forbidden** (`Cargo.toml` lint).
- **No interactive jj/git commands.** Always pass `-m` to avoid opening `$EDITOR`. Use
  `jj split [FILESETS...]` by path, never interactively.
- **Read the relevant ADR before starting a feature.** ADRs in `docs/design/` capture constraints
  and non-goals that aren't obvious from the code alone.
- **`docs/design/` is internal, not public API surface.** It's for contributors reading the repo,
  not for crate consumers. Never reference `docs/design/*.md` paths, ADR numbers, or ADR titles from
  doc comments, rustdoc, README snippets, or anything else that ends up in published API docs --
  those readers have no access to the file and the reference is dead weight. If a doc comment needs
  the rationale, restate the relevant part of it inline instead of pointing there.

## Pre-push hooks

`hk` (configured in `hk.pkl`) runs on every `jj push` via `jj-hooks`:

- `just fmt-check` (rustfmt + prettier)
- `just lint` (clippy + markdown lint)

```sh
cargo bin hk run pre-push     # run manually
JJ_HOOKS_SKIP=1 jj push       # bypass (use sparingly)
```

## Docs

- `docs/design/` — ADRs and milestone plans. Internal only: read the relevant ADR before starting a
  feature, but never cite it (path, number, or title) from doc comments or anything published to
  rustdoc/docs.rs.
  - Key ADRs: 001 (architecture), 004 (testing strategy), 008 (layer composition), 013 (codecov),
    014 (workspace split), 018 (terminal family split: retroglyph-terminal +
    retroglyph-crossterm/retroglyph-terminal-wasm implementors).
- `docs/references/` — deep-dives organized by topic:
  - `backends/` — terminal, software rendering, WebGL, WASM, etc.
  - `core/` — color systems, font rendering, Unicode, game loops, threading, error handling.
  - `libs/` — comparisons with ratatui, bracket-lib, notcurses, libtcod, etc.
- `docs/style/` — Rust API and code style guidelines. New modules must follow these.
- `llms.txt` / `llms-full.txt` — machine-readable workspace overview generated by `just llms`
  (README + Cargo metadata). `just doc` additionally generates a per-crate pair under
  `target/doc/<crate>/` with that crate's actual public API surface (see `tools/gen-llms-txt.sh`);
  the full version includes all public type signatures and doc comments.
