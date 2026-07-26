# Testing

How retroglyph is tested, and where each kind of test lives. For the exact commands to run, see
`AGENTS.md`'s Correctness gate section, which stays the single source of truth for the command list.

## Unit tests

Unit tests live alongside their modules in each crate (`retroglyph-core` and `retroglyph-widgets`
carry the bulk of them). Pixel-level software-backend regressions live in
`crates/software/src/snapshots/`.

## Headless GPU render tests (retroglyph-gl, Linux)

`crates/gl/src/headless.rs` runs the real native GL pipeline (shader compile/link, atlas upload,
instanced draw) and reads the result back with `glReadPixels`, so the GPU path is actually exercised
instead of only its CPU-side units (atlas byte layout, shader-string generation). It creates an EGL
_surfaceless_ context off the windowed path -- an EGL display built from an EGL device via glutin's
`api::egl`, made current with no surface -- and renders into an offscreen framebuffer; the windowed
`GlContext` needs a real window handle and can't run in CI.

The module is `cfg(test, target_os = "linux")`: the EGL device platform is the portable CI-able
headless path (macOS's CGL pbuffer is deprecated, Windows differs), and render correctness only
needs asserting on one platform. It asserts two ways, both robust against driver-version pixel
drift: property checks (a full-block cell is entirely its foreground, a blank cell entirely its
background, a glyph matches the font's own coverage bits) and pixel-for-pixel parity against the
`retroglyph-software` CPU rasterizer, which shares the same `retroglyph-window` font. Parity is
checked for both a single flattened frame and a full multi-layer frame (`draw_layers`), so the GPU's
back-to-front layer compositing (issue #368) is verified to match the software backend's per-pixel
occlusion -- including the opaque-space-erases-lower-glyph and inherited-background cases.

The render only runs when `RETROGLYPH_REQUIRE_GL` is set; otherwise the tests skip. That keeps the
ordinary `test`/`coverage` jobs from depending on whatever GL a runner happens to expose (GitHub's
stock `ubuntu-latest` ships llvmpipe, so an unconditional "run if a context exists" would assert
against an uncontrolled driver). The dedicated `gl-headless` job (`.github/workflows/ci.yml`) sets
the flag and forces Mesa's llvmpipe software rasterizer (`LIBGL_ALWAYS_SOFTWARE=1`,
`GALLIUM_DRIVER=llvmpipe`) after installing the Mesa EGL/GL packages, so rendering runs against one
known-good software stack; with the flag set, a missing/broken context is a hard failure instead of
a silent skip. To run them locally, set `RETROGLYPH_REQUIRE_GL=1` on a Linux box with a headless
GL/EGL stack.

### WebGL2 browser render + recovery tests (issues #370, #373)

`crates/gl/src/webgl_smoke.rs` is the browser sibling of `headless.rs`: a `wasm-bindgen-test` that
builds a WebGL2 context from a `<canvas>`, runs the same `GlRenderer::build_resources` + instanced
draw the windowed path uses, reads the pixels back, and asserts a full-block cell is entirely its
foreground (an atlas that fails to upload -- the glow 0.16 `texImage3D` bug -- renders it as the
background, failing the test). It runs in real headless Chrome via `just test-wasm-gl`
(`wasm-pack test --headless --chrome`); CI runners have no GPU, so `crates/gl/webdriver.json`
launches Chrome with `--enable-unsafe-swiftshader` for a software WebGL2 stack. The dedicated
`test-wasm-gl` job (`.github/workflows/ci.yml`) installs a matched Chrome + chromedriver pair and
runs it; the `compile-wasm-gl` job stays as the fast build-only check. Locally, a Chrome that lags
the latest stable needs a matching chromedriver (a major-version skew makes the WebDriver session
fail to start).

`crates/gl/src/webgl_smoke.rs` also carries `composites_two_layers_back_to_front`, which drives the
GPU compositing path (issue #368) in the browser and asserts the three occlusion cases directly (a
transparent empty overlay, an opaque occluding glyph, and an opaque space that erases the base glyph
while inheriting its background) -- the runnable local counterpart to the Linux-only multi-layer
software-parity test above.

The glyph atlas grid-packs slots into `TEXTURE_2D_ARRAY` layers (issue #367's grid-packing half,
lifting the 256-layer cap). The bundled Unscii 16 font is 256 glyphs, so it stays in one layer; the
slot -> `(layer, column, row)` addressing that would span layers is covered by `src/atlas.rs` unit
tests, and the within-layer sub-rect sampling is exercised by every bitmap render test above.

Sprite/tileset rendering (issue #366, `tilesets` feature) has a matching pair:
`sprite_cells_render_their_tileset_colors` in both `src/headless.rs` (Linux llvmpipe) and
`src/webgl_smoke.rs` (browser SwiftShader) builds a renderer from a tiny in-memory 2-tile PNG (red
and green tiles), draws them through `draw_layers`, and asserts each cell is its tile's color --
exercising the RGBA sprite atlas upload, the second (source-over) sprite pass, and the per-cell
glyph -> sprite dispatch. The Linux and browser gl jobs both build with `--features tilesets`. Note
the browser harness asserts `glGetError` is clear after the draw passes, which is what first caught
the signed/unsigned vertex-attribute mismatch that SwiftShader rejects.

`crates/gl/src/webgl_recovery.rs` is the companion context-loss test (issue #373). It drives the
real windowed path (`Presenter::init_surface` then `present`), forces a lost/restored cycle with the
`WEBGL_lose_context` extension, and asserts `present()` reports the recoverable error while lost and
then renders the full-block cell correctly again after the restore -- which only holds if the
invalidated program/atlas/buffers were rebuilt on the live context. It runs under the same
`just test-wasm-gl` / `test-wasm-gl` CI job (both tests are in the crate, so `wasm-pack test` runs
them together). The `WEBGL_lose_context` extension is implemented by the browser, not the GL driver,
so it works under SwiftShader.

## Snapshot tests (insta)

`Headless::format_view()` renders a grid to text (spaces become `·`). Combined with
`insta::assert_snapshot!`, this is the primary tool for layout assertions: write the drawing code,
snapshot the headless render, and diff future changes against the committed baseline instead of
hand-writing character-grid assertions.

Snapshot files are committed next to their crate (`crates/*/src/snapshots/`,
`examples/tests/snapshots/`).

```sh
cargo insta test    # run and open review UI
cargo insta accept  # accept pending snapshots
```

## Driving `Headless` with synthetic events

`Headless` doesn't just render; it also accepts input, via `Input::push_event` /
`Headless::push_event`. That makes it possible to test a whole update-draw cycle -- inject a key or
mouse event, drain it through your app's event handling, then snapshot the resulting grid -- without
a real terminal, window, or PTY. This is the same technique used throughout this crate's own unit
and integration tests (see `crates/core/src/terminal.rs`, `crates/core/src/app.rs`) and in
`crates/core/examples/headless.rs`.

```rust
use retroglyph_core::{Terminal, Headless};
use retroglyph_core::event::{Event, KeyCode, KeyEvent, KeyModifiers};

let backend = Headless::new(10, 3);
let mut term = Terminal::new(backend);

// Draw an initial frame.
term.put((1, 1), '@');
term.present().unwrap();

// Inject a synthetic key event, exactly as a real backend would push one from its own input
// source (a crossterm poll, a winit `KeyEvent`, a browser `keydown`, ...).
term.backend_mut().push_event(Event::Key(KeyEvent::new(
    KeyCode::Right,
    KeyModifiers::NONE,
)));

// Drain the queued event(s) and let your app's update logic react to them, then redraw.
for event in term.drain_events() {
    // handle_input(event) -- move the `@`, etc.
    let _ = event;
}
term.put((1, 1), ' ');
term.put((2, 1), '@');
term.present().unwrap();

// Assert on the result. In a real test this is `insta::assert_snapshot!(view, @"...")`
// instead of a manual string compare.
let view = term.backend().format_view();
assert!(view.contains('@'));
```

Run `cargo run -p retroglyph-core --example headless` to see this end to end, including the
before/after `format_view()` output printed to stdout.

## Example-driven snapshots (examples crate)

`examples/tests/support/` drives every `Example` implementation through three snapshot types from
one source of truth:

- **Headless text** (insta) — the same `format_view()` mechanism as unit tests, run against the
  example's actual `update()` logic.
- **Software PNG** — a pixel buffer capture of the software backend's rendered output.
- **Crossterm SVG** — a real PTY capture, parsed via the `vt100` crate, verifying the ANSI/SGR
  output an actual terminal would receive.

`support::capture_pty` spawns those crossterm binaries with `RG_FPS=0`, because the shared example
driver draws its FPS overlay by default and a live frame rate is not reproducible. The one place
that deliberately doesn't is `examples/tests/fps_overlay.rs`, which pins the default itself (the
overlay was originally behind an opt-in Cargo feature, so nothing that ran an example the documented
way ever saw it) and drives its `` ` `` toggle through the PTY in both directions.

The crossterm binary each `svg_snapshot` test spawns is built with its own `--target-dir`
(`target/pty-examples/`, see `support::build_crossterm_example`), separate from the workspace's
normal `target/`. `cargo test --workspace --all-features` builds every example with the `software`
feature (unusable in a PTY) before any test runs, so building the crossterm-only variant back into
the same output path would force a relink -- and, on macOS, a real code-signature validation cost of
roughly a second or two -- on every single test run. The isolated target dir keeps that binary
byte-identical (and already validated) across runs instead.

Every example under `examples/examples/*.rs` is also auto-built to three WASM variants (headless /
xterm.js terminal / software canvas) and deployed to the docs gallery by
`.github/workflows/docs.yml` on every push, so each example carries real, ongoing CI cost, not just
a one-time snapshot.

```sh
cargo test -p retroglyph-examples --all-features
```

See `examples/AGENTS.md` for the per-example validation checklist a new example must satisfy before
it's considered complete (all three snapshot types, all four WASM variants, graceful backend
degradation, etc.).
