//! Shared utilities for rg examples.

#![allow(unreachable_pub, dead_code)]

pub mod action;
pub mod fov;
pub mod game;
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
/// [`Headless`]: retroglyph_core::Headless
#[doc(hidden)]
pub fn run_headless<S>(
    mut init: impl FnMut(&mut retroglyph_core::Terminal<retroglyph_core::Headless>) -> S,
    mut tick: impl FnMut(&mut retroglyph_core::Terminal<retroglyph_core::Headless>, &mut S) -> bool,
) {
    let frames: u32 = std::env::var("RG_HEADLESS_FRAMES")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(3);

    let backend = retroglyph_core::Headless::new(50, 25);
    let mut term = retroglyph_core::Terminal::new(backend);
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

/// WASM-only support for driving a [`Terminal<Headless>`](retroglyph_core::Terminal)
/// from a browser `requestAnimationFrame` loop instead of a canvas/window.
///
/// See the `wasm-headless` branch of [`rg_run!`] for the generated
/// `#[wasm_bindgen]` entry points. This module only holds the pieces that
/// don't need to be generated per-example: the FFI key decoder and the
/// shared state container the generated code stores in a `thread_local`.
///
/// Gated on the `wasm-headless` feature only (not also `target_arch =
/// "wasm32"`): [`decode_key`] itself has no wasm-only dependencies, so it
/// stays testable on the host target. Only the generated `#[wasm_bindgen]`
/// entry points in [`wasm_headless_entry!`](crate::wasm_headless_entry) are
/// wasm32-only.
#[cfg(feature = "wasm-headless")]
pub mod wasm_headless {
    /// Decode an FFI-friendly `(code, mods)` pair into a
    /// [`KeyEvent`](retroglyph_core::event::KeyEvent).
    ///
    /// `retroglyph_core::event` types are not `wasm_bindgen`-friendly (no
    /// `#[wasm_bindgen]` on the enum, no stable C-like repr contract), so the
    /// browser side calls in with two plain integers instead of a rich event
    /// type crossing the FFI boundary directly:
    ///
    /// - `code`: for printable characters, the Unicode scalar value (as you'd
    ///   get from JS `event.key.codePointAt(0)` for a single-codepoint key).
    ///   Special keys use codepoints above `0x0010_FFFF` (outside the valid
    ///   `char` range), one per [`KeyCode`](retroglyph_core::event::KeyCode)
    ///   variant that isn't `Char`/`F`. Function keys `F1..=F24` use
    ///   `SPECIAL_F0 + n`.
    /// - `mods`: a bitmask matching
    ///   [`KeyModifiers`](retroglyph_core::event::KeyModifiers)'s internal
    ///   layout: bit 0 = shift, bit 1 = control, bit 2 = alt.
    ///
    /// Returns `None` for codes that don't map to a known key (the caller
    /// should silently drop the event rather than panic — a malformed or
    /// future JS build shouldn't be able to crash the wasm module).
    #[must_use]
    pub fn decode_key(code: u32, mods: u8) -> Option<retroglyph_core::event::KeyEvent> {
        use retroglyph_core::event::{KeyCode, KeyEvent, KeyModifiers};

        // Special keys live just above the valid `char` range (0x0 ..=
        // 0x10_FFFF, minus the surrogate gap) so they can never collide with
        // a real codepoint coming from `codePointAt`.
        const SPECIAL_BASE: u32 = 0x0011_0000;
        const BACKSPACE: u32 = SPECIAL_BASE;
        const ENTER: u32 = SPECIAL_BASE + 1;
        const LEFT: u32 = SPECIAL_BASE + 2;
        const RIGHT: u32 = SPECIAL_BASE + 3;
        const UP: u32 = SPECIAL_BASE + 4;
        const DOWN: u32 = SPECIAL_BASE + 5;
        const HOME: u32 = SPECIAL_BASE + 6;
        const END: u32 = SPECIAL_BASE + 7;
        const PAGE_UP: u32 = SPECIAL_BASE + 8;
        const PAGE_DOWN: u32 = SPECIAL_BASE + 9;
        const TAB: u32 = SPECIAL_BASE + 10;
        const BACK_TAB: u32 = SPECIAL_BASE + 11;
        const DELETE: u32 = SPECIAL_BASE + 12;
        const INSERT: u32 = SPECIAL_BASE + 13;
        const ESCAPE: u32 = SPECIAL_BASE + 14;
        const SPECIAL_F0: u32 = SPECIAL_BASE + 100;

        let key_code = match code {
            BACKSPACE => KeyCode::Backspace,
            ENTER => KeyCode::Enter,
            LEFT => KeyCode::Left,
            RIGHT => KeyCode::Right,
            UP => KeyCode::Up,
            DOWN => KeyCode::Down,
            HOME => KeyCode::Home,
            END => KeyCode::End,
            PAGE_UP => KeyCode::PageUp,
            PAGE_DOWN => KeyCode::PageDown,
            TAB => KeyCode::Tab,
            BACK_TAB => KeyCode::BackTab,
            DELETE => KeyCode::Delete,
            INSERT => KeyCode::Insert,
            ESCAPE => KeyCode::Escape,
            SPECIAL_F0..=0xFFFF_FFFF if u8::try_from(code - SPECIAL_F0).is_ok() =>
            {
                #[allow(clippy::cast_possible_truncation)]
                KeyCode::F((code - SPECIAL_F0) as u8)
            }
            c => KeyCode::Char(char::from_u32(c)?),
        };

        let mut modifiers = KeyModifiers::NONE;
        if mods & 0b001 != 0 {
            modifiers |= KeyModifiers::SHIFT;
        }
        if mods & 0b010 != 0 {
            modifiers |= KeyModifiers::CONTROL;
        }
        if mods & 0b100 != 0 {
            modifiers |= KeyModifiers::ALT;
        }

        Some(KeyEvent::new(key_code, modifiers))
    }

    #[cfg(test)]
    mod tests {
        use super::decode_key;
        use retroglyph_core::event::{KeyCode, KeyModifiers};

        #[test]
        fn decodes_plain_char() {
            let ev = decode_key(u32::from('a'), 0).unwrap();
            assert_eq!(ev.code, KeyCode::Char('a'));
            assert_eq!(ev.modifiers, KeyModifiers::NONE);
        }

        #[test]
        fn decodes_arrow_keys() {
            assert_eq!(decode_key(0x0011_0002, 0).unwrap().code, KeyCode::Left);
            assert_eq!(decode_key(0x0011_0003, 0).unwrap().code, KeyCode::Right);
            assert_eq!(decode_key(0x0011_0004, 0).unwrap().code, KeyCode::Up);
            assert_eq!(decode_key(0x0011_0005, 0).unwrap().code, KeyCode::Down);
        }

        #[test]
        fn decodes_escape() {
            assert_eq!(decode_key(0x0011_000e, 0).unwrap().code, KeyCode::Escape);
        }

        #[test]
        fn decodes_modifiers() {
            let ev = decode_key(u32::from('c'), 0b011).unwrap();
            assert!(ev.modifiers.contains(KeyModifiers::SHIFT));
            assert!(ev.modifiers.contains(KeyModifiers::CONTROL));
            assert!(!ev.modifiers.contains(KeyModifiers::ALT));
        }

        #[test]
        fn decodes_function_keys() {
            assert_eq!(decode_key(0x0011_0064, 0).unwrap().code, KeyCode::F(0));
            assert_eq!(decode_key(0x0011_006e, 0).unwrap().code, KeyCode::F(10));
        }

        #[test]
        fn rejects_surrogate_codepoint() {
            // 0xD800 is a lone surrogate half; not a valid `char`, and below
            // SPECIAL_BASE, so it should fail to decode rather than panic.
            assert!(decode_key(0xD800, 0).is_none());
        }
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
/// [`App`]: retroglyph_core::App
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

impl<B, S, I, T> retroglyph_core::App<B> for ClosureApp<S, I, T>
where
    B: retroglyph_core::Backend,
    I: FnMut(&mut retroglyph_core::Terminal<B>) -> S,
    T: FnMut(&mut retroglyph_core::Terminal<B>, &mut S) -> bool,
{
    fn update(
        &mut self,
        term: &mut retroglyph_core::Terminal<B>,
        _frame: &retroglyph_core::Frame,
    ) -> retroglyph_core::Flow {
        if self.state.is_none() {
            self.state = Some((self.init)(term));
        }
        let state = self.state.as_mut().expect("state initialized above");
        if (self.tick)(term, state) {
            retroglyph_core::Flow::Continue
        } else {
            retroglyph_core::Flow::Exit
        }
    }
}

/// Emit a `main` function wired to the enabled backend, driving an [`App`]
/// built from `init` + `tick` closures.
///
/// - When `software` is enabled (takes priority): builds a `SoftwareRenderer`,
///   sizes a window with `WindowConfig::fit`, and runs `retroglyph-window`'s
///   inverted driver. On `wasm32`, also emits a `wasm_bindgen(start)` entry.
/// - When only `crossterm` is enabled: runs the generic blocking driver.
/// - When neither is enabled: falls back to [`run_headless`], ticking a few
///   frames against a [`Headless`](retroglyph_core::Headless) backend and
///   printing them to stdout. This keeps every example buildable with the
///   crate's default features and backs `examples/runner.rs`'s "Headless"
///   backend option.
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
/// struct State { /* ... */ }
/// fn init<B: retroglyph_core::Backend>(t: &mut retroglyph_core::Terminal<B>) -> State { todo!() }
/// fn tick<B: retroglyph_core::Backend>(t: &mut retroglyph_core::Terminal<B>, s: &mut State) -> bool { todo!() }
/// retroglyph_examples::rg_run!(State, init, tick);
/// ```
#[macro_export]
macro_rules! rg_run {
    ($State:ty, $init:expr, $tick:expr) => {
        // Wrap `$init`/`$tick` in named generic fns. Named fns are properly
        // higher-ranked over the `&mut Terminal<B>` lifetime, whereas a bare
        // closure that ignores its argument (e.g. `|_t| State::new()`) fails
        // HRTB inference when stored in the App adapter.
        #[allow(clippy::missing_const_for_fn, clippy::needless_pass_by_ref_mut)]
        fn __rg_init<B: ::retroglyph_core::Backend>(
            term: &mut ::retroglyph_core::Terminal<B>,
        ) -> $State {
            ($init)(term)
        }
        #[allow(clippy::missing_const_for_fn, clippy::needless_pass_by_ref_mut)]
        fn __rg_tick<B: ::retroglyph_core::Backend>(
            term: &mut ::retroglyph_core::Terminal<B>,
            state: &mut $State,
        ) -> bool {
            ($tick)(term, state)
        }

        // ── Software backend (desktop + WASM) ─────────────────────────────
        #[cfg(feature = "software")]
        fn main() {
            #[cfg(target_arch = "wasm32")]
            ::console_error_panic_hook::set_once();

            let renderer = ::retroglyph_software::SoftwareBackendBuilder::new()
                .grid_size(50, 25)
                .scale(2)
                .build()
                .expect("failed to initialize software backend")
                .run_headless();
            let config = ::retroglyph_window::winit::WindowConfig::fit(
                &renderer,
                env!("CARGO_BIN_NAME"),
                None,
            );
            let app = $crate::util::ClosureApp::new(__rg_init, __rg_tick);
            ::retroglyph_window::winit::run_app(config, renderer, app).expect("event loop failed");
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
            ::retroglyph_crossterm::Crossterm::run(app)
        }

        // ── WASM Headless backend (browser rAF loop, no canvas/window) ────
        //
        // Takes priority over the stdout-printing Headless fallback below,
        // but yields to `software` if both feature flags are somehow enabled
        // at once (see the Cargo.toml doc comment on `wasm-headless`).
        #[cfg(all(
            feature = "wasm-headless",
            not(feature = "software"),
            target_arch = "wasm32"
        ))]
        ::retroglyph_examples::wasm_headless_entry!($State, __rg_init, __rg_tick);

        // ── Headless fallback (no backend feature enabled, or non-wasm) ───
        #[cfg(not(any(
            feature = "crossterm",
            feature = "software",
            all(feature = "wasm-headless", target_arch = "wasm32"),
        )))]
        fn main() {
            $crate::util::run_headless(__rg_init, __rg_tick);
        }
    };
}

/// Emit the `#[wasm_bindgen]` entry points for the `wasm-headless` branch of
/// [`rg_run!`].
///
/// Broken out into its own macro (rather than inlined in `rg_run!`) because it
/// needs a `thread_local!` to stash the `Terminal<Headless>` + state between
/// calls from JS — there is no `main`-owned stack frame to hold it in, unlike
/// the blocking crossterm driver or the winit-owned software driver. wasm32 is
/// single-threaded (no shared-memory threads without extra toolchain setup),
/// so a `thread_local!` behaves like a plain global here without `unsafe`.
///
/// Exposes three `#[wasm_bindgen]` functions to JS:
///
/// - `wasm_headless_init()` — builds the `Terminal<Headless>` and calls
///   `$init` once. Call this before the first tick.
/// - `wasm_headless_push_key(code: u32, mods: u8)` — decodes and queues a key
///   event via [`wasm_headless::decode_key`]. Silently drops undecodable
///   codes.
/// - `wasm_headless_tick() -> String` — calls `$tick` once (draining any
///   queued key events first) and returns
///   [`Headless::format_view`](retroglyph_core::Headless::format_view) of the
///   freshly rendered frame. The browser side calls this once per
///   `requestAnimationFrame` (or `setInterval`) tick and writes the returned
///   string into a `<pre>`/`<textarea>`.
///
/// Grid size is fixed at 50x25 to match [`run_headless`] and the plain
/// `headless` example.
#[macro_export]
macro_rules! wasm_headless_entry {
    ($State:ty, $init:path, $tick:path) => {
        struct __WasmHeadlessState {
            term: ::retroglyph_core::Terminal<::retroglyph_core::Headless>,
            state: $State,
        }

        ::std::thread_local! {
            static __WASM_HEADLESS: ::std::cell::RefCell<::std::option::Option<__WasmHeadlessState>> =
                ::std::cell::RefCell::new(::std::option::Option::None);
        }

        /// Build the `Terminal<Headless>` and run `$init` once. Call before
        /// the first `wasm_headless_tick`.
        #[::wasm_bindgen::prelude::wasm_bindgen]
        #[allow(missing_docs)]
        pub fn wasm_headless_init() {
            ::console_error_panic_hook::set_once();
            let backend = ::retroglyph_core::Headless::new(50, 25);
            let mut term = ::retroglyph_core::Terminal::new(backend);
            let state = $init(&mut term);
            __WASM_HEADLESS.with(|cell| {
                *cell.borrow_mut() = ::std::option::Option::Some(__WasmHeadlessState { term, state });
            });
        }

        /// Decode and queue a key event. `code`/`mods` are the FFI encoding
        /// documented on `wasm_headless::decode_key`. Codes that don't decode
        /// to a known key are silently dropped.
        #[::wasm_bindgen::prelude::wasm_bindgen]
        #[allow(missing_docs)]
        pub fn wasm_headless_push_key(code: u32, mods: u8) {
            let Some(event) = $crate::util::wasm_headless::decode_key(code, mods) else {
                return;
            };
            __WASM_HEADLESS.with(|cell| {
                if let ::std::option::Option::Some(s) = cell.borrow_mut().as_mut() {
                    s.term.backend_mut().push_event(::retroglyph_core::event::Event::Key(event));
                }
            });
        }

        /// Run one tick of the game loop and return the rendered frame as a
        /// plain string (space cells shown as `·`), suitable for direct
        /// assignment into a `<pre>`/`<textarea>`'s content.
        ///
        /// Returns an empty string if called before `wasm_headless_init`.
        #[::wasm_bindgen::prelude::wasm_bindgen]
        #[allow(missing_docs)]
        pub fn wasm_headless_tick() -> ::std::string::String {
            __WASM_HEADLESS.with(|cell| {
                let mut guard = cell.borrow_mut();
                let Some(s) = guard.as_mut() else {
                    return ::std::string::String::new();
                };
                $tick(&mut s.term, &mut s.state);
                s.term.backend().format_view()
            })
        }

        // `cargo build --target wasm32-unknown-unknown --example ...` still
        // requires a `main` symbol to exist even though JS never calls it for
        // this entry mode — it only calls the three `#[wasm_bindgen]`
        // functions above. No `#[wasm_bindgen(start)]` here: unlike the
        // software backend, there is no event loop to kick off at module-load
        // time, so we deliberately leave it unmarked.
        fn main() {}
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
/// The builder's `.title(...)` (if any) is ignored for windowing; the window
/// title comes from `CARGO_BIN_NAME` via `WindowConfig::fit`.
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
/// struct State { /* ... */ }
/// fn init<B: retroglyph_core::Backend>(t: &mut retroglyph_core::Terminal<B>) -> State { todo!() }
/// fn tick<B: retroglyph_core::Backend>(t: &mut retroglyph_core::Terminal<B>, s: &mut State) -> bool { todo!() }
/// retroglyph_examples::rg_run_software!(State, init, tick, builder = {
///     retroglyph_software::SoftwareBackendBuilder::new()
///         .grid_size(40, 15)
///         .scale(4)
/// });
/// ```
#[macro_export]
macro_rules! rg_run_software {
    ($State:ty, $init:expr, $tick:expr, builder = $builder:expr) => {
        #[allow(clippy::missing_const_for_fn, clippy::needless_pass_by_ref_mut)]
        fn __rg_init<B: ::retroglyph_core::Backend>(
            term: &mut ::retroglyph_core::Terminal<B>,
        ) -> $State {
            ($init)(term)
        }
        #[allow(clippy::missing_const_for_fn, clippy::needless_pass_by_ref_mut)]
        fn __rg_tick<B: ::retroglyph_core::Backend>(
            term: &mut ::retroglyph_core::Terminal<B>,
            state: &mut $State,
        ) -> bool {
            ($tick)(term, state)
        }

        #[cfg(feature = "software")]
        fn main() {
            #[cfg(target_arch = "wasm32")]
            ::console_error_panic_hook::set_once();

            let renderer = { $builder }
                .build()
                .expect("failed to initialize software backend")
                .run_headless();
            let config = ::retroglyph_window::winit::WindowConfig::fit(
                &renderer,
                env!("CARGO_BIN_NAME"),
                None,
            );
            let app = $crate::util::ClosureApp::new(__rg_init, __rg_tick);
            ::retroglyph_window::winit::run_app(config, renderer, app).expect("event loop failed");
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
