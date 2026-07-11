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

#[cfg(any(feature = "crossterm", feature = "software"))]
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
    /// Responsible for calling [`Terminal::present`]. Mirrors
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
#[cfg(any(feature = "crossterm", feature = "software"))]
struct ExampleApp<E> {
    state: Option<E>,
}

#[cfg(any(feature = "crossterm", feature = "software"))]
impl<E> ExampleApp<E> {
    const fn new() -> Self {
        Self { state: None }
    }
}

#[cfg(any(feature = "crossterm", feature = "software"))]
impl<B: Backend, E: Example> App<B> for ExampleApp<E> {
    fn update(&mut self, term: &mut Terminal<B>, frame: &Frame) -> Flow {
        let state = self.state.get_or_insert_with(|| E::init(term));
        if state.tick(term, frame) {
            Flow::Continue
        } else {
            Flow::Exit
        }
    }
}

// ── Software backend (desktop + WASM) ───────────────────────────────────────

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
    run_software_with::<E>(
        retroglyph_software::SoftwareBackendBuilder::new()
            .grid_size(50, 25)
            .scale(2),
    );
}

/// Runs `E` on the software backend using a caller-supplied, already-
/// configured `builder` instead of [`run_software`]'s hardcoded 50x25-at-2x
/// default.
///
/// This is the escape hatch for examples that need a custom grid size,
/// scale, font, or tileset -- `retroglyph-examples`'s `software` feature
/// unconditionally pulls in `retroglyph-software/default-font` (see the
/// Cargo.toml comment), but the builder itself is fully caller-controlled
/// here. [`run_software`] delegates to this with its default builder, so
/// both stay in sync automatically.
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
        .run_headless();
    let config = retroglyph_window::winit::WindowConfig::fit(&renderer, E::NAME, None);
    let app = ExampleApp::<E>::new();
    retroglyph_window::winit::run_app(config, renderer, app).expect("event loop failed");
}

// ── Crossterm backend ───────────────────────────────────────────────────────

/// Runs `E` on the crossterm (real TTY) backend, blocking until it quits.
///
/// # Errors
///
/// Returns an error if the terminal fails to initialize.
#[cfg(feature = "crossterm")]
pub fn run_crossterm<E: Example>() -> std::io::Result<()> {
    retroglyph_crossterm::Crossterm::run(ExampleApp::<E>::new())
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

/// See [`launch`]'s software-enabled overload.
#[cfg(all(feature = "crossterm", not(feature = "software")))]
pub fn launch<E: Example>() {
    run_crossterm::<E>().expect("crossterm backend failed");
}

/// No-op on `wasm32`: the real entry points for this backend are the
/// `#[wasm_bindgen]` functions generated by
/// [`wasm_entry!`](crate::wasm_entry), which JS calls directly instead of
/// through `main`.
#[cfg(all(
    feature = "wasm-headless",
    not(feature = "software"),
    target_arch = "wasm32"
))]
pub fn launch<E: Example>() {
    let _ = core::marker::PhantomData::<E>;
}

/// No-op on `wasm32`: see the `wasm-headless` overload above.
#[cfg(all(
    feature = "wasm-terminal",
    not(any(feature = "software", feature = "wasm-headless")),
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
    all(feature = "wasm-headless", target_arch = "wasm32"),
    all(feature = "wasm-terminal", target_arch = "wasm32"),
)))]
pub fn launch<E: Example>() {
    run_headless_stdout::<E>();
}
