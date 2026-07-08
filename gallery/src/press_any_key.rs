//! "Quit on any key, or the window being closed" -- the gallery's default exit condition.

use retroglyph_core::event::Event;
use retroglyph_core::{Backend, Terminal};

/// Returns `true` once a key has been pressed, or the window's close button has been clicked,
/// since the last call.
///
/// Drains the event queue so `Event::Resize`, `Event::Mouse`, and backend-specific events (e.g.
/// the software backend's theme change) never trigger a false exit -- unlike
/// `Terminal::has_input()`, which fires on *any* queued event, not just these two.
///
/// `Event::Close` needs handling explicitly: the windowed backends push it when the OS close
/// button is clicked, but nothing auto-exits the app on it -- without this, clicking the window's
/// X would do nothing.
///
/// This is a demo-only convenience: it discards every other queued event to answer one boolean
/// question, which is fine for "quit the tutorial" but not something a real app should build on.
#[doc(hidden)]
pub fn any_key_pressed_or_window_closed<B: Backend>(term: &mut Terminal<B>) -> bool {
    term.drain_events()
        .any(|event| matches!(event, Event::Key(_) | Event::Close))
}
