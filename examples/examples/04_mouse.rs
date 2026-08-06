//! 04: Mouse
//!
//! [`Event::Mouse`]/[`MouseEventKind`] decode: button down/up, motion, and scroll, all reported
//! in cell-grid coordinates. Free (button-less) mouse motion is a real-terminal capability, not
//! something every backend can guarantee. If no [`MouseEventKind::Moved`] event has arrived
//! after a short grace period, the example assumes motion isn't being reported and shows
//! "motion unavailable on this backend" instead of a frame that looks broken or blank. A click
//! still updates the tracked position either way.
//!
//! ```sh
//! cargo run --example 04_mouse --features crossterm
//! cargo run --example 04_mouse --features software
//! cargo run --example 04_mouse  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Keys: `q` or `Escape` quits. Move the mouse and click to see it tracked, or close the window.

use retroglyph_core::backend::Backend;
use retroglyph_core::color::Style;
use retroglyph_core::event::{Event, KeyCode, MouseEventKind};
use retroglyph_core::grid::Pos;
use retroglyph_core::surface::Surface;
use retroglyph_core::terminal::Terminal;
use retroglyph_examples::Example;

/// Ticks with no [`MouseEventKind::Moved`] event before assuming this backend never
/// reports free motion and switching to the fallback note. Large enough that a human
/// interacting with the real backends never sees a false fallback while still moving
/// the mouse toward the window; small enough that headless/scripted runs (which never
/// inject a `Moved` event) settle into the fallback state within a handful of frames.
const MOTION_GRACE_TICKS: u32 = 120;

/// State for the mouse example: last known position, click state, and whether motion
/// has ever been observed.
#[derive(Default)]
pub struct Mouse {
    ticks: u32,
    motion_seen: bool,
    position: Option<Pos>,
    last_event: String,
    click_count: u32,
}

impl Mouse {
    /// Drains pending input, returning `false` if the user asked to quit.
    fn handle_events<B: Backend>(&mut self, term: &mut Terminal<B>) -> bool {
        for event in term.drain_events() {
            match event {
                Event::Key(key) if matches!(key.code, KeyCode::Char('q') | KeyCode::Escape) => {
                    return false;
                }
                Event::Close => return false,
                Event::Mouse(mouse) => {
                    self.position = Some(mouse.position);
                    self.last_event = match mouse.kind {
                        MouseEventKind::Down(button) => {
                            self.click_count += 1;
                            format!("Down({button:?})")
                        }
                        MouseEventKind::Up(button) => format!("Up({button:?})"),
                        MouseEventKind::Moved => {
                            self.motion_seen = true;
                            "Moved".to_owned()
                        }
                        MouseEventKind::Scroll { dx, dy } => format!("Scroll(dx: {dx}, dy: {dy})"),
                        _ => "Unknown".to_owned(),
                    };
                }
                _ => {}
            }
        }
        true
    }

    /// `true` once `MOTION_GRACE_TICKS` have elapsed with no `Moved` event ever seen --
    /// the fallback condition. Sticks to `false` permanently the moment motion is seen,
    /// since `motion_seen` only ever transitions `false` -> `true`.
    const fn motion_unavailable(&self) -> bool {
        !self.motion_seen && self.ticks > MOTION_GRACE_TICKS
    }

    /// Draws this frame (the driver presents).
    fn draw(&self, surface: &mut Surface<'_>) {
        surface.print(
            (1, 1),
            "Move the mouse and click; q / Escape quits.",
            Style::default(),
        );

        if self.motion_unavailable() {
            surface.print(
                (1, 3),
                "motion unavailable on this backend",
                Style::default(),
            );
            surface.print(
                (1, 4),
                "(click tracking still works below)",
                Style::default(),
            );
        } else if !self.motion_seen {
            surface.print((1, 3), "waiting for mouse motion...", Style::default());
        }

        surface.print((1, 6), "Position:", Style::default());
        let pos_text = self.position.map_or_else(
            || "(none yet)".to_owned(),
            |p| format!("({}, {})", p.x, p.y),
        );
        surface.print((11, 6), &pos_text, Style::default());

        surface.print((1, 7), "Last event:", Style::default());
        let event_text = if self.last_event.is_empty() {
            "(none yet)"
        } else {
            &self.last_event
        };
        surface.print((13, 7), event_text, Style::default());

        surface.print((1, 8), "Clicks:", Style::default());
        surface.print((9, 8), &self.click_count.to_string(), Style::default());
    }
}

impl Example for Mouse {
    const NAME: &'static str = "04_mouse";

    fn tick<B: Backend>(
        &mut self,
        term: &mut Terminal<B>,
        _frame: &retroglyph_core::app::Frame,
    ) -> bool {
        self.ticks += 1;
        if !self.handle_events(term) {
            return false;
        }
        let mut surface = term.surface();
        self.draw(&mut surface);
        true
    }
}

retroglyph_examples::example_main!(Mouse);
