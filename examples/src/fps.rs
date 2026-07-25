//! FPS / frame-time overlay, drawn by the shared driver ([`ExampleApp`](crate::launch)) on top of
//! every windowed and crossterm example.
//!
//! Visible by default -- running an example should show its frame rate without having to know a
//! flag exists -- and toggled at runtime by pressing [`` ` ``/F1](is_toggle_key), or by clicking
//! the floating button the overlay injects on wasm (where there's no key the page reliably owns).
//! `RG_FPS=0` picks the *starting* state for a run that wants to come up clean; see
//! [`starts_visible`].
//!
//! This is deliberately example-only shared code, not a `retroglyph-widgets` widget: the reusable
//! part (draw a small labeled box) is trivial, while everything that matters here -- the backend
//! label, and the driver owning `present` so the overlay lands on top of every example for free --
//! is inherently harness plumbing. If it earns its keep it can graduate to a widget later.

#![allow(clippy::redundant_pub_crate)]

use retroglyph_core::event::{Event, KeyCode, KeyEventKind};
use retroglyph_core::{Backend, Color, Style, Terminal};
use std::time::Duration;

/// Grid layer the overlay draws on. Above every layer the examples use (they only touch 0 and 1),
/// so it composites on top on the GL backend and flattens last on the cell backends. Kept small
/// because the GL backend allocates every layer up to the highest one referenced, so a large value
/// would make the overlay itself cost several empty layer draws per frame.
const OVERLAY_LAYER: u8 = 2;

/// Exponential-moving-average weight for each new frame's time. Small = steadier readout, slower to
/// react to a genuine frame-rate change.
const ALPHA: f64 = 0.1;

/// Whether `event` is the overlay's toggle key.
///
/// Backtick, plus F1 as an alias. Backtick is the primary because it's the one that survives
/// everywhere this runs unmodified: macOS maps F1 to a system brightness key unless the user has
/// opted into "use F1, F2, etc. as standard function keys", and browsers claim several of the
/// function row outright. Neither is bound by any example in the gallery.
///
/// Only [`KeyEventKind::Press`] counts. The windowed backends (and crossterm under the kitty
/// keyboard protocol) report releases too, so matching on the key alone would toggle twice per
/// physical press and appear to do nothing at all.
pub(crate) fn is_toggle_key(event: &Event) -> bool {
    let Event::Key(key) = event else {
        return false;
    };
    key.kind == KeyEventKind::Press && matches!(key.code, KeyCode::Char('`') | KeyCode::F(1))
}

/// Whether the overlay starts visible, from the `RG_FPS` environment variable.
///
/// This is the *starting* state only -- [`is_toggle_key`] flips it at runtime either way. It
/// exists for runs that need to come up clean and never touch a keyboard: `examples/tests/support`'s
/// `capture_pty` sets `RG_FPS=0` when it spawns an example under a PTY, because a live frame rate
/// is by definition not reproducible and would otherwise churn every committed SVG on every run.
///
/// Read by the driver once at startup rather than by [`Fps::draw`], so [`Fps`] stays a pure
/// renderer whose unit tests don't depend on the ambient environment. Always `true` on `wasm32`
/// (nothing sets environment variables there).
pub(crate) fn starts_visible() -> bool {
    visible_from_env(std::env::var("RG_FPS").ok().as_deref())
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

/// Smoothed frame-timing state behind the [FPS overlay](Self::draw).
pub(crate) struct Fps {
    /// Smoothed frame time in seconds; `None` until the first [`tick`](Self::tick).
    smoothed_secs: Option<f64>,
    /// Whether [`draw`](Self::draw) renders anything. Flipped by [`toggle`](Self::toggle).
    visible: bool,
}

impl Fps {
    pub(crate) const fn new(visible: bool) -> Self {
        Self {
            smoothed_secs: None,
            visible,
        }
    }

    /// Shows the overlay if it's hidden, hides it if it's shown.
    pub(crate) const fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Folds one frame's wall-clock delta into the smoothed average, and applies a pending click
    /// on the wasm toggle button (the one input source that can't reach the driver's key
    /// interception, since it's a DOM button rather than an [`Event`]).
    pub(crate) fn tick(&mut self, delta: Duration) {
        #[cfg(all(target_arch = "wasm32", any(feature = "software", feature = "gl")))]
        {
            wasm_toggle::ensure_button();
            if wasm_toggle::take_toggle_request() {
                self.toggle();
            }
        }

        let secs = delta.as_secs_f64();
        self.smoothed_secs = Some(
            self.smoothed_secs
                .map_or(secs, |prev| prev.mul_add(1.0 - ALPHA, secs * ALPHA)),
        );
    }

    /// Draws `NNN fps  MM.M ms  <backend>` in the top-right corner on a solid dark background, on a
    /// top layer so it sits over the example's content. No-op while hidden
    /// ([`toggle`](Self::toggle)), before the first [`tick`](Self::tick), or if the grid is
    /// narrower than the readout.
    ///
    /// Nothing has to be wiped when it's hidden: [`Terminal::present`] clears every layer of the
    /// working grid after each frame, so simply not drawing leaves the overlay layer empty and the
    /// next present diffs the old readout away.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub(crate) fn draw<B: Backend>(&self, term: &mut Terminal<B>, backend: &str) {
        if !self.visible {
            return;
        }
        let Some(secs) = self.smoothed_secs else {
            return;
        };
        if secs <= 0.0 {
            return;
        }
        // Fixed-width numeric fields so the string keeps a constant length as the numbers change,
        // which means redrawing over the same cells each frame leaves no stale glyphs.
        let text = format!(
            " {:>3.0} fps  {:>5.1} ms  {backend} ",
            1.0 / secs,
            secs * 1000.0
        );

        let width = term.backend().size().width;
        let len = text.chars().count() as u16;
        if len > width {
            return;
        }
        let x0 = width - len;

        let style = Style::new()
            .fg(Color::Rgb {
                r: 0xE6,
                g: 0xE6,
                b: 0xE6,
            })
            .bg(Color::Rgb {
                r: 0x10,
                g: 0x10,
                b: 0x14,
            });

        term.layer(OVERLAY_LAYER);
        for (i, ch) in text.chars().enumerate() {
            term.put_styled(x0 + i as u16, 0, ch, style);
        }
        // Restore the base layer so the next example `tick` draws where it expects.
        term.layer(0);
    }
}

/// The wasm-only floating toggle button: the browser counterpart to the native
/// [backtick/F1 key](is_toggle_key).
///
/// A `position: fixed` DOM button, so it works over a full-screen canvas, and a click never
/// touches the example's event queue. It can't route through the driver's key interception like
/// the native toggle does (it isn't an [`Event`] and never enters the backend's queue), so it
/// records a request that [`Fps::tick`] picks up on the next frame instead.
#[cfg(all(target_arch = "wasm32", any(feature = "software", feature = "gl")))]
mod wasm_toggle {
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
    pub(super) fn take_toggle_request() -> bool {
        TOGGLE_REQUESTED.with(|requested| requested.replace(false))
    }

    /// Injects the toggle button once (idempotent).
    pub(super) fn ensure_button() {
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
    use super::{Fps, is_toggle_key, visible_from_env};
    use retroglyph_core::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use retroglyph_core::{Headless, Terminal};
    use std::time::Duration;

    /// A settled `Fps`: ~16 ms/frame -> ~62 fps, fed enough frames for the EMA to converge.
    fn settled(visible: bool) -> Fps {
        let mut fps = Fps::new(visible);
        for _ in 0..200 {
            fps.tick(Duration::from_millis(16));
        }
        fps
    }

    fn rendered(fps: &Fps, backend: &str) -> String {
        let mut term = Terminal::new(Headless::new(40, 5));
        fps.draw(&mut term, backend);
        term.present().ok();
        term.backend().format_view()
    }

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

    #[test]
    fn overlay_renders_fps_ms_and_backend_top_right() {
        let view = rendered(&settled(true), "software");
        let top = view.lines().next().expect("a top row");
        assert!(top.contains("fps"), "top row missing fps: {top:?}");
        assert!(top.contains("ms"), "top row missing ms: {top:?}");
        assert!(top.contains("software"), "top row missing backend: {top:?}");
        // ~62 fps for a 16 ms frame.
        assert!(top.contains("62"), "top row missing ~62 fps: {top:?}");
    }

    #[test]
    fn draw_is_a_noop_before_the_first_tick() {
        let view = rendered(&Fps::new(true), "gl");
        assert!(!view.contains("fps"), "nothing should render before a tick");
    }

    #[test]
    fn toggle_hides_and_shows_a_settled_overlay() {
        let mut fps = settled(true);
        assert!(rendered(&fps, "software").contains("fps"));

        fps.toggle();
        assert!(
            !rendered(&fps, "software").contains("fps"),
            "toggled off, nothing should render"
        );

        fps.toggle();
        assert!(
            rendered(&fps, "software").contains("fps"),
            "toggled back on"
        );
    }

    #[test]
    fn an_overlay_that_starts_hidden_can_be_toggled_on() {
        let mut fps = settled(false);
        assert!(!rendered(&fps, "crossterm").contains("fps"));
        fps.toggle();
        assert!(rendered(&fps, "crossterm").contains("fps"));
    }

    #[test]
    fn backtick_and_f1_presses_are_the_toggle() {
        for code in [KeyCode::Char('`'), KeyCode::F(1)] {
            let press = Event::Key(KeyEvent::new(code, KeyModifiers::NONE));
            assert!(is_toggle_key(&press), "{code:?} press should toggle");
        }
    }

    #[test]
    fn key_releases_and_repeats_are_not_the_toggle() {
        // Otherwise one physical press on a backend that reports releases (the windowed ones,
        // and crossterm under the kitty protocol) would flip twice and look like a no-op.
        for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
            let event = Event::Key(KeyEvent::with_kind(
                KeyCode::Char('`'),
                KeyModifiers::NONE,
                kind,
            ));
            assert!(!is_toggle_key(&event), "{kind:?} should not toggle");
        }
    }

    #[test]
    fn other_input_is_left_for_the_example() {
        for event in [
            Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            Event::Key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE)),
            Event::Resize(80, 24),
            Event::Close,
        ] {
            assert!(!is_toggle_key(&event), "{event:?} should not toggle");
        }
    }
}
