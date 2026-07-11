# AGENTS.md

`retroglyph` is a 2D pseudographic terminal library. It provides a double-buffered `Terminal<B>`
generic over a pluggable `Backend`, with styled cells, input events, and pluggable
software/crossterm/WASM backends.

The workspace split (ADR 014) is done: the code lives in a Cargo workspace of per-crate members
under `crates/*` (`retroglyph-core` plus backend and helper crates), an `examples` crate, and
`tools/cargo-bin`. There is no single-crate `src/` root and no `retroglyph` facade crate; consumers
depend on `retroglyph-core` and whichever backend/helper crates they need.

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

## Workspace layout

```text
crates/
  core/            retroglyph-core — no_std-capable foundation, no backend
    src/
      color.rs       Color (Default / ANSI / Indexed / RGB); Color::lerp behind `gem`
      style.rs       Style (depends on: color)
      tile.rs        Tile — glyph + Style + sub-cell offsets (depends on: style)
      grid.rs        Grid, Pos, Rect, Size, Grid::from_charmap (depends on: tile, style; uses grixy)
      event.rs       Event, KeyEvent, MouseEvent (depends on: grid::Pos)
      text.rs        Line, Span (depends on: style)
      layout.rs      TextLayout, HAlign, VAlign (depends on: text, terminal; feature = "egc")
      camera.rs      Camera — viewport that follows a target over a larger world
      animate.rs     Tween/easing helpers for frame-based animation
      frame_clock.rs FrameClock — fixed-timestep driver
      app.rs         App/Flow/Frame game-loop contract, run_blocking
      terminal.rs    Terminal<B> — stateful drawing API, double buffering (depends on: all above)
      backend/
        mod.rs       Backend trait (depends on: event, grid, tile)
        headless.rs  In-memory backend for testing (no external deps)
  terminal/        retroglyph-terminal — shared ANSI/SGR cell-diff renderer for terminal backends
  crossterm/       retroglyph-crossterm — real TTY backend (depends on terminal + core)
  terminal-wasm/   retroglyph-terminal-wasm — browser terminal backend (pushed events, pulled ANSI)
  software/        retroglyph-software — winit + softbuffer pixel backend
    src/
      config.rs      SoftwareBackend, SoftwareBackendBuilder
      bitmap_font.rs BitmapFont, embedded VGA 8x16 (feature = "default-font")
      tileset.rs     Codepage, TilesetBuilder (feature = "tilesets")
      sprite_cache.rs SpriteCache, alpha blending (feature = "tilesets")
      surface_native.rs / surface_wasm.rs  present targets per platform
  window/          retroglyph-window — shared winit windowing layer for windowed backends
  widgets/         retroglyph-widgets — immediate-mode drawing helpers (optional)
    src/            layout (split_h/v, Constraint, Flex), interact (HitTester, FocusRing,
                    Interaction, Shortcuts, Density), widget (Table, Gauge, Sparkline, Panel,
                    Modal, Scrollbar, Log, Meter, Paragraph, ...), state (ListState, ScrollState),
                    style (BoxStyle), theme (Theme), block (join_h/join_v)
examples/          retroglyph-examples — Example trait, backend launcher, WASM FFI macros,
                   snapshot-test harness, and the runnable demos under examples/examples/
tools/cargo-bin/   cargo-bin shim (workspace dev tooling)
```

The core internal dependency flow is `color -> style -> tile -> grid`, with `event` depending only
on `grid::Pos`, and `terminal` pulling everything together. Backend crates depend on `core` (and
`terminal` for the ANSI family, `window` for the windowed family) but not on each other.
`retroglyph-widgets` depends only on `retroglyph-core`, so games that draw manually never pull it
in.

## Feature flags

Features are per-crate now, not a single flat set. The important ones:

| Crate                                                  | Flag              | Description                                                           |
| ------------------------------------------------------ | ----------------- | --------------------------------------------------------------------- |
| `retroglyph-core`                                      | `std` (default)   | Enable std-dependent code; disable for `no_std` (`alloc` always).     |
| `retroglyph-core`                                      | `egc` (default)   | Extended grapheme cluster support via `unicode-segmentation`.         |
| `retroglyph-core`                                      | `gem` (default)   | `gem`-backed color math (`Color::lerp`, color ramps).                 |
| `retroglyph-software`                                  | `default-font`    | Embedded VGA 8×16 bitmap font.                                        |
| `retroglyph-software`                                  | `tilesets`        | PNG sprite sheet tilesets + alpha blending (`image` + `alpha-blend`). |
| `retroglyph-window`                                    | `winit` (default) | winit windowing layer.                                                |
| `retroglyph-widgets`                                   | `egc`             | Forwards to core `egc`; needed for `Paragraph` word-wrap.             |
| `retroglyph-crossterm` / `-terminal-wasm` / `-widgets` | `egc`             | Forward grapheme support to core.                                     |

## Testing

Unit tests live alongside their modules in each crate (core and widgets carry the bulk). Pixel-level
software regressions live in `crates/software/src/snapshots/`. The examples crate doubles as the
cross-backend integration + visual-regression harness.

### Snapshot tests (insta)

`Headless::format_view()` renders the grid to text (spaces → `·`). Use it with
`insta::assert_snapshot!` for layout assertions. Snapshot files are committed next to their crate
(`crates/*/src/snapshots/`, `examples/tests/snapshots/`).

```sh
cargo insta test    # run and open review UI
cargo insta accept  # accept pending snapshots
```

### Example-driven snapshots (examples crate)

`examples/tests/support/` drives each `Example` impl through three snapshot types from one source of
truth: headless text (insta), software PNG (pixel buffer), and crossterm SVG (real PTY capture
parsed via the `vt100` crate). Every example under `examples/examples/*.rs` is also auto-built to
three WASM variants (headless / xterm.js terminal / software canvas) and deployed to the docs
gallery by `.github/workflows/docs.yml`, so each example carries real CI cost.

```sh
cargo test -p retroglyph-examples --all-features
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
