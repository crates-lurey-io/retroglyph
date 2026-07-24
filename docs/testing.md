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
`retroglyph-software` CPU rasterizer, which shares the same `retroglyph-window` font.

The render only runs when `RETROGLYPH_REQUIRE_GL` is set; otherwise the tests skip. That keeps the
ordinary `test`/`coverage` jobs from depending on whatever GL a runner happens to expose (GitHub's
stock `ubuntu-latest` ships llvmpipe, so an unconditional "run if a context exists" would assert
against an uncontrolled driver). The dedicated `gl-headless` job (`.github/workflows/ci.yml`) sets
the flag and forces Mesa's llvmpipe software rasterizer (`LIBGL_ALWAYS_SOFTWARE=1`,
`GALLIUM_DRIVER=llvmpipe`) after installing the Mesa EGL/GL packages, so rendering runs against one
known-good software stack; with the flag set, a missing/broken context is a hard failure instead of
a silent skip. To run them locally, set `RETROGLYPH_REQUIRE_GL=1` on a Linux box with a headless
GL/EGL stack. The WebGL2/browser side is tracked separately (issue #370).

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
term.put(1, 1, '@');
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
term.put(1, 1, ' ');
term.put(2, 1, '@');
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
it's considered complete (all three snapshot types, all three WASM variants, graceful backend
degradation, etc.).
