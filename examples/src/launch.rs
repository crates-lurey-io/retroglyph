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

#[cfg(any(feature = "crossterm", feature = "software", feature = "gl"))]
use retroglyph_core::{App, Flow};
use retroglyph_core::{Backend, Frame, Terminal};
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

    /// Customize the software backend's builder before it's built.
    ///
    /// Default: `builder` unchanged, i.e. [`run_software`]'s standard 50x25-at-2x grid with no
    /// tileset. Override this (rather than hand-writing a custom `main`) when an example needs a
    /// non-default grid size, scale, font, or tileset -- see `07_sprites_tileset.rs` for a real
    /// override. `launch::<E>()`/`example_main!` still dispatch through the exact same path on
    /// every backend either way; this is the one customization point `run_software` threads
    /// through to the example, the same way [`init`](Self::init) is the one customization point
    /// for backend-dependent startup state.
    #[cfg(feature = "software")]
    fn configure_software(
        builder: retroglyph_software::SoftwareBackendBuilder,
    ) -> retroglyph_software::SoftwareBackendBuilder {
        builder
    }

    /// Customize the GL backend's builder before it's built.
    ///
    /// The GL counterpart of [`configure_software`](Self::configure_software), and the reason it
    /// is a second method rather than a shared one: the two builders are different types from
    /// different crates. An example that registers a tileset needs to register it on both, or the
    /// docs gallery's WebGL2 variant renders bitmap glyphs where the software variant renders
    /// sprites. The [`TilesetOptions`](retroglyph_window::tileset::TilesetOptions) themselves are
    /// shared, so the usual shape is one helper returning the options and two one-line overrides
    /// -- see `07_sprites_tileset.rs`.
    ///
    /// Default: `builder` unchanged, i.e. [`run_gl`]'s standard 50x25-at-2x grid with no tileset.
    #[cfg(feature = "gl")]
    fn configure_gl(builder: retroglyph_gl::GlBackendBuilder) -> retroglyph_gl::GlBackendBuilder {
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
    /// mobile screen -- see `15_outpost_dashboard.rs`. Read by both [`run_software`] and
    /// [`run_gl`], so it applies to the software (`Canvas2D`) and GL (WebGL2) wasm backends alike;
    /// has no effect on native.
    #[cfg(any(feature = "software", feature = "gl"))]
    fn fill_viewport() -> bool {
        false
    }

    /// Advance and render one frame. Return `false` to quit.
    ///
    /// `frame` carries the real wall-clock time elapsed since the previous tick
    /// ([`Frame::delta`]), already measured correctly by whichever driver is
    /// actually running (`run_blocking`'s `std::time::Instant` on native,
    /// `run_app`'s native/wasm split, or a fixed synthetic delta from the
    /// headless test harness -- see [`render_headless_frames`]). Any example
    /// that animates over real time (rather than once per raw tick, which can
    /// fire at wildly different rates depending on the backend -- crossterm's
    /// `run_blocking` is an unthrottled spin loop, unlike the software
    /// backend's vsync-paced redraw) should drive a [`Tween`](retroglyph_core::Tween)
    /// or [`FrameClock`](retroglyph_core::FrameClock) with `frame.delta`
    /// instead of counting raw `tick` calls -- see `06_layers.rs`.
    ///
    /// Draws only -- it does **not** call [`Terminal::present`]. The shared driver presents after
    /// `tick` returns, so it can stamp the [FPS overlay](crate::fps) on top first. Mirrors
    /// [`App::update`](retroglyph_core::App::update)'s combined
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
/// Only referenced by [`run_software`]/[`run_crossterm`]; with neither
/// feature enabled (the headless-stdout fallback), nothing constructs one.
#[cfg(any(feature = "crossterm", feature = "software", feature = "gl"))]
struct ExampleApp<E> {
    state: Option<E>,
    /// Backend label for the FPS overlay ("software"/"gl"/"crossterm").
    backend_name: &'static str,
    /// Smoothed frame-timing state and visibility for the FPS overlay.
    fps: crate::fps::Fps,
    /// Scratch buffer for [`intercept_overlay_keys`]'s pass-through events, reused across frames
    /// so the interception doesn't allocate on every frame that has input.
    passthrough: Vec<retroglyph_core::event::Event>,
    /// Toggle presses swallowed at the source by a [`ToggleFilter`](crate::fps::ToggleFilter).
    /// Always empty unless [`run_crossterm`] installed one; see that filter for why crossterm
    /// needs it and the windowed backends don't.
    filtered_toggles: crate::fps::TogglePresses,
    /// Multiplier applied to [`Frame::delta`] before the example sees it, from
    /// [`time_scale`]. `1.0` for an ordinary run.
    time_scale: f64,
}

/// Multiplier applied to every [`Frame::delta`] handed to [`Example::tick`], read once from the
/// `RG_TIME_SCALE` environment variable. Defaults to `1.0`, i.e. real time.
///
/// This exists for captures, not for viewers. An example that animates over real elapsed time
/// takes real seconds to reach its end state, and the ones that deliberately park there
/// (`06_layers`, `08_animation`) publish that parked state as the ready marker the PTY snapshot
/// harness waits on -- so the marker cannot appear until the whole animation has played out. That
/// makes the capture's wall-clock cost a property of the animation rather than of the terminal I/O
/// it is actually there to test, and a
/// [`FrameClock`](retroglyph_core::FrameClock)-driven one cannot make that time up afterwards:
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
#[cfg(any(feature = "crossterm", feature = "software", feature = "gl"))]
fn time_scale() -> f64 {
    scale_from_env(std::env::var("RG_TIME_SCALE").ok().as_deref())
}

/// [`time_scale`]'s parsing, split out so it's testable without mutating the process environment
/// (`std::env::set_var` is `unsafe` in edition 2024, and `unsafe_code` is forbidden
/// workspace-wide).
#[cfg(any(feature = "crossterm", feature = "software", feature = "gl"))]
fn scale_from_env(value: Option<&str>) -> f64 {
    value
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|scale| scale.is_finite() && *scale > 0.0)
        .unwrap_or(1.0)
}

#[cfg(any(feature = "crossterm", feature = "software", feature = "gl"))]
impl<E> ExampleApp<E> {
    fn new(backend_name: &'static str) -> Self {
        Self {
            state: None,
            backend_name,
            fps: crate::fps::Fps::new(crate::fps::starts_visible()),
            passthrough: Vec::new(),
            filtered_toggles: crate::fps::TogglePresses::default(),
            time_scale: time_scale(),
        }
    }

    /// Consumes the [FPS overlay's toggle key](crate::fps::is_toggle_key) out of this frame's
    /// pending input, and hands everything else straight back.
    ///
    /// The overlay is the driver's, not the example's, so its key has to be handled here -- but
    /// there's no non-destructive way to look at the queue ([`Terminal`] buffers exactly one
    /// event and won't hand it back, and [`App`] has no event hook), so "look at it" means drain
    /// it and re-push what wasn't ours. Order is preserved: the drain empties both the terminal's
    /// one-event buffer and the backend's queue, so re-pushing to the back of an empty queue puts
    /// everything back in the order it arrived, still ahead of anything that lands later.
    ///
    /// This is why [`Input::push_event`](retroglyph_core::Input::push_event) has to actually work
    /// on every backend an example runs on. It didn't on crossterm until it grew a pushback queue
    /// of its own; the trait default silently drops, which here would have eaten every keystroke.
    ///
    /// Draining the queue only catches keys that were already waiting when the frame started,
    /// which is every key the example can see on the windowed backends and *not* on crossterm --
    /// [`ToggleFilter`](crate::fps::ToggleFilter) covers the difference, and this folds in what it
    /// swallowed.
    fn intercept_overlay_keys<B: Backend>(&mut self, term: &mut Terminal<B>) {
        self.passthrough.clear();
        self.passthrough.extend(term.drain_events());

        let mut toggles = self.filtered_toggles.take();
        self.passthrough.retain(|event| {
            let is_toggle = crate::fps::is_toggle_key(event);
            toggles += usize::from(is_toggle);
            !is_toggle
        });
        for _ in 0..toggles {
            self.fps.toggle();
        }

        for event in self.passthrough.drain(..) {
            term.backend_mut().push_event(event);
        }
    }
}

#[cfg(any(feature = "crossterm", feature = "software", feature = "gl"))]
impl<B: Backend, E: Example> App<B> for ExampleApp<E> {
    fn update(&mut self, term: &mut Terminal<B>, frame: &Frame) -> Flow {
        self.intercept_overlay_keys(term);
        let state = self.state.get_or_insert_with(|| E::init(term));
        // Scaled for the example, real for the overlay below: the FPS readout reports how fast
        // this loop is actually running, which `RG_TIME_SCALE` does not change.
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
        // The driver presents automatically after `update` returns, so the FPS overlay (a top
        // layer) is stamped after the example's draw and survives to the flush.
        self.fps.tick(frame.delta);
        self.fps.draw(&mut term.surface(), self.backend_name);
        Flow::Continue
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
/// canvases tick the same way rather than being the odd two out.
///
/// 60 specifically because that's the common display refresh rate, so on native it lands one frame
/// per vsync without a partial-interval sleep; on `wasm32` the number is advisory and the browser's
/// `requestAnimationFrame` cadence wins either way.
#[cfg(any(feature = "software", feature = "gl"))]
const TARGET_FPS: Option<u32> = Some(60);

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
    run_software_with::<E>(E::configure_software(
        retroglyph_software::SoftwareBackendBuilder::new()
            .grid_size(50, 25)
            .scale(2),
    ));
}

/// Runs `E` on the software backend using a caller-supplied, already-
/// configured `builder` instead of [`run_software`]'s hardcoded 50x25-at-2x
/// default.
///
/// This is the lower-level building block [`run_software`] itself delegates to (via
/// [`Example::configure_software`]), so both stay in sync automatically; most examples that need
/// a non-default grid size, scale, font, or tileset should override
/// [`configure_software`](Example::configure_software) instead of calling this directly, since
/// that keeps `example_main!`'s single-call-site convention intact. Calling this directly from a
/// hand-written `main` remains available for anything `configure_software`'s builder-in,
/// builder-out shape can't express.
///
/// # Panics
///
/// Panics if the software backend fails to initialize, or if the event loop
/// fails to start.
#[cfg(feature = "software")]
pub fn run_software_with<E: Example>(builder: retroglyph_software::SoftwareBackendBuilder) {
    #[cfg(target_arch = "wasm32")]
    console_error_panic_hook::set_once();

    let renderer = builder
        .build()
        .expect("failed to initialize software backend")
        .run_headless()
        .expect("failed to build headless renderer");
    let config = retroglyph_window::winit::WindowConfig::fit(&renderer, E::NAME, TARGET_FPS, false)
        .fill_viewport(E::fill_viewport());
    let app = ExampleApp::<E>::new("software");
    retroglyph_window::winit::run_app(config, renderer, app).expect("event loop failed");
}

// ── GL backend (desktop + WASM) ─────────────────────────────────────────────

/// Runs `E` on the GPU (OpenGL 3.3 native / WebGL2 wasm) backend.
///
/// Builds a 50x25 window at `scale(2)` sized to fit via
/// [`WindowConfig::fit`](retroglyph_window::winit::WindowConfig::fit), then drives it with
/// `retroglyph-window`'s winit `App` driver -- the same driver `run_software` uses, since
/// `GlRenderer` is a `Presenter` too. Customization goes through [`Example::configure_gl`]
/// rather than [`Example::configure_software`], since the two backends have different builder
/// types. Like `run_software`, it honors [`Example::fill_viewport`] to fill the browser viewport
/// on `wasm32` (the grid then grows to the canvas via the winit resize path).
///
/// # Panics
///
/// Panics if the GL backend fails to initialize, or if the event loop fails to start.
#[cfg(feature = "gl")]
pub fn run_gl<E: Example>() {
    #[cfg(target_arch = "wasm32")]
    console_error_panic_hook::set_once();

    let renderer = E::configure_gl(
        retroglyph_gl::GlBackendBuilder::new()
            .grid_size(50, 25)
            .scale(2),
    )
    .build()
    .expect("failed to initialize gl backend");
    let config = retroglyph_window::winit::WindowConfig::fit(&renderer, E::NAME, TARGET_FPS, false)
        .fill_viewport(E::fill_viewport());
    let app = ExampleApp::<E>::new("gl");
    retroglyph_window::winit::run_app(config, renderer, app).expect("event loop failed");
}

// ── Crossterm backend ───────────────────────────────────────────────────────

/// Runs `E` on the crossterm (real TTY) backend, blocking until it quits.
///
/// # Errors
///
/// Returns an error if the terminal fails to initialize, or if a frame present fails while `E` is
/// running.
#[cfg(feature = "crossterm")]
pub fn run_crossterm<E: Example>() -> std::io::Result<()> {
    // Not `Crossterm::run`, which builds the `Terminal` itself: the backend has to be wrapped in
    // a `ToggleFilter` before the terminal sees it, or the overlay's toggle key races the
    // example's own `drain_events` and gets swallowed. See `ToggleFilter`.
    let app = ExampleApp::<E>::new("crossterm");
    let filter = crate::fps::ToggleFilter::new(
        retroglyph_crossterm::Crossterm::new()?,
        std::rc::Rc::clone(&app.filtered_toggles),
    );
    retroglyph_core::run_blocking(Terminal::new(filter), app)
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

/// Renders up to `frames` frames of `E` against a fresh 50x25
/// [`Headless`](retroglyph_core::Headless) backend and returns each frame's
/// [`format_view`](retroglyph_core::Headless::format_view) text.
///
/// No terminal or window is involved, and no input is ever injected --
/// `tick` only ever sees an empty event queue. Each call is handed a
/// [`Frame`] with [`HEADLESS_FRAME_DELTA`] as its delta (see that constant's
/// doc comment) and a monotonically increasing `frame` counter. Shared by
/// [`run_headless_stdout`] and the crate's snapshot tests, so both use the
/// exact same rendering path.
#[must_use]
pub fn render_headless_frames<E: Example>(frames: u32) -> Vec<String> {
    let backend = retroglyph_core::Headless::new(50, 25);
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

/// Fallback `main` body when neither `crossterm` nor `software` is enabled:
/// ticks a few frames against a [`Headless`](retroglyph_core::Headless)
/// backend and prints each to stdout.
///
/// This exists so every example keeps a `main` (and stays `cargo
/// build`-able) with the crate's default feature set, and so
/// `examples/src/bin/runner.rs` can offer a "Headless" backend option
/// uniformly across examples instead of requiring each one to opt in
/// individually. Frame count defaults to 3 and can be overridden with the
/// `RG_HEADLESS_FRAMES` environment variable.
pub fn run_headless_stdout<E: Example>() {
    let frames: u32 = std::env::var("RG_HEADLESS_FRAMES")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(3);

    for (i, view) in render_headless_frames::<E>(frames).into_iter().enumerate() {
        println!("--- Frame {} ---", i + 1);
        println!("{view}");
    }
}

// ── Backend dispatch ─────────────────────────────────────────────────────────
//
// Mutually exclusive by construction: at most one of these `launch` items is
// compiled in for any given feature set, mirroring the old rg_run! macro's
// priority (software > crossterm > wasm-headless > wasm-terminal > headless
// stdout fallback). `wasm-headless`/`wasm-terminal` on non-wasm32 targets
// (e.g. `cargo check --features wasm-headless` on a host) fall through to the
// headless-stdout arm, so every feature combination stays host-checkable.

/// Picks a backend from the crate's enabled Cargo features and runs `E` on
/// it. Call this (and nothing else) from every example's `main`.
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

/// See [`launch`]'s software-enabled overload.
#[cfg(all(feature = "crossterm", not(any(feature = "software", feature = "gl"))))]
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
    all(feature = "wasm-headless", target_arch = "wasm32"),
    all(feature = "wasm-terminal", target_arch = "wasm32"),
)))]
pub fn launch<E: Example>() {
    run_headless_stdout::<E>();
}

#[cfg(all(test, any(feature = "crossterm", feature = "software", feature = "gl")))]
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
