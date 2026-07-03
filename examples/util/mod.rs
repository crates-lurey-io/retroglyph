//! Shared utilities for rg examples.

#![allow(unreachable_pub, dead_code)]

pub mod action;
pub mod draw;
pub mod fov;
pub mod game;
pub mod layout;
pub mod lcg;
pub mod perf;
pub mod timestep;

/// Fallback `main` body for `rg_run!`/`rg_run_software!` when neither the
/// `crossterm` nor `software` feature is enabled.
///
/// Runs `init` once against a fresh [`Headless`] backend, then ticks a small
/// fixed number of frames, printing each frame's grid to stdout. No terminal
/// or window is involved, and no input is ever injected — `tick` only ever
/// sees an empty event queue, so purely time-driven demos (e.g. `subpixel`,
/// `dirty_viz`) show motion across frames while input-driven demos just
/// repeat their initial state.
///
/// This exists so every example keeps a `main` (and stays `cargo build`-able)
/// with the crate's default feature set, and so `examples/runner.rs` can
/// offer a "Headless" backend option uniformly across examples instead of
/// requiring each one to opt in individually. Frame count defaults to 3 and
/// can be overridden with the `RG_HEADLESS_FRAMES` environment variable.
///
/// [`Headless`]: retroglyph::backend::Headless
#[doc(hidden)]
pub fn run_headless<S>(
    mut init: impl FnMut(&mut retroglyph::Terminal<retroglyph::backend::Headless>) -> S,
    mut tick: impl FnMut(&mut retroglyph::Terminal<retroglyph::backend::Headless>, &mut S) -> bool,
) {
    let frames: u32 = std::env::var("RG_HEADLESS_FRAMES")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(3);

    let backend = retroglyph::backend::Headless::new(50, 25);
    let mut term = retroglyph::Terminal::new(backend);
    let mut state = init(&mut term);

    for frame in 1..=frames {
        if !tick(&mut term, &mut state) {
            println!("--- Frame {frame}: quit requested ---");
            break;
        }
        println!("--- Frame {frame} ---");
        println!("{}", term.backend().grid());
    }
}

/// Adapter turning an `init` + `tick` closure pair into an [`App`].
///
/// `tick` keeps its `(&mut Terminal, &mut State) -> bool` shape (return `false`
/// to quit) and is responsible for calling `present`, exactly as before. The
/// state is created lazily on the first frame, so the same adapter works for
/// both the blocking (crossterm) driver and the inverted (software) driver
/// without an `Option<State>` dance or `process::exit` in game code.
///
/// [`App`]: retroglyph::App
#[doc(hidden)]
pub struct ClosureApp<S, I, T> {
    state: Option<S>,
    init: I,
    tick: T,
}

impl<S, I, T> ClosureApp<S, I, T> {
    pub const fn new(init: I, tick: T) -> Self {
        Self {
            state: None,
            init,
            tick,
        }
    }
}

impl<B, S, I, T> retroglyph::App<B> for ClosureApp<S, I, T>
where
    B: retroglyph::Backend,
    I: FnMut(&mut retroglyph::Terminal<B>) -> S,
    T: FnMut(&mut retroglyph::Terminal<B>, &mut S) -> bool,
{
    fn update(
        &mut self,
        term: &mut retroglyph::Terminal<B>,
        _frame: &retroglyph::Frame,
    ) -> retroglyph::Flow {
        if self.state.is_none() {
            self.state = Some((self.init)(term));
        }
        let state = self.state.as_mut().expect("state initialized above");
        if (self.tick)(term, state) {
            retroglyph::Flow::Continue
        } else {
            retroglyph::Flow::Exit
        }
    }
}

/// Emit a `main` function wired to the enabled backend, driving an [`App`]
/// built from `init` + `tick` closures (ADR 015 Decision 2).
///
/// - When `software` is enabled (takes priority): runs the software renderer's
///   inverted driver. On `wasm32`, also emits a `wasm_bindgen(start)` entry.
/// - When only `crossterm` is enabled: runs the generic blocking driver.
/// - When neither is enabled: falls back to [`run_headless`], ticking a few
///   frames against a [`Headless`] backend and printing them to stdout. This
///   keeps every example buildable with the crate's default features and
///   backs `examples/runner.rs`'s "Headless" backend option.
///
/// [`Headless`]: retroglyph::backend::Headless
///
/// # Arguments
///
/// - `$State` — the game state type.
/// - `$init`  — callable as `$init(&mut Terminal<B>) -> $State`; called once.
/// - `$tick`  — callable as `$tick(&mut Terminal<B>, &mut $State) -> bool`;
///   return `false` to quit. Responsible for calling `present`.
///
/// # Example
///
/// ```ignore
/// mod util;
/// struct State { /* ... */ }
/// fn init<B: retroglyph::Backend>(term: &mut retroglyph::Terminal<B>) -> State { todo!() }
/// fn tick<B: retroglyph::Backend>(term: &mut retroglyph::Terminal<B>, s: &mut State) -> bool { todo!() }
/// util::rg_run!(State, init, tick);
/// ```
#[macro_export]
macro_rules! rg_run {
    ($State:ty, $init:expr, $tick:expr) => {
        // Wrap `$init`/`$tick` in named generic fns. Named fns are properly
        // higher-ranked over the `&mut Terminal<B>` lifetime, whereas a bare
        // closure that ignores its argument (e.g. `|_t| State::new()`) fails
        // HRTB inference when stored in the App adapter.
        #[allow(clippy::missing_const_for_fn, clippy::needless_pass_by_ref_mut)]
        fn __rg_init<B: ::retroglyph::Backend>(term: &mut ::retroglyph::Terminal<B>) -> $State {
            ($init)(term)
        }
        #[allow(clippy::missing_const_for_fn, clippy::needless_pass_by_ref_mut)]
        fn __rg_tick<B: ::retroglyph::Backend>(
            term: &mut ::retroglyph::Terminal<B>,
            state: &mut $State,
        ) -> bool {
            ($tick)(term, state)
        }

        // ── Software backend (desktop + WASM) ─────────────────────────────
        #[cfg(feature = "software")]
        fn main() {
            use ::retroglyph::backend::software::SoftwareBackendBuilder;

            #[cfg(target_arch = "wasm32")]
            ::console_error_panic_hook::set_once();

            let backend = SoftwareBackendBuilder::new()
                .title(env!("CARGO_BIN_NAME"))
                .grid_size(50, 25)
                .scale(2)
                .build()
                .expect("failed to initialize software backend");

            let app = $crate::util::ClosureApp::new(__rg_init, __rg_tick);
            backend.run_app(app).expect("event loop failed");
        }

        // WASM entry point called by the browser JS glue before the event loop.
        #[cfg(all(feature = "software", target_arch = "wasm32"))]
        #[allow(missing_docs)]
        #[::wasm_bindgen::prelude::wasm_bindgen(start)]
        pub fn wasm_main() -> ::std::result::Result<(), ::wasm_bindgen::JsValue> {
            main();
            ::std::result::Result::Ok(())
        }

        // ── Crossterm backend ──────────────────────────────────────────────
        #[cfg(all(feature = "crossterm", not(feature = "software")))]
        fn main() -> ::std::result::Result<(), ::std::io::Error> {
            let app = $crate::util::ClosureApp::new(__rg_init, __rg_tick);
            ::retroglyph::backend::Crossterm::run(app)
        }

        // ── Headless fallback (no backend feature enabled) ────────────────
        #[cfg(not(any(feature = "crossterm", feature = "software")))]
        fn main() {
            $crate::util::run_headless(__rg_init, __rg_tick);
        }
    };
}

/// Emit a software-only `main` function using a caller-supplied
/// [`SoftwareBackendBuilder`] expression.
///
/// Use this for examples that need custom grid dimensions, scale, tilesets,
/// or any other builder option that differs from `rg_run!`'s defaults.
/// Unlike `rg_run!`, no crossterm branch is emitted — these examples only run
/// on the desktop or WASM software renderer. When `software` is disabled,
/// falls back to [`run_headless`] like `rg_run!` does, so the example still
/// builds by default and supports `examples/runner.rs`'s "Headless" backend.
///
/// [`run_headless`]: crate::util::run_headless
///
/// # Arguments
///
/// - `$State`   — the game state type.
/// - `$init`    — callable as `$init(&mut Terminal<B>) -> $State`; called once.
/// - `$tick`    — callable as `$tick(&mut Terminal<B>, &mut $State) -> bool`;
///   return `false` to quit.
/// - `builder = $expr` — a block or expression that evaluates to a
///   *configured* [`SoftwareBackendBuilder`] (without the final `.build()`).
///
/// # Example
///
/// ```ignore
/// mod util;
/// struct State { ... }
/// fn init<B: retroglyph::Backend>(t: &mut retroglyph::Terminal<B>) -> State { ... }
/// fn tick<B: retroglyph::Backend>(t: &mut retroglyph::Terminal<B>, s: &mut State) -> bool { ... }
/// rg_run_software!(State, init, tick, builder = {
///     retroglyph::backend::software::SoftwareBackendBuilder::new()
///         .title(env!("CARGO_BIN_NAME"))
///         .grid_size(40, 15)
///         .scale(4)
/// });
/// ```
#[macro_export]
macro_rules! rg_run_software {
    ($State:ty, $init:expr, $tick:expr, builder = $builder:expr) => {
        #[allow(clippy::missing_const_for_fn, clippy::needless_pass_by_ref_mut)]
        fn __rg_init<B: ::retroglyph::Backend>(term: &mut ::retroglyph::Terminal<B>) -> $State {
            ($init)(term)
        }
        #[allow(clippy::missing_const_for_fn, clippy::needless_pass_by_ref_mut)]
        fn __rg_tick<B: ::retroglyph::Backend>(
            term: &mut ::retroglyph::Terminal<B>,
            state: &mut $State,
        ) -> bool {
            ($tick)(term, state)
        }

        #[cfg(feature = "software")]
        fn main() {
            #[cfg(target_arch = "wasm32")]
            ::console_error_panic_hook::set_once();

            let backend = { $builder }
                .build()
                .expect("failed to initialize software backend");

            let app = $crate::util::ClosureApp::new(__rg_init, __rg_tick);
            backend.run_app(app).expect("event loop failed");
        }

        #[cfg(all(feature = "software", target_arch = "wasm32"))]
        #[allow(missing_docs)]
        #[::wasm_bindgen::prelude::wasm_bindgen(start)]
        pub fn wasm_main() -> ::std::result::Result<(), ::wasm_bindgen::JsValue> {
            main();
            ::std::result::Result::Ok(())
        }

        // ── Headless fallback (software feature disabled) ───────────────────
        #[cfg(not(feature = "software"))]
        fn main() {
            $crate::util::run_headless(__rg_init, __rg_tick);
        }
    };
}
