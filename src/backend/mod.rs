//! Pluggable rendering backends.

#[cfg(feature = "crossterm")]
pub mod crossterm;
pub mod headless;
#[cfg(feature = "software")]
pub mod software;

#[cfg(feature = "crossterm")]
pub use crossterm::Crossterm;
pub use headless::Headless;
#[cfg(feature = "software")]
pub use software::SoftwareBackend;

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
        I: Iterator<Item = (Position, &'a Cell)>;

    /// Flush buffered output to the display.
    fn flush(&mut self);

    /// Return current display dimensions.
    #[must_use]
    fn size(&self) -> Size;

    /// Clear the entire display.
    fn clear(&mut self);

    /// Notify the backend of a terminal resize.
    ///
    /// Called automatically by [`Terminal::resize`] after both grids are resized.
    /// Backends that maintain internal state tied to terminal dimensions (such as
    /// [`Headless`]) should override this to update that state. The default
    /// implementation is a no-op.
    fn resize(&mut self, size: Size) {
        let _ = size;
    }

    /// Poll for an input event, waiting up to `timeout`.
    fn poll_event(&mut self, timeout: Duration) -> Option<Event>;

    /// Show or hide the cursor.
    fn set_cursor_visible(&mut self, visible: bool);

    /// Move the cursor to a position.
    fn set_cursor_position(&mut self, position: Position);
}
