//! Shared utilities for rg examples.

#![allow(unreachable_pub, dead_code)]

pub mod action;
pub mod draw;
pub mod fov;
pub mod game;
pub mod lcg;
pub mod perf;
pub mod timestep;

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
///
/// If neither backend feature is enabled the crate fails to compile with a
/// missing entry point error — add `--features crossterm` or
/// `--features software-default-font`.
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
    };
}

/// Emit a software-only `main` function using a caller-supplied
/// [`SoftwareBackendBuilder`] expression.
///
/// Use this for examples that need custom grid dimensions, scale, tilesets,
/// or any other builder option that differs from `rg_run!`'s defaults.
/// Unlike `rg_run!`, no crossterm branch is emitted — these examples only run
/// on the desktop or WASM software renderer.
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
    };
}
