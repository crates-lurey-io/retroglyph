//! Harness-only glue around `retroglyph_ui::perf::PerfOverlayApp`, which now owns the perf overlay
//! itself (toggle key, frame-time bookkeeping, drawing) generically for every backend -- see
//! `launch.rs`'s `ExampleApp`/`WasmToggleApp`.
//!
//! What's left here is what stayed backend/harness-specific even after that: the `RG_FPS`
//! environment variable that picks the overlay's *starting* visibility for this gallery's runs
//! (`PerfOverlayApp` itself has no opinion on environment variables), the wasm floating toggle
//! button (`PerfOverlayApp`'s built-in key toggle has nothing to hook into on a page with no key
//! it reliably owns), and [`ToggleFilter`] (crossterm needs its toggle key swallowed at the raw
//! `Input` level, one layer below where `PerfOverlayApp` itself drains events -- see that type's
//! docs for why).

#![allow(clippy::redundant_pub_crate)]

#[cfg(feature = "crossterm")]
use retroglyph_core::backend::{Compositing, Cursor, Input, Output};
#[cfg(feature = "crossterm")]
use retroglyph_core::event::Event;
#[cfg(feature = "crossterm")]
use retroglyph_core::grid::{Pos, Size};
#[cfg(feature = "crossterm")]
use std::cell::Cell;
#[cfg(feature = "crossterm")]
use std::rc::Rc;
#[cfg(feature = "crossterm")]
use std::time::Duration;

/// Whether the overlay starts visible, from `--fps` (see [`crate::args`]) or, if that flag
/// isn't passed, the `RG_FPS` environment variable.
///
/// This is the *starting* state only -- `PerfOverlayApp`'s own toggle key flips it at runtime
/// either way. It exists for runs that need to come up clean and never touch a keyboard:
/// `examples/tests/support`'s `capture_pty` sets `RG_FPS=0` when it spawns an example under a
/// PTY, because a live frame rate is by definition not reproducible and would otherwise churn
/// every committed SVG on every run.
///
/// Always `true` on `wasm32` (nothing sets environment variables there).
pub(crate) fn starts_visible() -> bool {
    crate::args::parsed()
        .fps
        .unwrap_or_else(|| visible_from_env(std::env::var("RG_FPS").ok().as_deref()))
}

/// [`starts_visible`]'s parsing, split out so it's testable without mutating the process
/// environment (`std::env::set_var` is `unsafe` in edition 2024, and `unsafe_code` is forbidden
/// workspace-wide).
fn visible_from_env(value: Option<&str>) -> bool {
    value.is_none_or(|value| {
        !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off"
        )
    })
}

/// Toggle-key presses seen by a [`ToggleFilter`] and not yet applied to the wrapping
/// [`PerfOverlayApp`](retroglyph_ui::perf::PerfOverlayApp) via [`CrosstermToggleApp`](crate::launch).
///
/// Shared by clone: the filter wraps the raw backend, the driver owns the `PerfOverlayApp`, and
/// `run_on` takes both by value into separate owners, so the count has to live outside
/// either of them. `Rc`/`Cell` rather than `Arc`/atomics because both ends are the same thread --
/// the blocking driver loop.
#[cfg(feature = "crossterm")]
pub(crate) type TogglePresses = Rc<Cell<usize>>;

/// Wraps a [`Backend`](retroglyph_core::backend::Backend) and swallows [`PerfOverlayApp`]'s toggle key
/// ([`retroglyph_ui::perf::default_is_toggle_key`]) on its way out of
/// [`Input::poll_event`](retroglyph_core::backend::Input::poll_event), counting each press into a shared
/// [`TogglePresses`] for the driver.
///
/// [`PerfOverlayApp`](retroglyph_ui::perf::PerfOverlayApp) already does this itself generically, by
/// draining [`Terminal`](retroglyph_core::terminal::Terminal)'s own event queue and re-pushing whatever
/// isn't the toggle key (see that type's "Toggling" docs) -- which is race-free for the windowed
/// backends, where winit fills the queue from its own event loop before `App::update` ever runs,
/// so a drain-at-the-top-of-the-frame genuinely sees everything the app could possibly see that
/// frame.
///
/// Crossterm has no such queue: [`Input::poll_event`](retroglyph_core::backend::Input::poll_event) reads
/// the OS directly, so an event that arrives *after* `PerfOverlayApp`'s drain and *before* the
/// example's own `drain_events` inside `tick` skips `PerfOverlayApp` entirely and lands in the
/// example, which drops it on the floor (every example in the gallery ignores keys it doesn't
/// bind). That window is measured in microseconds but is real, and gets hit often enough under a
/// scripted PTY (a snapshot test types a key the instant it sees the example is ready, with no
/// human reaction time in between) to make the gallery's own crossterm snapshot tests flaky.
/// Filtering at the source -- one layer below `Terminal`, inside the raw `Input::poll_event` call
/// itself -- is what makes the toggle arrive exactly once no matter when it is pressed.
#[cfg(feature = "crossterm")]
pub(crate) struct ToggleFilter<B> {
    /// The real backend every non-toggle operation delegates to.
    inner: B,
    /// Presses swallowed so far, drained by the driver each frame.
    presses: TogglePresses,
}

#[cfg(feature = "crossterm")]
impl<B> ToggleFilter<B> {
    /// Wraps `inner`, reporting swallowed toggle presses through `presses`.
    pub(crate) const fn new(inner: B, presses: TogglePresses) -> Self {
        Self { inner, presses }
    }
}

#[cfg(feature = "crossterm")]
impl<B: Output> Output for ToggleFilter<B> {
    type Error = B::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = retroglyph_core::backend::DrawCell<'a>>,
    {
        self.inner.draw(content)
    }

    fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = retroglyph_core::backend::DrawCell<'a>>,
    {
        self.inner.draw_layers(content)
    }

    fn compositing(&self) -> Compositing {
        self.inner.compositing()
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush()
    }

    fn size(&self) -> Size {
        self.inner.size()
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.inner.clear()
    }

    fn resize(&mut self, size: Size) {
        self.inner.resize(size);
    }
}

#[cfg(feature = "crossterm")]
impl<B: Input> Input for ToggleFilter<B> {
    fn poll_event(&mut self, timeout: Duration) -> Option<Event> {
        let start = std::time::Instant::now();
        let mut remaining = timeout;
        loop {
            let event = self.inner.poll_event(remaining)?;
            if !retroglyph_ui::perf::default_is_toggle_key(&event) {
                return Some(event);
            }
            self.presses.set(self.presses.get().saturating_add(1));
            // Zero timeout is the non-blocking drain: keep pulling so swallowing a toggle can't
            // cut a `drain_events` short with events still waiting.
            if timeout.is_zero() {
                continue;
            }
            let elapsed = start.elapsed();
            if elapsed >= timeout {
                return None;
            }
            remaining = timeout.saturating_sub(elapsed);
        }
    }

    fn push_event(&mut self, event: Event) {
        self.inner.push_event(event);
    }
}

#[cfg(feature = "crossterm")]
impl<B: Cursor> Cursor for ToggleFilter<B> {
    fn set_cursor_visible(&mut self, visible: bool) {
        self.inner.set_cursor_visible(visible);
    }

    fn set_cursor_position(&mut self, position: Pos) {
        self.inner.set_cursor_position(position);
    }
}

/// The wasm-only floating toggle button: the browser counterpart to `PerfOverlayApp`'s built-in
/// backtick/F1 key.
///
/// A `position: fixed` DOM button, so it works over a full-screen canvas, and a click never
/// touches the example's event queue. It can't route through `PerfOverlayApp`'s own key-based
/// toggle like the native one does (it isn't an [`Event`](retroglyph_core::event::Event) and
/// never enters the backend's queue), so it records a request that `WasmToggleApp` picks up on
/// the next frame instead.
#[cfg(all(
    target_arch = "wasm32",
    any(feature = "software", feature = "gl", feature = "wgpu")
))]
pub(crate) mod wasm_toggle {
    use std::cell::Cell;
    use wasm_bindgen::JsCast as _;
    use wasm_bindgen::prelude::Closure;

    thread_local! {
        static TOGGLE_REQUESTED: Cell<bool> = const { Cell::new(false) };
        static BUTTON_ADDED: Cell<bool> = const { Cell::new(false) };
    }

    /// Consumes a pending click, if any. Coalescing (rather than counting) is deliberate: two
    /// clicks inside one frame are two flips, i.e. no change, so collapsing them to one pending
    /// request and dropping the pair would be wrong only if a frame never ran in between -- in
    /// which case nothing was ever drawn differently anyway.
    pub(crate) fn take_toggle_request() -> bool {
        TOGGLE_REQUESTED.with(|requested| requested.replace(false))
    }

    /// Injects the toggle button once (idempotent).
    pub(crate) fn ensure_button() {
        if BUTTON_ADDED.with(|added| added.replace(true)) {
            return;
        }
        let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        let Some(body) = doc.body() else {
            return;
        };
        let Ok(btn) = doc.create_element("button") else {
            return;
        };
        let _ = btn.set_attribute(
            "style",
            "position:fixed;top:6px;right:6px;z-index:2147483647;font:12px monospace;\
             padding:2px 6px;opacity:0.7;cursor:pointer",
        );
        btn.set_text_content(Some("FPS"));
        let onclick = Closure::<dyn FnMut()>::new(|| {
            TOGGLE_REQUESTED.with(|requested| requested.set(true));
        });
        let _ = btn.add_event_listener_with_callback("click", onclick.as_ref().unchecked_ref());
        // Leak the closure so the callback stays valid for the page's lifetime.
        onclick.forget();
        let _ = body.append_child(&btn);
    }
}

#[cfg(test)]
mod tests {
    use super::visible_from_env;

    #[test]
    fn overlay_starts_visible_unless_rg_fps_says_otherwise() {
        assert!(visible_from_env(None), "visible by default");
        assert!(visible_from_env(Some("1")));
        assert!(
            visible_from_env(Some("")),
            "an empty value is not an opt-out"
        );
        for off in ["0", "false", "off", "OFF", " 0 "] {
            assert!(!visible_from_env(Some(off)), "{off:?} should start hidden");
        }
    }
}
