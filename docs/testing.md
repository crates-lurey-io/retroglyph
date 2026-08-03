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

## Cross-backend conformance

`retroglyph_core::testing::conformance` (behind the `testing` feature) is a third kind of test,
orthogonal to the render tests above and to `TestHarness` below: it drives a raw backend directly
through the `Output`/`Cursor`/`Input` trait contracts (retroglyph#763), independent of `Terminal`
and `App`. Where the render tests above check a single backend's pixels are correct and
`TestHarness` drives a whole `App` update loop, the four `assert_*_contract` functions check that
the backends in this workspace agree on obligations no lone `impl` block states:

- `assert_output_contract`: `clear()`/`resize()` reset internal shadow/diff state (a redraw after
  either must repaint, not silently skip because it matches a stale copy), and an out-of-range
  `DrawCell` position is silently dropped rather than panicking or reaching the display.
- `assert_cursor_contract`: an external `Cursor::set_cursor_position` call keeps a backend's
  internally tracked cursor in sync with where the cursor actually is, the same as an ordinary draw
  does.
- `assert_cursor_style_contract`: each `CursorStyle` variant has its own distinct, observable effect
  (no two variants collapsing onto the same DECSCUSR code via a `match` fallthrough).
- `assert_input_contract`: a burst of consecutive `Mouse(Moved)` events coalesces to the latest one,
  without swallowing a non-`Moved` event queued between two bursts.

These are four separate entry points rather than one `B: Backend` bound because not every backend
implements every facet: `GlRenderer` implements neither `Input` nor `Cursor` (a GPU/pixel surface
has no text cursor and never receives external input), so a bound requiring all of them could never
be satisfied by every backend that only wants the `Output` contract. Each backend crate opts into
exactly the facets it implements.

### The `Observable` hook

None of `Output`/`Cursor` has a shared way to read back "what would actually appear": a terminal
backend has emitted bytes, a pixel backend has a framebuffer, `Headless` has a `Grid`.
`Observable::snapshot(&mut self) -> u64` is the one method a backend implements to bridge that gap,
and every assertion above only ever compares two calls to it for equality.

That equality only means what it should if `snapshot` returns a digest of **what changed since the
previous call**, not the backend's whole history or whole current state. Get this wrong (e.g. hash a
terminal backend's entire emitted byte log) and two independently-built action sequences of
different lengths can never compare equal even when both are correct, so every assertion fails for a
reason that has nothing to do with the obligation under test.

Every backend that implements `Observable` in this workspace does so via a small test-only wrapper
around the real backend, rather than on the production type itself, because the "since last call"
bookkeeping has no other reason to exist outside a test:

- `HeadlessObserver` (`crates/core/src/testing/conformance.rs`) hashes the `(index, char)` pairs
  that differ from the previous `format_view()`.
- `SoftwareObserver` (`crates/software/src/lib.rs`) hashes the pixel indices that differ from the
  previous framebuffer.
- `CrosstermObserver` (`crates/crossterm/src/lib.rs`) hashes the bytes appended to its `Vec<u8>`
  writer since the previous call.
- `GlObserver` (`crates/gl/src/lib.rs`) hashes the per-cell instance data that differs from the
  previous frame's CPU-side upload buffer (`GlRenderer` has no CPU-readable framebuffer without a
  real GL context).

`retroglyph-terminal-wasm` is the exception: `TerminalWasm`'s own observable state is already an
append-only ANSI byte buffer drained by `take_output()`, so it implements `Observable` directly
rather than through a wrapper.

`fnv1a` (`retroglyph_core::testing::conformance::fnv1a`) is the non-cryptographic FNV-1a digest
every `Observable` impl above uses; it exists because `core::hash::Hasher` has no portable digest
guarantee and is unavailable at all under `no_std`.

### Coverage

Coverage is uneven: a backend only gets a facet's regressions caught for free once it's wired into a
test. Some gaps are load-bearing rather than accidental (see retroglyph#997, retroglyph#999).

| Backend                    | `Output` | `Cursor`                                                                          | `CursorStyle` | `Input`                                                  |
| -------------------------- | -------- | --------------------------------------------------------------------------------- | ------------- | -------------------------------------------------------- |
| `Headless`                 | checked  | checked                                                                           | not wired     | checked                                                  |
| `retroglyph-software`      | checked  | checked (trivially: no hardware cursor)                                           | not wired     | n/a (no `Input` impl)                                    |
| `retroglyph-crossterm`     | checked  | ignored (retroglyph#713: `set_cursor_position` doesn't resync the tracked cursor) | checked       | not wired (reads real terminal input, not synthesizable) |
| `retroglyph-gl`            | checked  | n/a (implements neither `Input` nor `Cursor`)                                     | n/a           | n/a                                                      |
| `retroglyph-terminal-wasm` | checked  | ignored (retroglyph#713)                                                          | checked       | checked                                                  |

A backend crate wires a facet in by writing an `Observable` wrapper (or implementing it directly,
per `TerminalWasm`) and calling the matching `assert_*_contract` function from a `#[test]`.

## Snapshot tests (insta)

`Headless::format_view()` renders a grid to text (spaces become `·`); the trailing half of a wide
glyph (`TileFlags::WIDE_CHAR_SPACER`) renders as a real space instead, which is what distinguishes
it from `·` in a snapshot containing wide characters (see
`test_format_view_renders_span_fallback_glyphs`). Combined with `insta::assert_snapshot!`, this is
the primary tool for layout assertions: write the drawing code, snapshot the headless render, and
diff future changes against the committed baseline instead of hand-writing character-grid
assertions.

`Headless::format_styled()` renders the same grid with each cell's colors emitted as SGR (ANSI)
escape sequences, so insta's terminal diff shows color: a color regression `format_view` can't see
(two styles that share a glyph) shows up as a snapshot diff here. Reach for it instead of
`format_view` whenever a test cares about `Style`, not just glyph placement.

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

`support::write_scratch_file` is the general escape hatch: output lives under `CARGO_TARGET_TMPDIR`
(under `target/`, so nothing tracked ever goes stale or dirty) for visual review rather than being
insta-pinned. `examples/tests/fps_overlay.rs` also relies on it for its compact/full overlay SVGs,
for the same reason as `20_overworld`: a live frame counter in the rendered output isn't byte-stable
across runs. Print the returned path from the test so a reviewer can still open the artifact after a
run; compare against `write_snapshot_file`, which writes a _tracked_ file and is only for output
some `insta` assertion in the same test already pinned.

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

The crossterm binary each `svg_snapshot` test spawns is built with its own `--target-dir`
(`target/pty-examples/`, see `support::build_crossterm_example`), separate from the workspace's
normal `target/`. `cargo test --workspace --all-features` builds every example with the `software`
feature (unusable in a PTY) before any test runs, so building the crossterm-only variant back into
the same output path would force a relink (and, on macOS, a real code-signature validation cost of
roughly a second or two) on every single test run. The isolated target dir keeps that binary
byte-identical (and already validated) across runs instead.

Every example under `examples/examples/*.rs` is also auto-built to four WASM variants (headless /
xterm.js terminal / software canvas / WebGL) and deployed to the docs gallery by
`.github/workflows/docs.yml` on every push, so each example carries real, ongoing CI cost, not just
a one-time snapshot.

```sh
cargo test -p retroglyph-examples --all-features
```

See `examples/AGENTS.md` for the per-example validation checklist a new example must satisfy before
it's considered complete (all three snapshot types, all four WASM variants, graceful backend
degradation, etc.).
