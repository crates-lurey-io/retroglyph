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
pub use software::{SoftwareBackend, SoftwareRenderer, WindowedBackend};

use crate::event::Event;
use crate::grid::{Pos, Size};
use crate::tile::Tile;
use core::time::Duration;

/// Associated error type used by all fallible backend methods.
///
/// Backends that are infallible (e.g. `Headless`, `SoftwareRenderer`) use
/// [`core::convert::Infallible`].  Fallible backends (e.g. `Crossterm`) use
/// [`std::io::Error`].
pub trait BackendError: core::fmt::Display + core::fmt::Debug {}

impl BackendError for core::convert::Infallible {}
impl BackendError for std::io::Error {}

/// A rendering backend that presents grid content to a display
/// and provides input events.
pub trait Backend {
    /// Error type returned by fallible operations.
    type Error: BackendError;

    /// Draw changed cells to the output surface.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot write to the output surface
    /// (e.g., a broken pipe or closed terminal).
    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
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
    ///
    /// # Errors
    ///
    /// See [`draw`](Self::draw).
    fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u8, Pos, &'a Tile)>,
    {
        self.draw(content.filter_map(
            |(layer, pos, tile)| {
                if layer == 0 { Some((pos, tile)) } else { None }
            },
        ))
    }

    /// Returns `true` if the backend needs the **entire** frame (all cells on
    /// all layers) on every call to [`draw_layers`](Self::draw_layers), rather
    /// than just the changed cells.
    ///
    /// Pixel-based backends (e.g. `SoftwareRenderer`) need this because
    /// sub-cell offsets can spill glyph pixels into adjacent cells — without
    /// a full redraw, orphaned pixels from the previous frame linger.
    ///
    /// The default implementation returns `false`.
    fn needs_full_frame(&self) -> bool {
        false
    }

    /// Whether this backend composites layers itself (per pixel or quad),
    /// receiving the raw layered stream from [`draw_layers`](Self::draw_layers).
    ///
    /// Backends that render one glyph per cell return `false` (the default) and
    /// receive a pre-flattened, single-layer stream: [`crate::Terminal::present`]
    /// composites all allocated layers into one frame first. This makes layers
    /// 1+ appear on every backend, not only pixel backends. Pixel/GPU backends
    /// return `true` and composite the layers themselves. See ADR 015.
    fn composites_layers(&self) -> bool {
        false
    }

    /// Flush buffered output to the display.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot flush (e.g., a broken pipe).
    fn flush(&mut self) -> Result<(), Self::Error>;

    /// Return current display dimensions.
    #[must_use]
    fn size(&self) -> Size;

    /// Clear the entire display.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot clear the display.
    fn clear(&mut self) -> Result<(), Self::Error>;

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

    /// Show or hide the cursor.
    fn set_cursor_visible(&mut self, visible: bool);

    /// Move the cursor to a position.
    fn set_cursor_position(&mut self, position: Pos);

    /// Push an event into the backend's event buffer.
    ///
    /// Backends that receive events externally (e.g., from a window event
    /// loop or a test harness) override this to queue events for
    /// [`poll_event`](Self::poll_event).  The default is a no-op.
    ///
    /// - Windowed backends: called by `ApplicationHandler` on each event.
    /// - Headless: called by tests to inject synthetic events.
    /// - Crossterm: reads from its own event stream; no-op here.
    fn push_event(&mut self, _event: Event) {}
}
