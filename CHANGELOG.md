# Changelog

All notable changes to the retroglyph workspace crates are documented here. Versioning is
[lockstep](RELEASING.md#versioning) across all publishable crates; see RELEASING.md for the pre-1.0
SemVer policy this project follows.

Automated per-crate changelog generation (via `git-cliff`, adopted alongside release-plz) takes over
after the 0.1.0 release; this file's initial entry is written by hand.

## [0.1.1+retroglyph-widgets](https://github.com/crates-lurey-io/retroglyph/compare/retroglyph-widgets-v0.1.0...retroglyph-widgets-v0.1.1) - 2026-07-12

**Full Changelog**: https://github.com/crates-lurey-io/retroglyph/compare/retroglyph-widgets-v0.1.0...retroglyph-widgets-v0.1.1



## [0.1.1+retroglyph-terminal-wasm](https://github.com/crates-lurey-io/retroglyph/compare/retroglyph-terminal-wasm-v0.1.0...retroglyph-terminal-wasm-v0.1.1) - 2026-07-12

**Full Changelog**: https://github.com/crates-lurey-io/retroglyph/compare/retroglyph-terminal-wasm-v0.1.0...retroglyph-terminal-wasm-v0.1.1



## [0.1.1+retroglyph-software](https://github.com/crates-lurey-io/retroglyph/compare/retroglyph-software-v0.1.0...retroglyph-software-v0.1.1) - 2026-07-12

**Full Changelog**: https://github.com/crates-lurey-io/retroglyph/compare/retroglyph-software-v0.1.0...retroglyph-software-v0.1.1



## [0.1.1+retroglyph-window](https://github.com/crates-lurey-io/retroglyph/compare/retroglyph-window-v0.1.0...retroglyph-window-v0.1.1) - 2026-07-12

**Full Changelog**: https://github.com/crates-lurey-io/retroglyph/compare/retroglyph-window-v0.1.0...retroglyph-window-v0.1.1



## [0.1.1+retroglyph-crossterm](https://github.com/crates-lurey-io/retroglyph/compare/retroglyph-crossterm-v0.1.0...retroglyph-crossterm-v0.1.1) - 2026-07-12

**Full Changelog**: https://github.com/crates-lurey-io/retroglyph/compare/retroglyph-crossterm-v0.1.0...retroglyph-crossterm-v0.1.1



## [0.1.1+retroglyph-terminal](https://github.com/crates-lurey-io/retroglyph/compare/retroglyph-terminal-v0.1.0...retroglyph-terminal-v0.1.1) - 2026-07-12

**Full Changelog**: https://github.com/crates-lurey-io/retroglyph/compare/retroglyph-terminal-v0.1.0...retroglyph-terminal-v0.1.1



## [0.1.1+retroglyph-core](https://github.com/crates-lurey-io/retroglyph/compare/retroglyph-core-v0.1.0...retroglyph-core-v0.1.1) - 2026-07-12

**Full Changelog**: https://github.com/crates-lurey-io/retroglyph/compare/retroglyph-core-v0.1.0...retroglyph-core-v0.1.1



## 0.1.0 - Initial release

First public release of the retroglyph workspace to crates.io:

- `retroglyph-core` -- `no_std`-capable foundation: grid, tile, style, color, text, terminal, and
  event types, the `Backend` trait, a dependency-free `Headless` test backend, and the
  `App`/`Flow`/`Frame` game-loop contract with a fixed-timestep `FrameClock`.
- `retroglyph-terminal` -- shared ANSI/SGR cell-diff renderer for the terminal-family backends.
- `retroglyph-crossterm` -- real TTY backend via `crossterm`.
- `retroglyph-terminal-wasm` -- browser terminal backend, driven by pushed events and pulled ANSI.
- `retroglyph-window` -- shared `winit` windowing layer for windowed backends.
- `retroglyph-software` -- CPU rasterization backend (`winit` + `softbuffer`), with optional
  embedded bitmap fonts and PNG sprite-sheet tilesets.
- `retroglyph-widgets` -- immediate-mode drawing helpers (panels, gauges, tables, sparklines,
  layout, interaction/hit-testing), depending only on `retroglyph-core`.

14 runnable examples across three tiers (core-capability proofs, `retroglyph-widgets` showcases, and
small games), each verified on all three backend families (headless, crossterm, software) plus three
WASM variants, with committed cross-backend regression snapshots.
