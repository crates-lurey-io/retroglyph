//! `wasm_entry!`: the one bit of per-example codegen `launch::<E>()` can't
//! replace.
//!
//! `wasm-bindgen` needs concrete, statically-named exported functions -- it
//! cannot attach to a function that's still generic over `E: Example` (there
//! is no such thing as a generic FFI symbol). Every other backend is a plain
//! call to [`launch::<E>()`](crate::launch), but the WASM entry points
//! genuinely have to be generated per example, once `E` is a concrete type.
//! This macro is deliberately the only piece of `macro_rules!` codegen left
//! in this crate.

/// Emits `fn main()` (calling [`launch::<$E>()`](crate::launch)) and the
/// [`wasm_entry!`] FFI surface in one call -- the usual way to close out an
/// example:
///
/// ```ignore
/// retroglyph_examples::example_main!(HelloWorld);
/// ```
///
/// is equivalent to writing both by hand:
///
/// ```ignore
/// fn main() {
///     retroglyph_examples::launch::<HelloWorld>();
/// }
/// retroglyph_examples::wasm_entry!(HelloWorld);
/// ```
///
/// Write the two out separately instead if an example needs a non-default
/// `main` body (it still needs to call `wasm_entry!($E)` itself afterward --
/// see [`wasm_entry!`]'s software-branch doc comment for why the ordering
/// matters: that branch's generated shim calls the example's own `main`).
#[macro_export]
macro_rules! example_main {
    ($E:ty) => {
        fn main() {
            $crate::launch::<$E>();
        }
        $crate::wasm_entry!($E);
    };
}

/// Emits the `wasm-bindgen` FFI surface for `$E: Example` on `wasm32`.
///
/// Call this once, at the top level of an example, alongside its `fn main()`
/// (or use [`example_main!`] to emit both together):
///
/// ```ignore
/// fn main() {
///     retroglyph_examples::launch::<HelloWorld>();
/// }
/// retroglyph_examples::wasm_entry!(HelloWorld);
/// ```
///
/// Expands to nothing at all off `wasm32`, and to nothing for a feature
/// combination it doesn't recognize (mirrors `launch!`'s fallback: an
/// example built with no wasm-capable feature just doesn't get any FFI
/// surface, which is correct since nothing would call it).
///
/// - `software` or `gl` (either wins if enabled): a `#[wasm_bindgen(start)]`
///   shim that calls the example's own `main()`. Both `run_software::<E>()`
///   and `run_gl::<E>()` are already portable to `wasm32` via winit (canvas +
///   WebGL2 for `gl`); they just need something to invoke them when the
///   module loads.
/// - `wasm-headless` (if `software`/`gl` are off): drives a
///   `Terminal<Headless>` from a browser `requestAnimationFrame` loop. See
///   [`wasm_headless`](crate::util::wasm_headless) for the FFI key decoder.
/// - `wasm-terminal` (if none of the above is on): drives a
///   `Terminal<TerminalWasm>` from a browser terminal emulator (e.g.
///   xterm.js).
#[macro_export]
macro_rules! wasm_entry {
    ($E:ty) => {
        // Both windowed backends (software canvas, gl WebGL2) run their winit event loop from
        // `main()`; this shim is the module-load hook that invokes it.
        #[cfg(all(any(feature = "software", feature = "gl"), target_arch = "wasm32"))]
        #[allow(missing_docs)]
        #[::wasm_bindgen::prelude::wasm_bindgen(start)]
        pub fn __rg_wasm_start() -> ::std::result::Result<(), ::wasm_bindgen::JsValue> {
            main();
            ::std::result::Result::Ok(())
        }

        $crate::__wasm_headless_entry!($E);
        $crate::__wasm_terminal_entry!($E);
    };
}

/// The `wasm-headless` arm of [`wasm_entry!`]: drives a
/// `Terminal<Headless>` from browser-pushed FFI calls instead of a
/// canvas/window.
///
/// Broken out of [`wasm_entry!`] only so that macro stays a short, readable
/// dispatch table; not meant to be called directly.
#[doc(hidden)]
#[macro_export]
macro_rules! __wasm_headless_entry {
    ($E:ty) => {
        #[cfg(all(
            feature = "wasm-headless",
            not(any(feature = "software", feature = "gl")),
            target_arch = "wasm32"
        ))]
        const _: () = {
            struct __RgWasmHeadlessState {
                term: ::retroglyph_core::Terminal<::retroglyph_core::Headless>,
                state: $E,
                last_tick: ::web_time::Instant,
                frame_count: u64,
            }

            ::std::thread_local! {
                static __RG_WASM_HEADLESS: ::std::cell::RefCell<::std::option::Option<__RgWasmHeadlessState>> =
                    ::std::cell::RefCell::new(::std::option::Option::None);
            }

            /// Build the `Terminal<Headless>` and run `$E::init` once. Call
            /// before the first `wasm_headless_tick`.
            #[::wasm_bindgen::prelude::wasm_bindgen]
            #[allow(missing_docs)]
            pub fn wasm_headless_init() {
                ::console_error_panic_hook::set_once();
                let backend = ::retroglyph_core::Headless::new(50, 25);
                let mut term = ::retroglyph_core::Terminal::new(backend);
                let state = <$E as $crate::Example>::init(&mut term);
                __RG_WASM_HEADLESS.with(|cell| {
                    *cell.borrow_mut() = ::std::option::Option::Some(__RgWasmHeadlessState {
                        term,
                        state,
                        last_tick: ::web_time::Instant::now(),
                        frame_count: 0,
                    });
                });
            }

            /// Decode and queue a key event. `code`/`mods` are the FFI
            /// encoding documented on
            /// [`wasm_headless::decode_key`](crate::util::wasm_headless::decode_key).
            #[::wasm_bindgen::prelude::wasm_bindgen]
            #[allow(missing_docs)]
            pub fn wasm_headless_push_key(code: u32, mods: u8) {
                let Some(event) = $crate::util::wasm_headless::decode_key(code, mods) else {
                    return;
                };
                __RG_WASM_HEADLESS.with(|cell| {
                    if let ::std::option::Option::Some(s) = cell.borrow_mut().as_mut() {
                        s.term.backend_mut().push_event(::retroglyph_core::event::Event::Key(event));
                    }
                });
            }

            /// Decode and queue a pointer (mouse/touch) event. See
            /// [`wasm_pointer::decode_mouse`](crate::util::wasm_pointer::decode_mouse)
            /// for the `(x, y, kind)` encoding.
            #[::wasm_bindgen::prelude::wasm_bindgen]
            #[allow(missing_docs)]
            pub fn wasm_headless_push_mouse(x: u16, y: u16, kind: u8) {
                let Some(event) = $crate::util::wasm_pointer::decode_mouse(x, y, kind) else {
                    return;
                };
                __RG_WASM_HEADLESS.with(|cell| {
                    if let ::std::option::Option::Some(s) = cell.borrow_mut().as_mut() {
                        s.term.backend_mut().push_event(event);
                    }
                });
            }

            /// Run one tick and return the rendered frame as plain text
            /// (space cells shown as `·`), suitable for direct assignment
            /// into a `<pre>`/`<textarea>`. Returns an empty string if
            /// called before `wasm_headless_init`.
            #[::wasm_bindgen::prelude::wasm_bindgen]
            #[allow(missing_docs)]
            pub fn wasm_headless_tick() -> ::std::string::String {
                __RG_WASM_HEADLESS.with(|cell| {
                    let mut guard = cell.borrow_mut();
                    let Some(s) = guard.as_mut() else {
                        return ::std::string::String::new();
                    };
                    let now = ::web_time::Instant::now();
                    let frame = ::retroglyph_core::Frame {
                        delta: now.duration_since(s.last_tick),
                        frame: s.frame_count,
                    };
                    s.last_tick = now;
                    s.frame_count = s.frame_count.wrapping_add(1);
                    $crate::Example::tick(&mut s.state, &mut s.term, &frame);
                    s.term.backend().format_view()
                })
            }

            // Required symbol for the wasm32 binary target; JS never calls
            // it directly in this entry mode (no event loop to kick off at
            // module-load time -- everything is pushed in from JS instead).
            fn main() {}
        };
    };
}

/// The `wasm-terminal` arm of [`wasm_entry!`]: drives a
/// `Terminal<TerminalWasm>` from browser-pushed FFI calls instead of a
/// canvas/window or a plain `<pre>`. Mirrors
/// [`__wasm_headless_entry!`] one backend over; not meant to be called
/// directly.
#[doc(hidden)]
#[macro_export]
macro_rules! __wasm_terminal_entry {
    ($E:ty) => {
        #[cfg(all(
            feature = "wasm-terminal",
            not(any(feature = "software", feature = "gl", feature = "wasm-headless")),
            target_arch = "wasm32"
        ))]
        const _: () = {
            struct __RgWasmTerminalState {
                term: ::retroglyph_core::Terminal<::retroglyph_terminal_wasm::TerminalWasm>,
                state: $E,
                last_tick: ::web_time::Instant,
                frame_count: u64,
            }

            ::std::thread_local! {
                static __RG_WASM_TERMINAL: ::std::cell::RefCell<::std::option::Option<__RgWasmTerminalState>> =
                    ::std::cell::RefCell::new(::std::option::Option::None);
            }

            /// Build the `Terminal<TerminalWasm>` at the given size (in
            /// cells) and run `$E::init` once. Call before the first tick,
            /// after sizing the host terminal emulator (e.g. xterm.js's
            /// `fitAddon.fit()`).
            #[::wasm_bindgen::prelude::wasm_bindgen]
            #[allow(missing_docs)]
            pub fn wasm_terminal_example_init(width: u16, height: u16) {
                ::console_error_panic_hook::set_once();
                let mut backend = ::retroglyph_terminal_wasm::TerminalWasm::new(width, height);
                ::retroglyph_core::backend::Cursor::set_cursor_visible(&mut backend, false);
                let mut term = ::retroglyph_core::Terminal::new(backend);
                let state = <$E as $crate::Example>::init(&mut term);
                __RG_WASM_TERMINAL.with(|cell| {
                    *cell.borrow_mut() = ::std::option::Option::Some(__RgWasmTerminalState {
                        term,
                        state,
                        last_tick: ::web_time::Instant::now(),
                        frame_count: 0,
                    });
                });
            }

            /// Report a new size (in cells), e.g. after the host terminal
            /// emulator re-fits on a window resize. No-op if called before
            /// `wasm_terminal_example_init`.
            #[::wasm_bindgen::prelude::wasm_bindgen]
            #[allow(missing_docs)]
            pub fn wasm_terminal_example_resize(width: u16, height: u16) {
                __RG_WASM_TERMINAL.with(|cell| {
                    if let ::std::option::Option::Some(s) = cell.borrow_mut().as_mut() {
                        ::retroglyph_core::backend::Output::resize(
                            s.term.backend_mut(),
                            ::retroglyph_core::grid::Size { width, height },
                        );
                    }
                });
            }

            /// Decode and queue a key event via
            /// `retroglyph_terminal_wasm::decode_key_event`.
            #[::wasm_bindgen::prelude::wasm_bindgen]
            #[allow(missing_docs)]
            pub fn wasm_terminal_example_push_key(code: u32, mods: u8) {
                let Some(event) = ::retroglyph_terminal_wasm::decode_key_event(code, mods) else {
                    return;
                };
                __RG_WASM_TERMINAL.with(|cell| {
                    if let ::std::option::Option::Some(s) = cell.borrow_mut().as_mut() {
                        ::retroglyph_core::Input::push_event(
                            s.term.backend_mut(),
                            ::retroglyph_core::event::Event::Key(event),
                        );
                    }
                });
            }

            /// Decode and queue a pointer (mouse/touch) event. See
            /// [`wasm_pointer::decode_mouse`](crate::util::wasm_pointer::decode_mouse).
            #[::wasm_bindgen::prelude::wasm_bindgen]
            #[allow(missing_docs)]
            pub fn wasm_terminal_example_push_mouse(x: u16, y: u16, kind: u8) {
                let Some(event) = $crate::util::wasm_pointer::decode_mouse(x, y, kind) else {
                    return;
                };
                __RG_WASM_TERMINAL.with(|cell| {
                    if let ::std::option::Option::Some(s) = cell.borrow_mut().as_mut() {
                        ::retroglyph_core::Input::push_event(s.term.backend_mut(), event);
                    }
                });
            }

            /// Run one tick and return the ANSI bytes rendered since the
            /// last call, suitable for writing directly into a browser
            /// terminal emulator (e.g. xterm.js's `term.write(...)`).
            /// Returns an empty string if called before
            /// `wasm_terminal_example_init`.
            #[::wasm_bindgen::prelude::wasm_bindgen]
            #[allow(missing_docs)]
            pub fn wasm_terminal_example_tick() -> ::std::string::String {
                __RG_WASM_TERMINAL.with(|cell| {
                    let mut guard = cell.borrow_mut();
                    let Some(s) = guard.as_mut() else {
                        return ::std::string::String::new();
                    };
                    let now = ::web_time::Instant::now();
                    let frame = ::retroglyph_core::Frame {
                        delta: now.duration_since(s.last_tick),
                        frame: s.frame_count,
                    };
                    s.last_tick = now;
                    s.frame_count = s.frame_count.wrapping_add(1);
                    $crate::Example::tick(&mut s.state, &mut s.term, &frame);
                    s.term.backend_mut().take_output()
                })
            }

            // Required symbol for the wasm32 binary target; see the
            // matching comment in `__wasm_headless_entry!`.
            fn main() {}
        };
    };
}
