//! Pluggable rendering backends.
//!
//! The [`Output`], [`Input`], and [`Cursor`] traits (plus the [`Backend`] bundle that ties them
//! together) and the dependency-free [`Headless`] test backend live here. Platform backends
//! (crossterm, software/winit) are separate crates (`retroglyph-crossterm`, `retroglyph-software`)
//! that depend on this one and implement these traits.

pub mod headless;

pub use headless::Headless;

use crate::event::Event;
use crate::grid::{Pos, Size};
use crate::tile::Tile;
use core::time::Duration;

/// Associated error type used by all fallible backend methods.
///
/// Backends that are infallible (e.g. `Headless`, `SoftwareRenderer`) use
/// [`core::convert::Infallible`]. Fallible backends (e.g. `Crossterm`) use
/// [`std::io::Error`].
pub trait BackendError: core::fmt::Display + core::fmt::Debug {}

impl BackendError for core::convert::Infallible {}
#[cfg(feature = "std")]
impl BackendError for std::io::Error {}

/// Draws grid content to a display and reports its dimensions.
///
/// This is the only one of the three backend facets ([`Output`], [`Input`], [`Cursor`]) that's
/// fallible: writing to a real display can fail (a broken pipe, a closed terminal, a lost
/// surface), so every mutating method here returns `Result<(), Self::Error>`.
pub trait Output {
    /// Error type returned by fallible operations.
    type Error: BackendError;

    /// Draw changed cells to the output surface.
    ///
    /// The third element of each item is the tile's full grapheme cluster
    /// (see [`Grid::grapheme`](crate::grid::Grid::grapheme)), `Some` only for
    /// multi-codepoint EGCs (combining marks, ZWJ sequences); `None` means
    /// render the tile's [`glyph`](Tile::glyph) alone. `Tile` itself never
    /// carries this text (it lives in a side-table on `Grid`), so backends
    /// that need the full grapheme at draw time must read it from here
    /// rather than from the tile.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot write to the output surface
    /// (e.g., a broken pipe or closed terminal).
    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (Pos, &'a Tile, Option<&'a str>)>;

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
    /// See [`draw`](Self::draw) for the meaning of each item's grapheme text.
    ///
    /// # Errors
    ///
    /// See [`draw`](Self::draw).
    fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u8, Pos, &'a Tile, Option<&'a str>)>,
    {
        self.draw(content.filter_map(|(layer, pos, tile, extra)| {
            if layer == 0 {
                Some((pos, tile, extra))
            } else {
                None
            }
        }))
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
    /// return `true` and composite the layers themselves.
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
}

/// Polls for and accepts input events.
///
/// Backends that never receive events from outside their own [`poll_event`](Self::poll_event)
/// implementation (e.g. `Crossterm`, which reads its own event stream) can use the default
/// no-op [`push_event`](Self::push_event) via an empty `impl Input for X {}`.
pub trait Input {
    /// Poll for an input event, waiting up to `timeout`.
    fn poll_event(&mut self, timeout: Duration) -> Option<Event>;

    /// Push an event into the backend's event buffer.
    ///
    /// Backends that receive events externally (e.g., from a window event
    /// loop or a test harness) override this to queue events for
    /// [`poll_event`](Self::poll_event). The default is a no-op.
    ///
    /// - Windowed backends: called by `ApplicationHandler` on each event.
    /// - Headless: called by tests to inject synthetic events.
    /// - Crossterm: reads from its own event stream; no-op here.
    fn push_event(&mut self, _event: Event) {}
}

/// Shows, hides, and moves a text cursor.
///
/// Both methods default to a no-op so backends with no text cursor to manage (pixel/windowed
/// backends, where games draw their own cursor if they want one) can use an empty
/// `impl Cursor for X {}` instead of writing dead stub bodies by hand.
pub trait Cursor {
    /// Show or hide the cursor.
    fn set_cursor_visible(&mut self, _visible: bool) {}

    /// Move the cursor to a position.
    fn set_cursor_position(&mut self, _position: Pos) {}
}

/// A rendering backend that presents grid content to a display and provides input events.
///
/// This is a pure ergonomic bundle over [`Output`], [`Input`], and [`Cursor`], with no members
/// of its own: every type implementing all three gets `Backend` for free, and every generic
/// call site that only needs one or two facets should bound on those directly instead of
/// requiring all three through this trait.
pub trait Backend: Output + Input + Cursor {}

impl<T: Output + Input + Cursor> Backend for T {}
