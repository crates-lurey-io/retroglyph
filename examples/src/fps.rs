//! Optional FPS / frame-time overlay, drawn by the shared driver ([`ExampleApp`](crate::launch))
//! when the `fps` feature is enabled.
//!
//! This is deliberately example-only shared code, not a `retroglyph-widgets` widget: the reusable
//! part (draw a small labeled box) is trivial, while everything that matters here -- the backend
//! label, and the driver owning `present` so the overlay lands on top of every example for free --
//! is inherently harness plumbing. If it earns its keep it can graduate to a widget later.

#![allow(clippy::redundant_pub_crate)]

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

/// Smoothed frame-timing state behind the [FPS overlay](Self::draw).
pub(crate) struct Fps {
    /// Smoothed frame time in seconds; `None` until the first [`tick`](Self::tick).
    smoothed_secs: Option<f64>,
}

impl Fps {
    pub(crate) const fn new() -> Self {
        Self {
            smoothed_secs: None,
        }
    }

    /// Folds one frame's wall-clock delta into the smoothed average.
    pub(crate) fn tick(&mut self, delta: Duration) {
        let secs = delta.as_secs_f64();
        self.smoothed_secs = Some(
            self.smoothed_secs
                .map_or(secs, |prev| prev.mul_add(1.0 - ALPHA, secs * ALPHA)),
        );
    }

    /// Draws `NNN fps  MM.M ms  <backend>` in the top-right corner on a solid dark background, on a
    /// top layer so it sits over the example's content. No-op before the first [`tick`](Self::tick)
    /// or if the grid is narrower than the readout.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub(crate) fn draw<B: Backend>(&self, term: &mut Terminal<B>, backend: &str) {
        // On wasm the overlay is live-toggleable via an injected floating button, so it works even
        // over a full-screen canvas without touching the example's input. When toggled off, wipe
        // any prior readout so it doesn't linger.
        #[cfg(all(target_arch = "wasm32", any(feature = "software", feature = "gl")))]
        {
            wasm_toggle::ensure_button();
            if !wasm_toggle::enabled() {
                term.layer(OVERLAY_LAYER);
                term.clear();
                term.layer(0);
                return;
            }
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

/// The wasm-only floating toggle button. A `position: fixed` DOM button (so it works over a
/// full-screen canvas) flips a thread-local flag the overlay reads each frame -- a live toggle that
/// never touches the example's event queue.
#[cfg(all(target_arch = "wasm32", any(feature = "software", feature = "gl")))]
mod wasm_toggle {
    use std::cell::Cell;
    use wasm_bindgen::JsCast as _;
    use wasm_bindgen::prelude::Closure;

    thread_local! {
        static ENABLED: Cell<bool> = const { Cell::new(true) };
        static BUTTON_ADDED: Cell<bool> = const { Cell::new(false) };
    }

    pub(super) fn enabled() -> bool {
        ENABLED.with(Cell::get)
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
            ENABLED.with(|e| e.set(!e.get()));
        });
        let _ = btn.add_event_listener_with_callback("click", onclick.as_ref().unchecked_ref());
        // Leak the closure so the callback stays valid for the page's lifetime.
        onclick.forget();
        let _ = body.append_child(&btn);
    }
}

#[cfg(test)]
mod tests {
    use super::Fps;
    use retroglyph_core::{Headless, Terminal};
    use std::time::Duration;

    #[test]
    fn overlay_renders_fps_ms_and_backend_top_right() {
        let mut term = Terminal::new(Headless::new(40, 5));
        let mut fps = Fps::new();
        // ~16 ms/frame -> ~62 fps; feed enough that the EMA settles.
        for _ in 0..200 {
            fps.tick(Duration::from_millis(16));
        }
        fps.draw(&mut term, "software");
        term.present().ok();

        let view = term.backend().format_view();
        let top = view.lines().next().expect("a top row");
        assert!(top.contains("fps"), "top row missing fps: {top:?}");
        assert!(top.contains("ms"), "top row missing ms: {top:?}");
        assert!(top.contains("software"), "top row missing backend: {top:?}");
        // ~62 fps for a 16 ms frame.
        assert!(top.contains("62"), "top row missing ~62 fps: {top:?}");
    }

    #[test]
    fn draw_is_a_noop_before_the_first_tick() {
        let mut term = Terminal::new(Headless::new(40, 5));
        Fps::new().draw(&mut term, "gl");
        term.present().ok();
        let view = term.backend().format_view();
        assert!(!view.contains("fps"), "nothing should render before a tick");
    }
}
