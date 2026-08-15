//! `app_entry!`'s own doc comment (below) is what renders publicly: `#[macro_export]` lifts a
//! macro to the crate root regardless of the declaring module's visibility, and this module is
//! deliberately kept private, the same pattern `examples/src/wasm_entry.rs` uses for its own
//! `Example`-based equivalents. Nothing here needs its own rustdoc page.

/// Emits the `wasm-bindgen` FFI surface driving `$A: App<TerminalWasm> + Default` from a browser
/// terminal emulator (e.g. xterm.js), on `wasm32` only.
///
/// `examples/src/wasm_entry.rs`'s `__wasm_terminal_entry!` does the same job for the examples
/// crate's private `Example` trait, but that crate is `publish = false`, so nothing outside this
/// repo can reach it (retroglyph#684). This macro is the generally-usable version: generic over
/// [`App`](retroglyph_core::app::App) (public, stable, and already the update contract every other
/// driver in `retroglyph-core` shares), not `Example`.
///
/// Call it once, at the top level of a `wasm32` binary crate that depends on this crate and
/// `retroglyph-core`:
///
/// ```ignore
/// #[derive(Default)]
/// struct MyGame { /* ... */ }
///
/// impl retroglyph_core::app::App<retroglyph_terminal_wasm::TerminalWasm> for MyGame {
///     fn update(
///         &mut self,
///         term: &mut retroglyph_core::terminal::Terminal<retroglyph_terminal_wasm::TerminalWasm>,
///         frame: &retroglyph_core::app::Frame,
///     ) -> retroglyph_core::app::Flow {
///         // ...
///         retroglyph_core::app::Flow::Continue
///     }
/// }
///
/// retroglyph_terminal_wasm::app_entry!(MyGame);
///
/// fn main() {}
/// ```
///
/// Expands to nothing at all off `wasm32` (a native build of the same crate just doesn't get this
/// FFI surface, since nothing would call it).
///
/// Exports, all thread-local and single-instance (one `$A` per page; construct a fresh
/// handle-based session per instance instead via this crate's `wasm` module, only compiled for
/// `target_arch = "wasm32"`, if a page needs more than one):
///
/// - `wasm_app_init(width, height)`: builds the `Terminal<TerminalWasm>` at the given size (in
///   cells) and `$A::default()`. Call once, before the first tick, after sizing the host terminal
///   emulator (e.g. xterm.js's `fitAddon.fit()`).
/// - `wasm_app_resize(width, height)`: reports a new size (in cells) via
///   [`resize_terminal`](crate::resize_terminal), so the driven `$A` sees the matching
///   `Event::Resize` on its next `update`, not just a backend that silently changed size under it.
/// - `wasm_app_push_key(code, mods)` / `wasm_app_push_mouse(x, y, action, button, mods)`: decode
///   and queue input via [`decode_key_event`](crate::decode_key_event)/
///   [`decode_mouse_event`](crate::decode_mouse_event).
/// - `wasm_app_push_paste(text)`: queues `text` as a single `Event::Paste`.
/// - `wasm_app_push_focus(focused)`: queues `Event::FocusGained`/`Event::FocusLost`.
/// - `wasm_app_tick() -> String`: runs one `App::update`, presents unless it returned
///   `Flow::Idle` (or already presented itself), and returns the ANSI bytes rendered since the
///   last call, the same contract [`TerminalWasm::take_output`](crate::TerminalWasm::take_output)
///   documents. `Frame::delta` is wall-clock time since the previous tick, clamped to
///   `MAX_TICK_DELTA` (250ms): a backgrounded tab can starve `requestAnimationFrame` for seconds
///   or minutes, and an uncapped delta handed straight to an animation/physics step would try to
///   simulate that entire gap in one frame (the same "spiral of death" concern
///   [`FrameClock`](retroglyph_core::frames::FrameClock) caps steps-per-frame to avoid), just on the raw
///   delta feeding into `Frame` instead. All FFI functions are no-ops (returning an empty string
///   for `wasm_app_tick`) if called before `wasm_app_init`.
/// - `wasm_app_exited() -> bool`: `true` once `$A::update` has returned `Flow::Exit` at least
///   once. A browser tab has no native "exit the process" the way a windowed backend's event loop
///   does, so this crate can't stop JS's `requestAnimationFrame` loop for it; check this after
///   `wasm_app_tick` and stop calling it once it flips `true`, e.g. to show a fixed "Game Over"
///   frame's own draw already put on screen. `wasm_app_tick` keeps calling `$A::update` (and,
///   correctly, doing nothing useful) if the caller ignores this rather than panicking or hanging.
#[macro_export]
macro_rules! app_entry {
    ($A:ty) => {
        #[cfg(target_arch = "wasm32")]
        const _: () = {
            /// A backgrounded tab can starve `requestAnimationFrame` for an arbitrary amount of
            /// wall-clock time; capping the delta handed to `App::update` keeps one resumed frame
            /// from asking an animation/physics step to simulate that entire gap at once. See
            /// this macro's own doc comment for the full rationale.
            const MAX_TICK_DELTA: ::core::time::Duration = ::core::time::Duration::from_millis(250);

            struct __RgWasmAppState {
                term: ::retroglyph_core::terminal::Terminal<$crate::TerminalWasm>,
                app: $A,
                last_tick: ::web_time::Instant,
                frame_count: u64,
                exited: bool,
            }

            ::std::thread_local! {
                static __RG_WASM_APP: ::std::cell::RefCell<::std::option::Option<__RgWasmAppState>> =
                    ::std::cell::RefCell::new(::std::option::Option::None);
            }

            /// Runs `f` against the live `__RgWasmAppState`, or does nothing and returns `None` if
            /// called before `wasm_app_init`.
            fn with_state<R>(
                f: impl ::std::ops::FnOnce(&mut __RgWasmAppState) -> R,
            ) -> ::std::option::Option<R> {
                __RG_WASM_APP.with(|cell| cell.borrow_mut().as_mut().map(f))
            }

            /// Queues `event` on the live state's backend. A no-op before `wasm_app_init`.
            fn push(event: ::retroglyph_core::event::Event) {
                with_state(|s| {
                    ::retroglyph_core::backend::Input::push_event(s.term.backend_mut(), event);
                });
            }

            /// Build the `Terminal<TerminalWasm>` and `$A::default()`. Call before the first
            /// `wasm_app_tick`.
            #[::wasm_bindgen::prelude::wasm_bindgen]
            #[allow(missing_docs)]
            pub fn wasm_app_init(width: u16, height: u16) {
                ::console_error_panic_hook::set_once();
                let mut backend = $crate::TerminalWasm::new(width, height);
                ::retroglyph_core::backend::Cursor::set_cursor_visible(&mut backend, false);
                let term = ::retroglyph_core::terminal::Terminal::new(backend);
                __RG_WASM_APP.with(|cell| {
                    *cell.borrow_mut() = ::std::option::Option::Some(__RgWasmAppState {
                        term,
                        app: <$A as ::std::default::Default>::default(),
                        last_tick: ::web_time::Instant::now(),
                        frame_count: 0,
                        exited: false,
                    });
                });
            }

            /// Report a new size (in cells), e.g. after the host terminal emulator re-fits on a
            /// window resize. No-op if called before `wasm_app_init`.
            #[::wasm_bindgen::prelude::wasm_bindgen]
            #[allow(missing_docs)]
            pub fn wasm_app_resize(width: u16, height: u16) {
                with_state(|s| $crate::resize_terminal(&mut s.term, width, height));
            }

            /// Decode and queue a key event via [`decode_key_event`](crate::decode_key_event).
            #[::wasm_bindgen::prelude::wasm_bindgen]
            #[allow(missing_docs)]
            pub fn wasm_app_push_key(code: u32, mods: u8) {
                let Some(event) = $crate::decode_key_event(code, mods) else {
                    return;
                };
                push(::retroglyph_core::event::Event::Key(event));
            }

            /// Decode and queue a pointer (mouse/touch) event via
            /// [`decode_mouse_event`](crate::decode_mouse_event).
            #[::wasm_bindgen::prelude::wasm_bindgen]
            #[allow(missing_docs)]
            pub fn wasm_app_push_mouse(x: u16, y: u16, action: u8, button: u8, mods: u8) {
                let Some(event) = $crate::decode_mouse_event(x, y, action, button, mods) else {
                    return;
                };
                push(::retroglyph_core::event::Event::Mouse(event));
            }

            /// Queue pasted text as a single `Event::Paste`, not one `Event::Key` per character.
            #[::wasm_bindgen::prelude::wasm_bindgen]
            #[allow(missing_docs)]
            pub fn wasm_app_push_paste(text: ::std::string::String) {
                push(::retroglyph_core::event::Event::Paste(text));
            }

            /// Queue a focus-change event: `true` for `Event::FocusGained`, `false` for
            /// `Event::FocusLost`.
            #[::wasm_bindgen::prelude::wasm_bindgen]
            #[allow(missing_docs)]
            pub fn wasm_app_push_focus(focused: bool) {
                let event = if focused {
                    ::retroglyph_core::event::Event::FocusGained
                } else {
                    ::retroglyph_core::event::Event::FocusLost
                };
                push(event);
            }

            /// Run one `App::update`, present unless it returned `Flow::Idle` (or already
            /// presented itself), and return the ANSI bytes rendered since the last call. Returns
            /// an empty string if called before `wasm_app_init`.
            #[::wasm_bindgen::prelude::wasm_bindgen]
            #[must_use]
            #[allow(missing_docs)]
            pub fn wasm_app_tick() -> ::std::string::String {
                __RG_WASM_APP.with(|cell| {
                    let mut guard = cell.borrow_mut();
                    let Some(s) = guard.as_mut() else {
                        return ::std::string::String::new();
                    };
                    let now = ::web_time::Instant::now();
                    let delta = ::std::cmp::min(now.duration_since(s.last_tick), MAX_TICK_DELTA);
                    s.last_tick = now;
                    let frame = ::retroglyph_core::app::Frame::new(delta, s.frame_count);
                    s.frame_count = s.frame_count.wrapping_add(1);
                    let present_count_before = s.term.present_count();
                    let flow = ::retroglyph_core::app::App::update(&mut s.app, &mut s.term, &frame);
                    if flow == ::retroglyph_core::app::Flow::Exit {
                        s.exited = true;
                    }
                    if flow != ::retroglyph_core::app::Flow::Idle
                        && s.term.present_count() == present_count_before
                    {
                        let _ = s.term.present();
                    }
                    s.term.backend_mut().take_output()
                })
            }

            /// `true` once `$A::update` has returned `Flow::Exit` at least once. See this
            /// macro's own doc comment for why the caller (not this crate) decides what "exit"
            /// means for a browser tab.
            #[::wasm_bindgen::prelude::wasm_bindgen]
            #[must_use]
            #[allow(missing_docs)]
            pub fn wasm_app_exited() -> bool {
                __RG_WASM_APP.with(|cell| {
                    cell.borrow()
                        .as_ref()
                        .is_some_and(|s| s.exited)
                })
            }

            // Required symbol for the wasm32 binary target; JS never calls it directly in this
            // entry mode (no event loop to kick off at module-load time; everything is pushed
            // in from JS instead). Matches the equivalent comment on
            // `examples::__wasm_terminal_entry!`.
            fn main() {}
        };
    };
}
