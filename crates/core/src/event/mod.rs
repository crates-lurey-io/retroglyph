//! Input event system.
//!
//! [`Terminal::poll`](crate::terminal::Terminal::poll) returns an optional [`Event`] with support for
//! keyboard ([`KeyEvent`], all standard keys plus [`KeyModifiers`]), mouse ([`MouseEvent`]:
//! buttons, movement, scroll), touch (synthesized into the same mouse events on the
//! software/WASM backend), window resize, and close events.
//! [`has_input`](crate::terminal::Terminal::has_input) checks for a pending event without blocking. Resize
//! events are applied to the grid automatically, before the event reaches your code.

mod key;
mod mouse;

pub use key::{KeyCode, KeyEvent, KeyEventKind, KeyLocation, KeyModifiers, KeyState, ModifierKey};
pub use mouse::{MouseButton, MouseEvent, MouseEventKind, PhysicalPos};

use alloc::string::String;

/// The system's light/dark color-scheme preference, as reported by the
/// windowing/browser layer.
///
/// Currently just these two variants: every source that can report this
/// (winit's `Theme`, the browser's `prefers-color-scheme` media query) only
/// ever resolves to one of exactly these two, and a backend that can't
/// determine a preference simply never emits [`Event::ThemeChanged`] rather
/// than emitting a third "unknown" case for callers to handle. Marked
/// `#[non_exhaustive]` for consistency with sibling public enums, in case a
/// future source (e.g. a `HighContrast` case) needs to be added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SystemTheme {
    /// The system prefers a light color scheme.
    Light,
    /// The system prefers a dark color scheme.
    Dark,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
/// Terminal input event.
///
/// Does not derive `Eq`/`Hash`: [`MouseEvent`] does not (its `MouseEventKind::Scroll` variant's
/// `f32` fields implement neither).
pub enum Event {
    /// Keyboard event.
    Key(KeyEvent),
    /// Mouse event.
    Mouse(MouseEvent),
    /// Terminal window resized to the given `(cols, rows)`.
    ///
    /// When this event comes from [`Terminal::poll`](crate::terminal::Terminal::poll) (or the other
    /// [`Terminal`](crate::terminal::Terminal) methods that route through it), the grid has already been
    /// resized to these dimensions by the time the event reaches your code; the payload is there
    /// so the app can react, for example recomputing layout or redrawing. A consumer driving
    /// [`Input::poll_event`](crate::backend::Input::poll_event) directly on a raw backend gets no
    /// such guarantee and must resize the grid itself.
    Resize(u16, u16),
    /// Window closed.
    Close,
    /// The system's light/dark color-scheme preference changed, or was
    /// determined for the first time at startup.
    ///
    /// Only backends with a real source of truth for this emit it: the
    /// windowed (winit) backend, on both native and wasm (winit's web
    /// target derives it from the browser's `prefers-color-scheme` media
    /// query, including live updates). Character-mode backends (crossterm)
    /// have no equivalent free API (see the windowed backend's own docs
    /// for why) and never emit this; an app that wants a default should
    /// pick one itself rather than waiting for an event that may never
    /// arrive.
    ThemeChanged(SystemTheme),
    /// Pasted text, delivered as a single event rather than individual key
    /// presses.
    ///
    /// Not emitted by all backends: see each backend's own docs for
    /// whether and how it sources this. Content is forwarded verbatim from
    /// the source, including embedded newlines; the receiving app is
    /// responsible for any filtering it needs.
    Paste(String),
    /// The terminal or application window gained input focus.
    ///
    /// This reflects OS/terminal-level focus, not in-app widget focus (see
    /// `retroglyph-widgets`' focus ring for that).
    FocusGained,
    /// The terminal or application window lost input focus.
    ///
    /// This reflects OS/terminal-level focus, not in-app widget focus (see
    /// `retroglyph-widgets`' focus ring for that).
    FocusLost,
    /// An application-defined event injected from outside the normal input
    /// source (e.g. a network, audio, or timer thread), carrying an opaque
    /// tag the app assigns its own meaning to.
    ///
    /// Only emitted by backends with a real cross-thread injection point:
    /// the windowed (winit) backend's `EventProxy`
    /// (`retroglyph_window::winit::EventProxy::send_event`), which forwards
    /// the `u64` unchanged. The payload is a plain `u64`
    /// rather than an arbitrary boxed value: it keeps `Event` cheaply
    /// `Clone`/`PartialEq` (a `Box<dyn Any>` could not derive either) and
    /// needs no generic parameter threaded through every crate that names
    /// `Event`. Treat it as a correlation id: look up
    /// the real payload in whatever shared state or channel the sending
    /// thread already placed it in.
    Custom(u64),
}

/// Whether `new` should replace the queue's current tail event instead of being pushed alongside
/// it, when a backend is appending `new` to a `Vec`/`VecDeque` of pending events.
///
/// True for two consecutive [`Event::Mouse`] events both carrying [`MouseEventKind::Moved`], or
/// both carrying [`MouseEventKind::Drag`] with the same button: a queue owner (winit, the wasm
/// FFI boundary, `Headless`) can be fed pointer-move/drag events far faster than a consumer
/// drains them, and only the most recent position matters once it does (whether or not a button
/// is held), so collapsing either run in place keeps the queue from growing unbounded
/// (retroglyph#294, retroglyph#768, retroglyph#942). A `Drag` only coalesces with another `Drag`
/// carrying the *same* button, so a button change mid-drag is never swallowed into the wrong
/// button's position. `Scroll` deliberately does not coalesce despite also being high-frequency:
/// its `dx`/`dy` are deltas, not absolute state, so collapsing a run would discard real scroll
/// distance rather than a stale intermediate value. Every other event kind (clicks, keys, resize,
/// ...) always returns `false`.
#[must_use]
pub const fn coalesces_with(new: &Event, existing: &Event) -> bool {
    matches!(
        (new, existing),
        (
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Moved,
                ..
            }),
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Moved,
                ..
            }),
        )
    ) || matches!(
        (new, existing),
        (
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Drag(new_button),
                ..
            }),
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Drag(existing_button),
                ..
            }),
        ) if *new_button as u8 == *existing_button as u8
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Pos;

    #[test]
    fn test_paste_event_carries_text() {
        use alloc::string::ToString as _;

        let event = Event::Paste("hello".to_string());
        let Event::Paste(text) = event else {
            panic!("Expected Event::Paste");
        };
        assert_eq!(text, "hello");
    }

    #[test]
    fn test_custom_event_carries_opaque_id() {
        let event = Event::Custom(42);
        let Event::Custom(id) = event else {
            panic!("Expected Event::Custom");
        };
        assert_eq!(id, 42);
        assert_ne!(Event::Custom(1), Event::Custom(2));
    }

    #[test]
    fn test_focus_gained_and_lost_are_distinct() {
        assert!(matches!(Event::FocusGained, Event::FocusGained));
        assert!(matches!(Event::FocusLost, Event::FocusLost));
        assert_ne!(Event::FocusGained, Event::FocusLost);
    }

    fn moved_at(x: u16, y: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: Pos::new(x, y),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        })
    }

    #[test]
    fn coalesces_with_true_for_two_consecutive_moved_events() {
        assert!(coalesces_with(&moved_at(1, 1), &moved_at(0, 0)));
    }

    #[test]
    fn coalesces_with_false_for_non_moved_mouse_events() {
        let down = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            position: Pos::new(0, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        });
        assert!(!coalesces_with(&moved_at(1, 1), &down));
        assert!(!coalesces_with(&down, &moved_at(0, 0)));
    }

    #[test]
    fn coalesces_with_false_for_non_mouse_events() {
        let key = Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(!coalesces_with(&moved_at(1, 1), &key));
        assert!(!coalesces_with(&key, &moved_at(0, 0)));
    }

    fn drag_at(x: u16, y: u16, button: MouseButton) -> Event {
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(button),
            position: Pos::new(x, y),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        })
    }

    #[test]
    fn coalesces_with_true_for_two_consecutive_drag_events_same_button() {
        assert!(coalesces_with(
            &drag_at(1, 1, MouseButton::Left),
            &drag_at(0, 0, MouseButton::Left),
        ));
    }

    #[test]
    fn coalesces_with_false_for_drag_events_with_different_buttons() {
        assert!(!coalesces_with(
            &drag_at(1, 1, MouseButton::Right),
            &drag_at(0, 0, MouseButton::Left),
        ));
        assert!(!coalesces_with(
            &drag_at(1, 1, MouseButton::Left),
            &drag_at(0, 0, MouseButton::Middle),
        ));
    }

    #[test]
    fn coalesces_with_false_for_scroll_events() {
        let scroll = |dy: f32| {
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Scroll { dx: 0.0, dy },
                position: Pos::new(0, 0),
                pixel_position: None,
                modifiers: KeyModifiers::NONE,
            })
        };
        assert!(!coalesces_with(&scroll(1.0), &scroll(1.0)));
    }

    #[test]
    fn coalesces_with_false_for_two_non_mouse_events() {
        assert!(!coalesces_with(&Event::Close, &Event::Resize(1, 1)));
    }
}
