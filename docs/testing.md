# Testing

How retroglyph is tested, and where each kind of test lives. Commands live in `AGENTS.md`'s
Correctness gate section, the single source of truth for the command list; this file links to them
rather than restating them, so the two can't drift apart.

## Test runner (nextest) and doctests

`cargo bin cargo-nextest run --workspace --all-features` is the actual test runner behind `just
test`/`just check`/`just test-ci`, not plain `cargo test`. It gives every `[[test]]` binary (each
file under a crate's `tests/`, plus each of the examples crate's per-example `svg_snapshot` files)
its own process, run in parallel, rather than `cargo test`'s one-binary-at-a-time model. That's why
those PTY-spawning tests can race each other over the shared `target/pty-examples/` build dir if it
isn't pre-built first (retroglyph#976; see `build-pty-examples` below). `.config/nextest.toml` sets
`slow-timeout = { period = "20s", terminate-after = 2 }`, workspace-wide.

Nextest can't run doctests at all ([a stated upstream limitation](https://nexte.st/docs/running/)),
so `just test` runs `cargo test --workspace --all-features --doc` as its own separate line. Every
crate's `README.md` is included into `src/lib.rs` via `#[doc = include_str!("../README.md")]`, so
those README code samples are themselves doctests: they compile (and, where written as runnable
examples rather than `text`/`ignore` blocks, execute) as part of the gate, not just as documentation.

## Default-features and no_std builds

Every downstream crate pins `retroglyph-core = { default-features = false }`, so a plain `cargo test
-p <crate>` (what the README's quick start runs, and what any fresh clone would run) builds with the
`egc` feature off, the opposite of every `--all-features` run above it. Because `--all-features`
never exercises that path, CI stayed green while the plain command failed on a clean clone
(retroglyph#757). `just test-default-features` is the recipe that exists to cover it, and it's a
separate line in `just test`/`just check` rather than folded into `--workspace`, because a
`--workspace` build unifies feature resolution across every member compiled together, which would
silently turn `egc` back on everywhere via `examples/Cargo.toml`'s own dependency on `retroglyph-core`.

`retroglyph-core` and `retroglyph-widgets` are mandatory-float-backend crates (see their crate-level
`compile_error!`s), so each also gets a dedicated `--no-default-features --features libm` line: the
`no_std` build, exercising the `libm`-backed float dispatch path instead of `std`'s
(retroglyph#843, #882, #886). The same three lines are mirrored in `compile` (plain `cargo check`)
and `clippy`, so a `no_std`-only lint or type error can't land on `main` invisibly either
(retroglyph#887, #903).

## Feature-matrix builds (cargo-hack)

`compile`'s `cargo bin cargo-hack check --each-feature` lines build every feature flag in isolation,
one at a time, rather than only at zero or all features. The all-or-nothing builds above can stay
green for a break in a single feature and only surface it once another PR happens to combine that
feature with something else; `--each-feature` catches it directly (retroglyph#894).
`retroglyph-core` and `retroglyph-widgets` get their own `--features libm` runs (a float backend is
mandatory for both, so every one of their features would otherwise fail alone), the rest of the
workspace runs with no extra flags, and none of the three lines needs an `--exclude-features` list:
every feature is expected to build standalone (retroglyph#886).

## `just check-targets`

`just check`/`just compile` only ever compile the host target, but `retroglyph-gl` gates three test
modules to other targets: `headless.rs` to `cfg(test, target_os = "linux")`, `webgl_smoke.rs` and
`webgl_recovery.rs` to wasm32 (both documented below). All three drive the `Output` trait, so a
change to the backend draw contract can be locally green under `just check` on a non-Linux,
non-wasm host and still break every one of them; that's how retroglyph#552 stayed green locally
while failing five CI jobs. `just check-targets` cross-compiles those modules (`--target
x86_64-unknown-linux-gnu` and `--target wasm32-unknown-unknown -p retroglyph-gl`) without needing
the actual OS/browser to run them; it's not part of `just check` itself (see that recipe's own
comment for why), so run it by hand whenever touching `Output`, `DrawCell`, or a backend impl.

## Unit tests

Unit tests live alongside their modules in each crate (`retroglyph-core` and `retroglyph-widgets`
carry the bulk of them). Pixel-level software-backend regressions live in
`crates/software/src/snapshots/`.

## Headless GPU render tests (retroglyph-gl, Linux)

`crates/gl/src/headless.rs` runs the real native GL pipeline (shader compile/link, atlas upload,
instanced draw) and reads the result back with `glReadPixels`, so the GPU path is actually exercised
instead of only its CPU-side units (atlas byte layout, shader-string generation). It creates an EGL
_surfaceless_ context off the windowed path (an EGL display built from an EGL device via glutin's
`api::egl`, made current with no surface) and renders into an offscreen framebuffer; the windowed
`GlContext` needs a real window handle and can't run in CI.

The module is `cfg(test, target_os = "linux")`: the EGL device platform is the portable CI-able
headless path (macOS's CGL pbuffer is deprecated, Windows differs), and render correctness only
needs asserting on one platform. It asserts two ways, both resilient to driver-version pixel drift:
property checks (a full-block cell is entirely its foreground, a blank cell entirely its background,
a glyph matches the font's own coverage bits) and pixel-for-pixel parity against the
`retroglyph-software` CPU rasterizer, which shares the same `retroglyph-window` font. Parity is
checked for both a single flattened frame and a full multi-layer frame (`draw_layers`), so the GPU's
back-to-front layer compositing (issue #368) is verified to match the software backend's per-pixel
occlusion: including the opaque-space-erases-lower-glyph and inherited-background cases.

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
foreground (an atlas that fails to upload, the glow 0.16 `texImage3D` bug, renders it as the
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
while inheriting its background): the runnable local counterpart to the Linux-only multi-layer
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
then renders the full-block cell correctly again after the restore: which only holds if the
invalidated program/atlas/buffers were rebuilt on the live context. It runs under the same
`just test-wasm-gl` / `test-wasm-gl` CI job (both tests are in the crate, so `wasm-pack test` runs
them together). The `WEBGL_lose_context` extension is implemented by the browser, not the GL driver,
so it works under SwiftShader.

## WASM FFI tests (retroglyph-terminal-wasm)

`crates/terminal-wasm/tests/wasm_ffi.rs` is the only place the `#[wasm_bindgen]`-exported
`wasm_terminal_*` FFI surface actually runs: it's `cfg(target_arch = "wasm32")`, so a host-target
`cargo test` never compiles it in the first place. `just test-wasm` runs it via `wasm-pack test
--node crates/terminal-wasm`; `--node` rather than `--chrome`/`--firefox` because this FFI has no
DOM/xterm.js dependency to exercise, so Node is enough and avoids needing a browser + webdriver in
CI. It has its own gated `test-wasm` CI job (guarded by a `dorny/paths-filter` check on
`crates/terminal-wasm` and its dependencies) in the required-checks set, separate from
`test-wasm-gl` above.

## Snapshot tests (insta)

`Headless::format_view()` renders a grid to text (spaces become `·`). Combined with
`insta::assert_snapshot!`, this is the primary tool for layout assertions: write the drawing code,
snapshot the headless render, and diff future changes against the committed baseline instead of
hand-writing character-grid assertions.

Snapshot files are committed next to their crate (`crates/*/src/snapshots/`,
`examples/tests/snapshots/`).

```sh
just insta  # bless every changed snapshot; no review step of its own
```

`just insta` runs `cargo test` with `INSTA_UPDATE=always`, not the separate `cargo-insta` CLI (a
tool this repo's conventions don't otherwise require), so it accepts unconditionally -- review the
diff (`jj diff`/`git diff`) before committing. Install `cargo-insta` by hand if you want its
interactive review UI instead.

## Driving `Headless` with synthetic events

`Headless` doesn't just render; it also accepts input, via `Input::push_event` /
`Headless::push_event`. That makes it possible to test a whole update-draw cycle (inject a key or
mouse event, drain it through your app's event handling, then snapshot the resulting grid) without a
real terminal, window, or PTY. This is the same technique used throughout this crate's own unit and
integration tests (see `crates/core/src/terminal.rs`, `crates/core/src/app.rs`) and in
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
    // handle_input(event): move the `@`, etc.
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

## Driving an `App` with `TestHarness`

The manual `push_event`/`drain_events`/`present` sequence above is what `TestHarness`
(`retroglyph_core::testing`, behind the `testing` feature) wraps for apps implementing the `App`
trait, so tests stop rewriting that loop by hand:

```rust
use retroglyph_core::testing::TestHarness;
use retroglyph_core::event::KeyCode;
use retroglyph_core::{App, Backend, Flow, Frame, Style, Terminal};

struct MyApp;

impl<B: Backend> App<B> for MyApp {
    fn update(&mut self, term: &mut Terminal<B>, _frame: &Frame) -> Flow {
        for event in term.drain_events() {
            // handle_input(event): move a cursor, etc.
            let _ = event;
        }
        term.surface().put((0, 0), '@', Style::default());
        Flow::Continue
    }
}

let mut harness = TestHarness::new(10, 3);
let mut app = MyApp;

harness.key(KeyCode::Right);
harness.run(&mut app); // steps until the queue drains, presenting each frame automatically

let view = harness.view();
assert!(view.contains('@'));
```

`click`/`key`/`mouse_move` only _queue_ events; `run`/`settle` are what actually step frames until
the queue drains (or `Flow::Exit`). A single `step()` after queuing input is not enough for a press
and release to resolve: see `TestHarness`'s own "two-frame rule" doc section for why (the same
single-frame-stale hit-testing snapshot that `retroglyph-widgets`' `Interaction` documents). Prefer
the manual technique above only for tests that don't have an `App` to drive (e.g. testing
`Interaction` or a widget directly, outside the frame-loop contract).

## Example-driven snapshots (examples crate)

`examples/tests/support/` drives every `Example` implementation through three snapshot types from
one source of truth:

- **Headless text** (insta): the same `format_view()` mechanism as unit tests, run against the
  example's actual `update()` logic.
- **Software PNG**: a pixel buffer capture of the software backend's rendered output.
- **Crossterm SVG**: a real PTY capture, parsed via the `vt100` crate, verifying the ANSI/SGR output
  an actual terminal would receive.

`20_overworld` is the one exception: its render isn't byte-stable, so its `svg_snapshot` test
asserts on substrings instead of pinning the SVG with `insta`, and writes the SVG to
`CARGO_TARGET_TMPDIR` via `support::write_scratch_file` rather than a tracked snapshot file. Reach
for `write_scratch_file` the same way for any future example whose output can't be pinned
byte-for-byte.

`support::capture_pty` spawns those crossterm binaries with `RG_FPS=0`, because the shared example
driver draws its FPS overlay by default and a live frame rate is not reproducible. The one place
that deliberately doesn't is `examples/tests/fps_overlay.rs`, which pins the default itself (the
overlay was originally behind an opt-in Cargo feature, so nothing that ran an example the documented
way ever saw it) and drives its `` ` `` toggle through the PTY in both directions.

Two examples need more than `RG_FPS=0`. `06_layers` and `08_animation` animate to a parked end state
and use _that_ as their ready marker, because an animation that loops forever never settles into a
single frame a snapshot can pin. Waiting on those markers is therefore waiting on real elapsed time
(4.7s and 2.3s respectively), and `FrameClock::advance` caps catch-up at five steps: whenever a
loaded test runner deschedules the child for longer than that cap, the lost wall time is animation
time it never gets back and the capture stretches without bound. Measured on a 12-core machine,
freezing the child for 1.5s five times pushed `06_layers` from 4.7s to 9.9s, past the harness's old
fixed 10s budget; that is retroglyph#544. `support::capture_pty_animated` runs those two with
`RG_TIME_SCALE` (see the README) so the marker still means "the animation has settled" but no longer
has a wall-clock floor under it, which takes the same stall case to 1.7s.

The ready-marker wait itself is a liveness check rather than a total budget: it fails when the child
produces no new output for `READY_IDLE_TIMEOUT`, has closed the PTY, or blows the
`READY_HARD_TIMEOUT` backstop (kept under nextest's own 40s kill so the failure names the marker it
was waiting for).

The crossterm binary each `svg_snapshot` test spawns lives in its own `--target-dir`
(`target/pty-examples/`, see `support::build_crossterm_example`), separate from the workspace's
normal `target/`. `cargo test --workspace --all-features` builds every example with the `software`
feature (unusable in a PTY) before any test runs, so building the crossterm-only variant back into
the same output path would force a relink (and, on macOS, a real code-signature validation cost of
roughly a second or two) on every single test run. The isolated target dir keeps that binary
byte-identical (and already validated) across runs instead. It's built exactly once, by the
`build-pty-examples` Justfile recipe, before any test process starts;
`support::build_crossterm_example` only asserts the binary is there rather than building it itself,
so concurrent nextest processes never race each other over that shared path (retroglyph#976).

Every example under `examples/examples/*.rs` is also auto-built to four WASM variants (headless /
xterm.js terminal / software canvas / WebGL) and deployed to the docs gallery by
`.github/workflows/docs.yml` on every push, so each example carries real, ongoing CI cost, not just
a one-time snapshot.

```sh
just build-pty-examples  # pre-builds the crossterm example binaries; skip this and svg_snapshot fails
cargo test -p retroglyph-examples --all-features
```

See `examples/AGENTS.md` for the per-example validation checklist a new example must satisfy before
it's considered complete (all three snapshot types, all four WASM variants, graceful backend
degradation, etc.).

## Benchmarks

Seven crates (`core`, `crossterm`, `software`, `terminal`, `terminal-wasm`, `widgets`, `window`)
have their own `benches/`, each a `harness = false` criterion suite. `.github/workflows/bench.yml`
tracks results to [Bencher](https://bencher.dev) on every push to `main` and checks a labeled PR
(`benchmark` label) for a statistically significant regression against that history. See
`CONTRIBUTING.md`'s Benchmarking section for `just bench`/`just bench-compare` and the full CI
mechanics; this file only tracks that benchmarks exist as a category, since they measure
performance rather than correctness.

## Coverage

`just coverage` (opens an HTML report) and `just coverage-ci` (writes `lcov.info`) both run
`cargo llvm-cov --workspace --lib --all-features`, `--lib` only: integration tests are excluded
because `examples/tests/*.rs`'s PTY tests shell out to `cargo build --example`, which lands in the
default `target/` dir rather than llvm-cov's own instrumented `--target-dir`, so those binaries
wouldn't be found under coverage anyway; unit tests are what's measured. The `coverage` CI job runs
`coverage-ci` and uploads `lcov.info` to Codecov, scoped per crate via the `flags:`/`paths:` mapping
in `codecov.yml` so one combined upload still yields per-crate numbers. It's in the required-checks
set alongside `test`/`test-wasm`/`test-wasm-gl`/`gl-headless`.

Separately, the `test` CI job (which runs `just test-ci`) writes JUnit XML via the `ci` nextest
profile (`.config/nextest.toml`, to `target/nextest/ci/junit.xml`). `tools/split-junit-flags.py`
splits that single workspace-wide file into one file per crate flag, since Codecov Test Analytics'
`flags` filter is set per upload, not per `<testsuite>`; each split file is then uploaded with
`codecov/codecov-action`'s `report_type: test_results`. That's test pass/fail/flake history,
distinct from the `lcov.info` line-coverage upload above.
