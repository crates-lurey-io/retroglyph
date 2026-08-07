# Choose a backend

retroglyph ships six backend crates. All of them implement the same
[`Backend` trait](https://docs.rs/retroglyph-core/latest/retroglyph_core/backend/trait.Backend.html),
so the game code you write against one runs unchanged on any other. This page is about picking the
right one to start with, not about how any of them work internally; each crate's own README covers
that.

## The short answer

- Building a CLI tool or a TUI that lives in a real terminal: **`crossterm`**.
- Building something with sprites, smooth animation, or a resizable window, and it needs to run
  natively: **`software`** first, `gl` or `wgpu` only once you've measured a reason to move.
- Building for the browser: see [Run in a browser](./run-in-a-browser.md); the short version is
  `terminal-wasm` for a text UI, `software`/`gl`/`wgpu` for a canvas.
- Writing tests: **`Headless`**, from `retroglyph-core`, no feature flag needed.

## The decision

**Does it need to run inside an actual terminal emulator (SSH session, `tmux`, a user's shell)?**
Use [`crossterm`](../../crates/retroglyph_crossterm/index.html). It's the only backend that reads
and writes a real terminal: raw mode, ANSI escapes, the terminal's own resize events. `software`,
`gl`, and `wgpu` all open their own window and have nothing to do with a terminal emulator.

**Otherwise, does it need pixel-level rendering: sprites, sub-cell offsets, custom fonts, smooth
scrolling?** All three windowed backends (`software`, `gl`, `wgpu`) support this equally; a cell
backend (`crossterm`, `terminal-wasm`) can only ever draw one glyph per cell. Pick one of the three:

- **`software`**: CPU rasterization via [`softbuffer`](https://crates.io/crates/softbuffer). No GPU
  driver, no shader compilation, works everywhere `winit` opens a window (including wasm's
  `<canvas>`). This is the right default until you have a concrete reason to leave it: most
  pseudographic games redraw at most a few thousand cells a frame, well within what CPU blitting
  handles at 60fps.
- **`gl`**: OpenGL 3.3 / WebGL2 via [`glow`](https://crates.io/crates/glow). Instanced-quad
  rendering on the GPU. Reach for this once profiling shows `software`'s CPU cost is the bottleneck,
  or you specifically need OpenGL/WebGL2 for platform reasons (e.g. targeting hardware or browsers
  where `wgpu`'s backend selection is unreliable).
- **`wgpu`**: Vulkan/Metal/D3D12 via [`wgpu`](https://crates.io/crates/wgpu). The modern-native
  equivalent of `gl`; prefer it over `gl` for a new native-only project that wants GPU rendering and
  doesn't need WebGL2 specifically.

**Neither of those, and it's a test?** Use
[`Headless`](https://docs.rs/retroglyph-core/latest/retroglyph_core/backend/struct.Headless.html),
part of `retroglyph-core` itself with no extra crate to add. See [Test a game](./test-a-game.md).

## Mixing backends in one binary

Nothing stops one binary from picking a backend at runtime (a `--backend` CLI flag, or falling back
from `crossterm` to `software` when stdout isn't a TTY): `Terminal<B>` is generic over `B: Backend`,
so a small enum-dispatch wrapper or two separate code paths behind an early branch both work.
`examples/src/launch.rs` in this workspace picks a backend from Cargo features this way, as a
concrete reference.

## See also

- [Write a backend](./write-a-backend.md), if none of the six fit.
- Each crate's own README (`crates/crossterm`, `crates/software`, `crates/gl`, `crates/wgpu`,
  `crates/terminal-wasm`) for that backend's specific setup and feature flags.
