//! Shared utilities for rg examples.

#![allow(unreachable_pub)]

pub mod action;
pub mod draw;
pub mod game;
pub mod lcg;
pub mod perf;
pub mod timestep;

/// Emit a `main` function wired to the enabled backend.
///
/// - When `software` is enabled (takes priority): launches the software
///   renderer. On `wasm32`, also emits a `wasm_bindgen(start)` entry point.
/// - When only `crossterm` is enabled: runs the crossterm terminal backend.
///
/// If neither backend feature is enabled the crate will fail to compile with
/// a missing entry point error — add `--features crossterm` or
/// `--features software-default-font`.
///
/// # Arguments
///
/// - `$State` — the game state type.
/// - `$init`  — callable as `$init(&mut Terminal<B>) -> $State`; called once.
/// - `$tick`  — callable as `$tick(&mut Terminal<B>, &mut $State) -> bool`;
///   return `false` to quit.
///
/// # Example
///
/// ```ignore
/// mod util;
/// struct State { ... }
/// fn init<B: retroglyph::Backend>(term: &mut retroglyph::Terminal<B>) -> State { ... }
/// fn tick<B: retroglyph::Backend>(term: &mut retroglyph::Terminal<B>, s: &mut State) -> bool { ... }
/// util::rg_run!(State, init, tick);
/// ```
#[macro_export]
macro_rules! rg_run {
    ($State:ty, $init:expr, $tick:expr) => {
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

            let mut state: ::std::option::Option<$State> = ::std::option::Option::None;
            let mut quit = false;
            backend
                .run_windowed(move |term| {
                    if quit {
                        return;
                    }
                    if state.is_none() {
                        state = ::std::option::Option::Some($init(&mut *term));
                    }
                    let s = state.as_mut().unwrap();
                    if !$tick(&mut *term, s) {
                        quit = true;
                        #[cfg(not(target_arch = "wasm32"))]
                        ::std::process::exit(0);
                    }
                })
                .expect("event loop failed");
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
            use ::retroglyph::Terminal;
            use ::retroglyph::backend::Crossterm;

            let backend = Crossterm::new()?;
            let mut term = Terminal::new(backend);
            let mut state = $init(&mut term);
            while $tick(&mut term, &mut state) {}
            ::std::result::Result::Ok(())
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
        #[cfg(feature = "software")]
        fn main() {
            #[cfg(target_arch = "wasm32")]
            ::console_error_panic_hook::set_once();

            let backend = { $builder }
                .build()
                .expect("failed to initialize software backend");

            let mut state: ::std::option::Option<$State> = ::std::option::Option::None;
            let mut quit = false;
            backend
                .run_windowed(move |term| {
                    if quit {
                        return;
                    }
                    if state.is_none() {
                        state = ::std::option::Option::Some($init(&mut *term));
                    }
                    let s = state.as_mut().unwrap();
                    if !$tick(&mut *term, s) {
                        quit = true;
                        #[cfg(not(target_arch = "wasm32"))]
                        ::std::process::exit(0);
                    }
                })
                .expect("event loop failed");
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
