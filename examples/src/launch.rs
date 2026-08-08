//! The [`Example`] trait and `launch::<E>()` backend dispatch.
//!
//! Replaces the old `rg_run!`/`rg_run_software!` macros with plain generic
//! functions: `launch::<E>()` picks a backend from the crate's enabled
//! Cargo features (`software` > `crossterm` > headless-stdout fallback,
//! mirroring the old macro's priority) and drives an [`Example`] on it.
//! Nothing here is textually generated per example -- every example calls
//! the exact same `launch::<Self>()`.
//!
//! The one thing that *does* still need per-example codegen is the
//! `wasm-bindgen` FFI surface for `wasm-headless`/`wasm-terminal` (and the
//! `#[wasm_bindgen(start)]` shim for `software` on `wasm32`): those need
//! concrete, statically-named exported symbols, which a generic function
//! can't produce. See [`wasm_entry!`](crate::wasm_entry) for that part.

#[cfg(any(
    feature = "crossterm",
    feature = "software",
    feature = "gl",
    feature = "wgpu"
))]
use crate::perf_overlay::PerfOverlayApp;
use retroglyph_core::app::Frame;
#[cfg(any(
    feature = "crossterm",
    feature = "software",
    feature = "gl",
    feature = "wgpu"
))]
use retroglyph_core::app::{App, Flow};
use retroglyph_core::backend::Backend;
#[cfg(feature = "crossterm")]
use retroglyph_core::backend::Output;
use retroglyph_core::terminal::Terminal;
#[cfg(feature = "crossterm")]
use std::rc::Rc;
use std::time::Duration;

/// A runnable example: `init` builds the state once, `tick` advances and
/// draws one frame.
///
/// Implement this once, generic over the backend, and call
/// `retroglyph_examples::launch::<Self>()` from `main`. The same
/// implementation runs on every backend the crate is built with --
/// `Headless`, `Crossterm`, `SoftwareRenderer`, or (via
/// [`wasm_entry!`](crate::wasm_entry)) the two WASM backends.
pub trait Example: Default + Sized + 'static {
    /// Display name, used as the window title on windowed backends.
    const NAME: &'static str;

    /// Build the initial state. Called once, before the first `tick`, with
    /// the first live `Terminal<B>` for the backend that's actually running
    /// -- not a placeholder built before the backend existed. This is the
    /// hook for anything that depends on the real starting grid size
    /// (`term.backend().size()`), which varies by backend (crossterm: the
    /// real terminal's columns/rows; software: whatever grid you
    /// configured; wasm-terminal: whatever JS set): centering a camera,
    /// sizing an initial layout, and so on.
    ///
    /// `Example` requires `Default` (rather than making it an optional
    /// bound on just this method) specifically so this default body works:
    /// `init` is called generically as `E::init(term)` from shared driver
    /// code (`ExampleApp`, `render_headless_frames`, the `wasm_entry!`
    /// macros) that only knows `E: Example`, not which examples happen to
    /// implement `Default` -- a default method can't add its own extra
    /// bound and still be callable through a bare `E: Example`, so the
    /// bound has to live on the trait itself. For an example with no
    /// backend-dependent startup state, `#[derive(Default)]` and skip
    /// overriding this entirely; for one that needs `term` (to center a
    /// camera on the real grid size, for example), override it and let
    /// `Default` stay an unused placeholder value that's never actually
    /// constructed.
    fn init<B: Backend>(_term: &mut Terminal<B>) -> Self {
        Self::default()
    }

    /// Customize a windowed backend's builder before it's built.
    ///
    /// Generic over [`PresenterBuilder`](retroglyph_window::presenter_builder::PresenterBuilder) rather than one
    /// method per backend crate: `SoftwareBackendBuilder`, `GlBackendBuilder`, and
    /// `WgpuBackendBuilder` are different types from different crates, but
    /// [`PresenterBuilder`](retroglyph_window::presenter_builder::PresenterBuilder) names the shape they share, so one
    /// override here customizes every windowed backend the example is built with instead of one
    /// per crate (retroglyph#1192). An example that registers a tileset needs it on every
    /// graphical backend it supports, or a WebGL2/wgpu variant renders bitmap glyphs where the
    /// software variant renders sprites -- see `07_sprites_tileset.rs` for a real override.
    ///
    /// Default: `builder` unchanged, i.e. [`run_software`]/[`run_gl`]/[`run_wgpu`]'s standard
    /// 50x25-at-2x grid with no tileset. `launch::<E>()`/`example_main!` still dispatch through the
    /// exact same path on every backend either way; this is the one customization point the
    /// windowed drivers thread through to the example, the same way [`init`](Self::init) is the
    /// one customization point for backend-dependent startup state.
    #[cfg(any(feature = "software", feature = "gl", feature = "wgpu"))]
    fn configure<B: retroglyph_window::presenter_builder::PresenterBuilder>(builder: B) -> B {
        builder
    }

    /// Whether the windowed backend's window should fill the browser viewport on `wasm32`
    /// (see [`WindowConfig::fill_viewport`](retroglyph_window::winit::WindowConfig::fill_viewport))
    /// instead of rendering at its natural grid size wherever it lands on the page.
    ///
    /// Default: `false`, matching [`WindowConfig::fit`](retroglyph_window::winit::WindowConfig::fit)'s
    /// own default -- most demos should render at a fixed, predictable grid size. Override this
    /// (returning `true`) for an app-like example meant to be the whole page, e.g. one with a
    /// pannable world that benefits from every cell the viewport can offer, especially on a small
    /// mobile screen -- see `15_outpost_dashboard.rs`. Read by [`run_software`], [`run_gl`], and
    /// [`run_wgpu`], so it applies to every windowed backend alike, on `wasm32` for all three.
    #[cfg(any(feature = "software", feature = "gl", feature = "wgpu"))]
    fn fill_viewport() -> bool {
        false
    }

    /// Advance and render one frame. Return `false` to quit.
    ///
    /// `frame` carries the real wall-clock time elapsed since the previous tick
    /// ([`Frame::delta`]), already measured correctly by whichever driver is
    /// actually running (`run_on`'s `std::time::Instant` on native,
    /// `run_app`'s native/wasm split, or a fixed synthetic delta from the
    /// headless test harness -- see [`render_headless_frames`]). Any example
    /// that animates over real time (rather than once per raw tick, which can
    /// fire at wildly different rates depending on the backend -- crossterm's
    /// `run_on` is an unthrottled spin loop, unlike the software
    /// backend's vsync-paced redraw) should drive a [`Tween`](retroglyph_ui::animate::Tween)
    /// or [`FrameClock`](retroglyph_core::frames::FrameClock) with `frame.delta`
    /// instead of counting raw `tick` calls -- see `06_layers.rs`.
    ///
    /// Draws only -- it does **not** call [`Terminal::present`]. The shared driver presents after
    /// `tick` returns, so it can stamp the perf overlay (a [`PerfOverlayApp`] wrapping this
    /// adapter) on top first. Mirrors
    /// [`App::update`](retroglyph_core::app::App::update)'s combined
    /// input-then-draw shape deliberately (rather than splitting into
    /// separate `handle_events`/`draw` trait methods) so `Example` stays a
    /// single-method contract, consistent with the rest of the library.
    /// Nothing stops an implementation from splitting its own `tick` body
    /// into private helper methods once it grows past a couple of lines --
    /// see `01_hello_world.rs`'s `handle_events`/`draw` split for the
    /// pattern -- that's just internal structure, not part of this trait.
    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>, frame: &Frame) -> bool;
}

/// Adapts an [`Example`] into an [`App`], creating the state lazily on the
/// first frame so the same adapter works for both the blocking (crossterm)
/// driver and the inverted (software) driver.
///
/// Carries no perf-overlay state of its own: every [`run_software`]/[`run_gl`]/[`run_crossterm`]
/// wraps this in a [`PerfOverlayApp`], which owns the toggle key, the frame-time bookkeeping, and
/// drawing the readout on top -- generically, the same way on every backend. See that type's docs
/// for why this needed no bespoke per-backend plumbing here beyond the wasm floating button (see
/// [`WasmToggleApp`]).
#[cfg(any(
    feature = "crossterm",
    feature = "software",
    feature = "gl",
    feature = "wgpu"
))]
struct ExampleApp<E> {
    state: Option<E>,
    /// Multiplier applied to [`Frame::delta`] before the example sees it, from
    /// [`time_scale`]. `1.0` for an ordinary run.
    time_scale: f64,
}

/// Multiplier applied to every [`Frame::delta`] handed to [`Example::tick`], from `--time-scale`
/// (see [`crate::args`]) or, if that flag isn't passed, the `RG_TIME_SCALE` environment
/// variable. Defaults to `1.0`, i.e. real time.
///
/// This exists for captures, not for viewers. An example that animates over real elapsed time
/// takes real seconds to reach its end state, and the ones that deliberately park there
/// (`06_layers`, `08_animation`) publish that parked state as the ready marker the PTY snapshot
/// harness waits on -- so the marker cannot appear until the whole animation has played out. That
/// makes the capture's wall-clock cost a property of the animation rather than of the terminal I/O
/// it is actually there to test, and a
/// [`FrameClock`](retroglyph_core::frames::FrameClock)-driven one cannot make that time up afterwards:
/// `advance` caps catch-up at five steps, so any stretch the child spends descheduled under a
/// loaded test runner is animation time it never gets back (retroglyph#544). Scaling the delta
/// keeps the marker meaning exactly what it meant before -- "the animation has settled" -- while
/// removing the fixed wall-clock floor underneath it.
///
/// Applied by the driver rather than by each example so it covers the whole gallery uniformly
/// (and so no example carries a test-only branch). Anything not parseable as a finite, positive
/// number is ignored in favour of `1.0`: this is a debugging/capture aid, and a typo in it should
/// not silently freeze or reverse an example's animation. Always `1.0` on `wasm32` (nothing sets
/// environment variables there).
#[cfg(any(
    feature = "crossterm",
    feature = "software",
    feature = "gl",
    feature = "wgpu"
))]
fn time_scale() -> f64 {
    crate::args::parsed()
        .time_scale
        .unwrap_or_else(|| scale_from_env(std::env::var("RG_TIME_SCALE").ok().as_deref()))
}

/// [`time_scale`]'s parsing, split out so it's testable without mutating the process environment
/// (`std::env::set_var` is `unsafe` in edition 2024, and `unsafe_code` is forbidden
/// workspace-wide).
#[cfg(any(
    feature = "crossterm",
    feature = "software",
    feature = "gl",
    feature = "wgpu"
))]
fn scale_from_env(value: Option<&str>) -> f64 {
    value
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|scale| scale.is_finite() && *scale > 0.0)
        .unwrap_or(1.0)
}

#[cfg(any(
    feature = "crossterm",
    feature = "software",
    feature = "gl",
    feature = "wgpu"
))]
impl<E> ExampleApp<E> {
    fn new() -> Self {
        Self {
            state: None,
            time_scale: time_scale(),
        }
    }
}

#[cfg(any(
    feature = "crossterm",
    feature = "software",
    feature = "gl",
    feature = "wgpu"
))]
impl<B: Backend, E: Example> App<B> for ExampleApp<E> {
    fn update(&mut self, term: &mut Terminal<B>, frame: &Frame) -> Flow {
        let state = self.state.get_or_insert_with(|| E::init(term));
        // Scaled for the example; the `PerfOverlayApp` wrapping this reports on real time, which
        // `RG_TIME_SCALE` does not change.
        let scaled = Frame {
            delta: frame.delta.mul_f64(self.time_scale),
            frame: frame.frame,
        };
        let keep_going = state.tick(term, &scaled);
        if !keep_going {
            // Quitting: `present` clears `current` each frame, so an example that returns without
            // drawing (it quit in its event handler before drawing) leaves `current` empty --
            // presenting it would erase the last frame. Leave the last drawn frame on screen and
            // exit, matching the old contract where `tick` presented only when it actually drew.
            return Flow::Exit;
        }
        Flow::Continue
    }
}

/// Wraps `inner` in a [`PerfOverlayApp`] configured the same way for every backend: visible per
/// `RG_FPS` (see [`crate::fps::starts_visible`]), cycling `Off -> Compact -> Full -> Off` for
/// every example in the gallery on one toggle key press. `Full` draws a bordered panel with a
/// frame-time sparkline (`retroglyph_ui::widget::PerfOverlay`) on top of `Compact`'s built-in
/// single-row readout; see [`crate::perf_overlay`] for why this lives here rather than in
/// `retroglyph-ui` (retroglyph#1286).
#[cfg(any(
    feature = "crossterm",
    feature = "software",
    feature = "gl",
    feature = "wgpu"
))]
fn perf_overlay_app<E: Example>(
    inner: ExampleApp<E>,
    backend: &'static str,
) -> PerfOverlayApp<ExampleApp<E>> {
    PerfOverlayApp::new(inner, backend).visible(crate::fps::starts_visible())
}

/// Adds the wasm floating toggle button on top of a [`PerfOverlayApp`]-wrapped [`ExampleApp`] --
/// the browser counterpart to the overlay's built-in backtick/F1 key, for the windowed backends
/// (a real key the page reliably owns doesn't exist on wasm the way it does natively).
///
/// A plain pass-through everywhere else: the click-detection body below only compiles in on
/// `wasm32` with a windowed backend enabled, so this wrapper costs nothing on native or on the
/// crossterm/headless backends (neither of which uses it). `retroglyph-wgpu` has no `wasm32`
/// build at all, so on that backend this is unconditionally a pass-through.
#[cfg(any(feature = "software", feature = "gl", feature = "wgpu"))]
struct WasmToggleApp<E: Example> {
    inner: PerfOverlayApp<ExampleApp<E>>,
}

#[cfg(any(feature = "software", feature = "gl", feature = "wgpu"))]
impl<B: Backend, E: Example> App<B> for WasmToggleApp<E> {
    fn update(&mut self, term: &mut Terminal<B>, frame: &Frame) -> Flow {
        #[cfg(target_arch = "wasm32")]
        {
            crate::fps::wasm_toggle::ensure_button();
            if crate::fps::wasm_toggle::take_toggle_request() {
                self.inner.toggle();
            }
        }
        self.inner.update(term, frame)
    }
}

/// Applies [`ToggleFilter`](crate::fps::ToggleFilter)-swallowed toggle presses to a
/// [`PerfOverlayApp`]-wrapped [`ExampleApp`], for crossterm.
///
/// The [`ToggleFilter`](crate::fps::ToggleFilter) itself intercepts the toggle key one layer
/// below `Terminal` (inside the raw backend's `Input::poll_event`) to avoid a race with the
/// example's own `drain_events` -- see that type's docs. It can only *count* presses, not flip
/// `PerfOverlayApp`'s visibility directly (it wraps the backend, not the app), so this applies
/// whatever it counted each frame before delegating.
#[cfg(feature = "crossterm")]
struct CrosstermToggleApp<E: Example> {
    inner: PerfOverlayApp<ExampleApp<E>>,
    presses: crate::fps::TogglePresses,
}

#[cfg(feature = "crossterm")]
impl<B: Backend, E: Example> App<B> for CrosstermToggleApp<E> {
    fn update(&mut self, term: &mut Terminal<B>, frame: &Frame) -> Flow {
        for _ in 0..self.presses.take() {
            self.inner.toggle();
        }
        self.inner.update(term, frame)
    }
}

// ── Software backend (desktop + WASM) ───────────────────────────────────────

/// Frame rate requested from the windowed backends, i.e. the
/// [`target_fps`](retroglyph_window::winit::WindowConfig::fit) every example in this gallery runs
/// at.
///
/// `Some(_)` rather than `None` for the whole gallery, not just the five examples that animate
/// (`06_layers`, `08_animation`, `11_sokoban`, `15_outpost_dashboard`, `20_overworld`): `None`
/// selects the windowed driver's redraw-on-demand mode, where a frame is only rendered in response
/// to an input/window event. That mode is right for the event-driven retro UIs the library is aimed
/// at, but wrong for a demo gallery -- an animating example under it renders one frame and then
/// freezes until the viewer happens to move the mouse, which reads as severe jank rather than as
/// a deliberate power saving. It also puts the four WASM variants of the docs gallery on the same
/// footing: the headless and terminal ones are already driven by an unconditional
/// `requestAnimationFrame` loop in their HTML templates (see
/// `docs/templates/examples/terminal-template.html`), so this is what makes the software and GL
/// canvases tick the same way rather than being the odd two out. `retroglyph-wgpu` has no
/// `wasm32` build, so for it this only ever means one frame per vsync on native.
///
/// 60 specifically because that's the common display refresh rate, so on native it lands one frame
/// per vsync without a partial-interval sleep; on `wasm32` the number is advisory and the browser's
/// `requestAnimationFrame` cadence wins either way.
#[cfg(any(feature = "software", feature = "gl", feature = "wgpu"))]
const TARGET_FPS: Option<u32> = Some(60);

/// Drives a already-configured [`PresenterBuilder`](retroglyph_window::presenter_builder::PresenterBuilder) to
/// completion: builds its presenter, wires up the perf overlay and wasm toggle button, and hands
/// both to `retroglyph-window`'s winit `App` driver.
///
/// Shared by [`run_software_with`], [`run_gl`], and [`run_wgpu`] -- the one driver behind all
/// three, generic over `B: PresenterBuilder` (retroglyph#1192). `backend_label` becomes the perf
/// overlay's backend readout and the panic message's backend name; it is the one thing that still
/// varies per caller, since [`PresenterBuilder`](retroglyph_window::presenter_builder::PresenterBuilder) has no
/// associated name of its own.
///
/// Deliberately does not call [`Example::configure`]: the caller applies that (or not) before
/// `builder` reaches here, which is what lets [`run_software_with`] hand in an already-customized
/// builder without `configure` being silently re-applied on top of it.
///
/// # Panics
///
/// Panics if `builder` fails to build its presenter, or if the event loop fails to start.
#[cfg(any(feature = "software", feature = "gl", feature = "wgpu"))]
fn run_windowed<E: Example, B: retroglyph_window::presenter_builder::PresenterBuilder>(
    builder: B,
    backend_label: &'static str,
) {
    #[cfg(target_arch = "wasm32")]
    console_error_panic_hook::set_once();

    let renderer = builder
        .build_presenter()
        .unwrap_or_else(|e| panic!("failed to initialize {backend_label} backend: {e}"));
    let config = retroglyph_window::winit::WindowConfig::fit(&renderer, E::NAME, TARGET_FPS, false)
        .fill_viewport(E::fill_viewport());
    let app = WasmToggleApp::<E> {
        inner: perf_overlay_app(ExampleApp::<E>::new(), backend_label),
    };
    retroglyph_window::winit::run_app(config, renderer, app).expect("event loop failed");
}

/// Runs `E` on the software (winit + softbuffer/Canvas2D) backend.
///
/// Builds a 50x25 window at `scale(2)` sized to fit via
/// [`WindowConfig::fit`](retroglyph_window::winit::WindowConfig::fit), then
/// drives it with `retroglyph-window`'s winit `App` driver. This same
/// function runs unchanged on native desktop and on `wasm32` (winit's event
/// loop is portable); on `wasm32` it still needs to be *invoked* somehow,
/// which is what [`wasm_entry!`](crate::wasm_entry)'s `#[wasm_bindgen(start)]`
/// shim is for.
///
/// # Panics
///
/// Panics if the software backend fails to initialize, or if the event loop
/// fails to start.
#[cfg(feature = "software")]
pub fn run_software<E: Example>() {
    run_software_with::<E>(E::configure(
        retroglyph_software::config::SoftwareBackendBuilder::new()
            .grid_size(50, 25)
            .scale(2),
    ));
}

/// Runs `E` on the software backend using a caller-supplied, already-
/// configured `builder` instead of [`run_software`]'s hardcoded 50x25-at-2x
/// default.
///
/// This is the lower-level building block [`run_software`] itself delegates to (via
/// [`Example::configure`]), so both stay in sync automatically; most examples that need
/// a non-default grid size, scale, font, or tileset should override
/// [`configure`](Example::configure) instead of calling this directly, since
/// that keeps `example_main!`'s single-call-site convention intact. Calling this directly from a
/// hand-written `main` remains available for anything `configure`'s builder-in,
/// builder-out shape can't express.
///
/// # Panics
///
/// Panics if the software backend fails to initialize, or if the event loop
/// fails to start.
#[cfg(feature = "software")]
pub fn run_software_with<E: Example>(builder: retroglyph_software::config::SoftwareBackendBuilder) {
    run_windowed::<E, _>(builder, "software");
}

// ── GL backend (desktop + WASM) ─────────────────────────────────────────────

/// Runs `E` on the GPU (OpenGL 3.3 native / WebGL2 wasm) backend.
///
/// Builds a 50x25 window at `scale(2)` sized to fit via
/// [`WindowConfig::fit`](retroglyph_window::winit::WindowConfig::fit), then drives it with
/// `retroglyph-window`'s winit `App` driver -- the same driver `run_software` uses, since
/// `GlRenderer` is a `Presenter` too. Customization goes through [`Example::configure`], the same
/// hook every windowed backend shares. Like `run_software`, it honors [`Example::fill_viewport`]
/// to fill the browser viewport on `wasm32` (the grid then grows to the canvas via the winit
/// resize path).
///
/// # Panics
///
/// Panics if the GL backend fails to initialize, or if the event loop fails to start.
#[cfg(feature = "gl")]
pub fn run_gl<E: Example>() {
    run_windowed::<E, _>(
        E::configure(
            retroglyph_gl::config::GlBackendBuilder::new()
                .grid_size(50, 25)
                .scale(2),
        ),
        "gl",
    );
}

// ── wgpu backend ────────────────────────────────────────────────────────────

/// Runs `E` on the GPU (Vulkan/Metal/D3D12, via `wgpu`) backend.
///
/// Builds a 50x25 window at `scale(2)` sized to fit via
/// [`WindowConfig::fit`](retroglyph_window::winit::WindowConfig::fit), then drives it with
/// `retroglyph-window`'s winit `App` driver -- the same driver `run_software`/`run_gl` use, since
/// `WgpuRenderer` is a `Presenter` too. Customization goes through [`Example::configure`], the
/// same hook every windowed backend shares.
///
/// Runs in a browser as well as natively, through WebGPU. [`Example::fill_viewport`] is honored on
/// `wasm32` exactly as it is for the other two windowed backends.
///
/// One browser-only difference: `retroglyph-wgpu` acquires its device asynchronously, because a
/// browser main thread cannot block on a future, so the canvas stays blank for the first few frames
/// after load and then starts rendering. That is expected rather than a failure. A browser without
/// WebGPU can't run this variant at all; the gallery's page detects that and says so.
///
/// # Panics
///
/// Panics if the wgpu backend fails to initialize, or if the event loop fails to start.
#[cfg(feature = "wgpu")]
pub fn run_wgpu<E: Example>() {
    run_windowed::<E, _>(
        E::configure(
            retroglyph_wgpu::config::WgpuBackendBuilder::new()
                .grid_size(50, 25)
                .scale(2),
        ),
        "wgpu",
    );
}

// ── Crossterm backend ───────────────────────────────────────────────────────

/// Runs `E` on the crossterm (real TTY) backend, blocking until it quits.
///
/// When `--record <path>` is passed (see [`crate::args`]), wraps the backend in a
/// [`retroglyph_recorder::FrameRecorder`] and writes an asciicast `.cast` to `path` once `E`
/// quits -- this is the one native, text-oriented backend `--record` covers; see that flag's own
/// docs on [`launch`] for why the windowed backends don't get it.
///
/// # Errors
///
/// Returns an error if the terminal fails to initialize, or if a frame present fails while `E` is
/// running.
#[cfg(feature = "crossterm")]
pub fn run_crossterm<E: Example>() -> std::io::Result<()> {
    // The backend has to be wrapped in a `ToggleFilter` before the `Terminal` sees it, or the
    // overlay's toggle key races the example's own `drain_events` and gets swallowed -- see
    // `ToggleFilter`'s docs for why crossterm specifically needs this and the windowed backends
    // don't.
    let presses = crate::fps::TogglePresses::default();
    let filter =
        crate::fps::ToggleFilter::new(retroglyph_crossterm::Crossterm::new()?, Rc::clone(&presses));
    let app = CrosstermToggleApp {
        inner: perf_overlay_app(ExampleApp::<E>::new(), "crossterm"),
        presses,
    };

    match crate::args::parsed().record.clone() {
        None => retroglyph_core::app::run_on(Terminal::new(filter), app),
        Some(path) => {
            let recorder = retroglyph_recorder::FrameRecorder::new(filter);
            let handle = recorder.handle();
            let size = recorder.inner().size();
            // `run_on` takes `Terminal<B>` (and so this `FrameRecorder`) by value and never
            // hands it back -- `handle`, taken before this call, is how the captured frames
            // survive it. Runs to completion (propagating any error) before saving, so a
            // recording is only written for a session that actually ran; see `save_cast` for why
            // a save failure itself doesn't override that result.
            let result = retroglyph_core::app::run_on(Terminal::new(recorder), app);
            save_cast(&handle, size, &path);
            result
        }
    }
}

/// Writes `handle`'s captured frames to `path` as asciicast v3, via
/// [`retroglyph_recorder::write_cast`]. Errors are logged to stderr rather than propagated: by
/// the time this runs, `E` has already quit (successfully or not) and the process is about to
/// exit either way, so failing to save the recording shouldn't also turn an otherwise-successful
/// run into a nonzero exit code.
#[cfg(not(target_arch = "wasm32"))]
fn save_cast(
    handle: &retroglyph_recorder::FrameRecorderHandle,
    size: retroglyph_core::grid::Size,
    path: &std::path::Path,
) {
    let frames = handle.frames();
    let result = std::fs::File::create(path)
        .and_then(|mut file| retroglyph_recorder::write_cast(&mut file, size, &frames));
    if let Err(error) = result {
        eprintln!("--record: failed to write {}: {error}", path.display());
    }
}

// ── Headless (stdout) fallback ──────────────────────────────────────────────

/// The synthetic per-call [`Frame::delta`] fed to [`Example::tick`].
///
/// Used by [`render_headless_frames`] and the crate's other hand-rolled headless test
/// loops (`03_keyboard`'s `headless_keyboard_snapshot`, `04_mouse`'s `drive`,
/// `support::png_snapshot`). No real clock is involved (headless never runs on wasm32 or against a
/// live backend, so there's no wall time to measure) -- this is a fixed
/// stand-in "one call is worth this much simulated time," chosen so a
/// `FrameClock`/`Tween`-driven example that advances one visible step per
/// 100ms of real elapsed time (see `06_layers.rs`) advances by exactly one
/// step per headless frame too, keeping headless snapshots' frame-by-frame
/// progression identical to what a human would see advancing one step at a
/// time interactively.
pub const HEADLESS_FRAME_DELTA: Duration = Duration::from_millis(100);

/// Renders up to `frames` frames of `E` against a fresh 50x25 `Headless` backend and returns
/// each frame's [`format_view`](retroglyph_core::backend::Headless::format_view) text.
///
/// No terminal or window is involved, and no input is ever injected --
/// `tick` only ever sees an empty event queue. Each call is handed a
/// [`Frame`] with [`HEADLESS_FRAME_DELTA`] as its delta (see that constant's
/// doc comment) and a monotonically increasing `frame` counter. Shared by
/// [`run_headless_stdout`] and the crate's snapshot tests, so both use the
/// exact same rendering path.
#[must_use]
pub fn render_headless_frames<E: Example>(frames: u32) -> Vec<String> {
    let backend = retroglyph_core::backend::Headless::new(50, 25);
    let mut term = Terminal::new(backend);
    let mut state = E::init(&mut term);

    let mut views = Vec::new();
    for i in 0..frames {
        let frame = Frame {
            delta: HEADLESS_FRAME_DELTA,
            frame: u64::from(i),
        };
        if !state.tick(&mut term, &frame) {
            break;
        }
        // The driver owns `present` now (the example's `tick` no longer does). No FPS overlay
        // here: headless is frame-stepped at a fixed synthetic delta, so a frame rate is meaningless
        // and it would perturb the snapshots.
        term.present().ok();
        views.push(term.backend().format_view());
    }
    views
}

/// Test-only: drives `E` through the [`PerfOverlayApp`]-wrapped harness.
///
/// Runs against a headless (no window) software renderer, so a caller can snapshot the perf
/// overlay exactly as the real harness (the same one [`run_software`] uses) draws it -- unlike
/// [`render_headless_frames`]/`support::png_snapshot`, which both drive `E::tick` directly and so
/// never show the overlay at all.
///
/// Runs `settle_frames` plain frames first (so [`retroglyph_core::frames::FrameStats`] has real samples
/// for a sparkline-drawing renderer to show), then one synthetic toggle-key press per frame for
/// `toggles` more frames (`PerfOverlayApp`'s toggle key cycles `Off -> Compact -> Full -> Off`),
/// then presents once. Returns `(width, height,
/// interleaved RGB bytes)`, the same shape `support::png_snapshot` PNG-encodes -- this function
/// stays free of an `image` dependency (a dev-dependency of the `tests/` binaries, not of this
/// library) by leaving the actual encoding to the caller.
///
/// # Panics
///
/// Panics if the software backend fails to initialize.
#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[must_use]
pub fn render_perf_overlay_rgb<E: Example>(
    cols: u16,
    rows: u16,
    scale: u16,
    settle_frames: u32,
    toggles: u32,
) -> (u32, u32, Vec<u8>) {
    use retroglyph_core::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use retroglyph_window::presenter::Presenter;

    let renderer = E::configure(
        retroglyph_software::config::SoftwareBackendBuilder::new()
            .grid_size(cols, rows)
            .scale(scale),
    )
    .build()
    .expect("software backend init")
    .into_renderer()
    .expect("headless renderer init");

    // Read the pixel-buffer geometry before handing `renderer` to `Terminal` (which owns it from
    // here): cols/rows in cells x the presenter's own reported cell size in pixels -- the same
    // approach `support::png_snapshot` uses.
    let (cell_w, cell_h) = renderer.cell_size();
    let width = u32::from(cols) * cell_w;
    let height = u32::from(rows) * cell_h;

    let mut term = Terminal::new(renderer);
    let mut app = perf_overlay_app(ExampleApp::<E>::new(), "software");

    let mut frame_n = 0u64;
    let mut tick = |app: &mut PerfOverlayApp<ExampleApp<E>>, term: &mut Terminal<_>| {
        // A varying synthetic delta, not a flat constant like `HEADLESS_FRAME_DELTA`: every
        // sample landing at the same value would give a sparkline-drawing renderer nothing to
        // show (every bar the same maxed-out height and color) and a static fps/min/max readout,
        // neither of which is representative of what this overlay looks like in a real run.
        #[allow(clippy::cast_precision_loss)] // `frame_n` is display-jitter phase, not a count.
        let phase = frame_n as f64 * 0.35;
        let millis = 10.0f64.mul_add(phase.sin(), 16.0);
        let frame = Frame {
            delta: Duration::from_secs_f64(millis / 1000.0),
            frame: frame_n,
        };
        frame_n += 1;
        let _ = App::update(app, term, &frame);
    };

    for _ in 0..settle_frames {
        tick(&mut app, &mut term);
    }
    for _ in 0..toggles {
        term.backend_mut().push_event(Event::Key(KeyEvent::new(
            KeyCode::Char('`'),
            KeyModifiers::NONE,
        )));
        tick(&mut app, &mut term);
    }
    term.present().ok();

    let mut rgb = Vec::with_capacity(term.backend().pixels().len() * 3);
    for &p in term.backend().pixels() {
        rgb.push(((p >> 16) & 0xff) as u8);
        rgb.push(((p >> 8) & 0xff) as u8);
        rgb.push((p & 0xff) as u8);
    }
    (width, height, rgb)
}

/// Fallback `main` body when neither `crossterm` nor `software` is enabled:
/// ticks a few frames against a [`Headless`](retroglyph_core::backend::Headless)
/// backend and prints each to stdout.
///
/// This exists so every example keeps a `main` (and stays `cargo
/// build`-able) with the crate's default feature set, and so
/// `examples/src/bin/runner.rs` can offer a "Headless" backend option
/// uniformly across examples instead of requiring each one to opt in
/// individually. Frame count defaults to 3 and can be overridden with `--headless-frames` (see
/// [`crate::args`]) or, if that flag isn't passed, the `RG_HEADLESS_FRAMES` environment
/// variable.
#[cfg(target_arch = "wasm32")]
pub fn run_headless_stdout<E: Example>() {
    let frames = headless_frame_count();
    for (i, view) in render_headless_frames::<E>(frames).into_iter().enumerate() {
        println!("--- Frame {} ---", i + 1);
        println!("{view}");
    }
}

/// Native counterpart to the `wasm32` [`run_headless_stdout`] above.
///
/// Adds `--record <path>` support: wraps the backend in a [`retroglyph_recorder::FrameRecorder`]
/// and writes an asciicast `.cast` to `path` once `E` quits. A separate loop (rather than adding
/// an optional `FrameRecorder` parameter to [`render_headless_frames`], which stays exactly as it
/// was) since `retroglyph-recorder` is a native-only dependency (see `examples/Cargo.toml`).
#[cfg(not(target_arch = "wasm32"))]
pub fn run_headless_stdout<E: Example>() {
    use retroglyph_core::backend::{Headless, Output as _};

    let frames = headless_frame_count();

    let Some(path) = crate::args::parsed().record.clone() else {
        for (i, view) in render_headless_frames::<E>(frames).into_iter().enumerate() {
            println!("--- Frame {} ---", i + 1);
            println!("{view}");
        }
        return;
    };

    let backend = retroglyph_recorder::FrameRecorder::new(Headless::new(50, 25));
    let mut term = Terminal::new(backend);
    let mut state = E::init(&mut term);
    for i in 0..frames {
        let frame = Frame {
            delta: HEADLESS_FRAME_DELTA,
            frame: u64::from(i),
        };
        if !state.tick(&mut term, &frame) {
            break;
        }
        term.present().ok();
        println!("--- Frame {} ---", i + 1);
        println!("{}", term.backend().inner().format_view());
    }
    let handle = term.backend().handle();
    let size = term.backend().inner().size();
    save_cast(&handle, size, &path);
}

/// The frame count [`run_headless_stdout`] renders, from `--headless-frames` (see
/// [`crate::args`]) or, if that flag isn't passed, the `RG_HEADLESS_FRAMES` environment
/// variable. Defaults to 3.
fn headless_frame_count() -> u32 {
    crate::args::parsed().headless_frames.unwrap_or_else(|| {
        std::env::var("RG_HEADLESS_FRAMES")
            .ok()
            .and_then(|s| s.parse().ok())
            .filter(|&n| n > 0)
            .unwrap_or(3)
    })
}

// ── Backend dispatch ─────────────────────────────────────────────────────────
//
// Mutually exclusive by construction: at most one of these `launch` items is compiled in for any
// given feature set, mirroring the old rg_run! macro's priority, extended with `wgpu` as a third
// windowed tier below `gl`: software > gl > wgpu > crossterm > wasm-headless > wasm-terminal >
// headless stdout fallback. `wasm-headless`/`wasm-terminal` on non-wasm32 targets (e.g. `cargo
// check --features wasm-headless` on a host) fall through to the headless-stdout arm, so every
// feature combination stays host-checkable.

/// Picks a backend from the crate's enabled Cargo features and runs `E` on
/// it. Call this (and nothing else) from every example's `main`.
///
/// `--record <path>` (see [`crate::args`]) is honored by the crossterm and headless-stdout
/// arms only, not this windowed (software) one: [`retroglyph_recorder::write_cast`] exports
/// text/ANSI output, and this backend presents pixels, not `DrawCell` glyph diffs, so there is
/// nothing meaningful for a `FrameRecorder` to capture here. Passing `--record` to a
/// software/gl/wgpu build is silently ignored rather than an error, matching this crate's
/// existing convention for a flag or env var a particular build doesn't apply to (see
/// `time_scale`'s `wasm32` note for the same convention elsewhere).
#[cfg(feature = "software")]
pub fn launch<E: Example>() {
    run_software::<E>();
}

/// See [`launch`]'s software-enabled overload. `gl` is the GPU windowed backend; `software` wins
/// if both are somehow enabled.
#[cfg(all(feature = "gl", not(feature = "software")))]
pub fn launch<E: Example>() {
    run_gl::<E>();
}

/// See [`launch`]'s software-enabled overload. `wgpu` is the other GPU windowed backend; it loses
/// to `software`/`gl` if either is also enabled, the same way `gl` loses to `software`.
#[cfg(all(feature = "wgpu", not(any(feature = "software", feature = "gl"))))]
pub fn launch<E: Example>() {
    run_wgpu::<E>();
}

/// See [`launch`]'s software-enabled overload.
#[cfg(all(
    feature = "crossterm",
    not(any(feature = "software", feature = "gl", feature = "wgpu"))
))]
pub fn launch<E: Example>() {
    run_crossterm::<E>().expect("crossterm backend failed");
}

/// No-op on `wasm32`: the real entry points for this backend are the
/// `#[wasm_bindgen]` functions generated by
/// [`wasm_entry!`](crate::wasm_entry), which JS calls directly instead of
/// through `main`.
#[cfg(all(
    feature = "wasm-headless",
    not(any(feature = "software", feature = "gl")),
    target_arch = "wasm32"
))]
pub fn launch<E: Example>() {
    let _ = core::marker::PhantomData::<E>;
}

/// No-op on `wasm32`: see the `wasm-headless` overload above.
#[cfg(all(
    feature = "wasm-terminal",
    not(any(feature = "software", feature = "gl", feature = "wasm-headless")),
    target_arch = "wasm32"
))]
pub fn launch<E: Example>() {
    let _ = core::marker::PhantomData::<E>;
}

/// Fallback: no backend feature enabled (or `wasm-headless`/`wasm-terminal`
/// enabled but not building for `wasm32`).
#[cfg(not(any(
    feature = "crossterm",
    feature = "software",
    feature = "gl",
    feature = "wgpu",
    all(feature = "wasm-headless", target_arch = "wasm32"),
    all(feature = "wasm-terminal", target_arch = "wasm32"),
)))]
pub fn launch<E: Example>() {
    run_headless_stdout::<E>();
}

#[cfg(all(
    test,
    any(
        feature = "crossterm",
        feature = "software",
        feature = "gl",
        feature = "wgpu"
    )
))]
mod tests {
    use super::scale_from_env;

    #[test]
    fn unset_time_scale_is_real_time() {
        assert!((scale_from_env(None) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parses_a_positive_scale() {
        assert!((scale_from_env(Some("20")) - 20.0).abs() < f64::EPSILON);
        assert!((scale_from_env(Some(" 2.5 ")) - 2.5).abs() < f64::EPSILON);
    }

    /// A typo (or a deliberate zero/negative) must not freeze or reverse an example's animation,
    /// and must not reach `Duration::mul_f64`, which panics on a non-finite or negative factor.
    #[test]
    fn rejects_values_that_are_not_a_usable_speed() {
        for value in ["", "fast", "0", "-1", "inf", "NaN"] {
            assert!(
                (scale_from_env(Some(value)) - 1.0).abs() < f64::EPSILON,
                "{value:?} should have fallen back to real time"
            );
        }
    }
}
