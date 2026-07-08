//! Shared "what key was pressed" extraction, so examples can `match` on a `KeyCode` directly
//! instead of repeating `Event::Key(key) if key.is_down() && key.code == ...` per key.

use retroglyph_core::event::{Event, KeyCode};

/// Returns the [`KeyCode`] of `event` if it's a key press or auto-repeat, `None` otherwise
/// (releases, non-key events).
///
/// Lets a `for event in term.drain_events()` loop match directly on `pressed_key(event)` instead
/// of writing `Event::Key(key) if key.is_down() && key.code == ...` as a guard on every arm.
#[doc(hidden)]
#[must_use]
pub const fn pressed_key(event: Event) -> Option<KeyCode> {
    match event {
        Event::Key(key) if key.is_down() => Some(key.code),
        _ => None,
    }
}
