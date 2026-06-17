//! Pluggable rendering backends.

pub mod headless;

pub use headless::Headless;

use crate::cell::Cell;
use crate::event::Event;
use crate::grid::{Position, Size};
use core::time::Duration;

/// A rendering backend that presents grid content to a display
/// and provides input events.
pub trait Backend {
    /// Draw changed cells to the output surface.
    fn draw<'a, I>(&mut self, content: I)
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>;

    /// Flush buffered output to the display.
    fn flush(&mut self);

    /// Return current display dimensions.
    #[must_use]
    fn size(&self) -> Size;

    /// Clear the entire display.
    fn clear(&mut self);

    /// Poll for an input event, waiting up to `timeout`.
    fn poll_event(&mut self, timeout: Duration) -> Option<Event>;

    /// Show or hide the cursor.
    fn set_cursor_visible(&mut self, visible: bool);

    /// Move the cursor to a position.
    fn set_cursor_position(&mut self, position: Position);
}
