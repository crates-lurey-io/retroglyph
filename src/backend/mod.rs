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
pub use software::{SoftwareBackend, SoftwareRenderer};

use crate::event::Event;
use crate::grid::{Pos, Size};
use crate::tile::Tile;
use core::time::Duration;

/// A rendering backend that presents grid content to a display
/// and provides input events.
pub trait Backend {
    /// Draw changed cells to the output surface.
    fn draw<'a, I>(&mut self, content: I)
    where
        I: Iterator<Item = (Pos, &'a Tile)>;

    /// Draw changed cells across all layers.
    ///
    /// The default implementation forwards layer-0 tiles to [`draw`](Self::draw)
    /// and ignores higher layers. Override this to support multi-layer
    /// compositing, sub-cell offsets, or transparency.
    ///
    /// When [`needs_full_frame`](Self::needs_full_frame) returns `true`, this
    /// receives **all** cells from every allocated layer, and the backend
    /// should clear its output surface before drawing.
    fn draw_layers<'a, I>(&mut self, content: I)
    where
        I: Iterator<Item = (u8, Pos, &'a Tile)>,
    {
        self.draw(content.filter_map(
            |(layer, pos, tile)| {
                if layer == 0 { Some((pos, tile)) } else { None }
            },
        ));
    }

    /// Returns `true` if the backend needs the **entire** frame (all cells on
    /// all layers) on every call to [`draw_layers`](Self::draw_layers), rather
    /// than just the changed cells.
    ///
    /// Pixel-based backends (e.g. [`SoftwareRenderer`]) need this because
    /// sub-cell offsets can spill glyph pixels into adjacent cells — without
    /// a full redraw, orphaned pixels from the previous frame linger.
    ///
    /// The default implementation returns `false`.
    fn needs_full_frame(&self) -> bool {
        false
    }

    /// Flush buffered output to the display.
    fn flush(&mut self);

    /// Return current display dimensions.
    #[must_use]
    fn size(&self) -> Size;

    /// Clear the entire display.
    fn clear(&mut self);

    /// Notify the backend of a terminal resize.
    ///
    /// Called automatically by [`crate::Terminal::resize`] after both grids are resized.
    /// Backends that maintain internal state tied to terminal dimensions (such as
    /// [`Headless`]) should override this to update that state. The default
    /// implementation is a no-op.
    fn resize(&mut self, size: Size) {
        let _ = size;
    }

    /// Poll for an input event, waiting up to `timeout`.
    fn poll_event(&mut self, timeout: Duration) -> Option<Event>;

    /// Returns `false` if the backend has been disconnected from its
    /// output (e.g. the window was closed). The game loop should
    /// terminate when this returns `false`.
    ///
    /// The default implementation always returns `true`. Override for
    /// backends that can detect disconnect.
    fn is_connected(&self) -> bool {
        true
    }

    /// Show or hide the cursor.
    fn set_cursor_visible(&mut self, visible: bool);

    /// Move the cursor to a position.
    fn set_cursor_position(&mut self, position: Pos);
}
