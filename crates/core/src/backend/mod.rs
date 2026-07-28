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
use crate::tint::Tint;
use core::time::Duration;

/// Associated error type used by all fallible backend methods.
///
/// Backends that are infallible (e.g. `Headless`, `SoftwareRenderer`) use
/// [`core::convert::Infallible`]. Fallible backends (e.g. `Crossterm`) use
/// [`std::io::Error`].
///
/// Requiring [`core::error::Error`] rather than just [`Display`](core::fmt::Display) +
/// [`Debug`](core::fmt::Debug) lets generic code convert `B::Error` into `Box<dyn Error>` or any
/// error-trait-based caller error with a plain `?`, and exposes `source()` chains for concrete
/// error types that wrap an inner error.
///
/// # Examples
///
/// ```
/// use retroglyph_core::backend::BackendError;
/// use core::fmt;
///
/// #[derive(Debug)]
/// struct MyBackendError;
///
/// impl fmt::Display for MyBackendError {
///     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
///         write!(f, "my backend failed")
///     }
/// }
///
/// impl core::error::Error for MyBackendError {}
/// impl BackendError for MyBackendError {}
/// ```
pub trait BackendError: core::error::Error {}

impl BackendError for core::convert::Infallible {}
#[cfg(feature = "std")]
impl BackendError for std::io::Error {}

/// One cell handed to a backend at draw time: the tile, plus the state that does not fit in one.
///
/// [`Tile`] is deliberately compact (20 bytes, no padding to spare), so a cell's rarer members
/// live in a sparse side table on [`Grid`](crate::grid::Grid) instead. A backend cannot reach
/// that table, so they are delivered here.
///
/// This is a struct rather than a tuple because it has grown twice and will grow again. Each
/// addition would otherwise break every backend's `draw` signature and leave the meaning of a
/// fourth or fifth unnamed element to the reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct DrawCell<'a> {
    /// Which layer this cell belongs to. Always `0` for cells arriving through
    /// [`Output::draw`].
    pub layer: u8,
    /// Where the cell sits in the grid.
    pub pos: Pos,
    /// The tile itself.
    pub tile: &'a Tile,
    /// The tile's full grapheme cluster, or `None` to render [`Tile::glyph`] alone.
    ///
    /// `Some` only for multi-codepoint clusters (combining marks, ZWJ sequences), and **only
    /// ever `Some` when the `egc` feature is enabled**: without it `Grid` never populates the
    /// side table, so a backend that does not support `egc` can ignore this entirely and render
    /// from [`Tile::glyph`].
    pub grapheme: Option<&'a str>,
    /// How a pixel backend recolours this cell's sprite.
    ///
    /// [`Tint::None`] for the overwhelming majority of cells. Cell backends have no sprite to
    /// recolour and ignore it; see [`Tint`].
    pub tint: Tint,
}

impl<'a> DrawCell<'a> {
    /// A cell on layer 0 with no grapheme text and no tint: the shape almost every test and
    /// cell backend wants.
    #[must_use]
    pub const fn new(pos: Pos, tile: &'a Tile) -> Self {
        Self {
            layer: 0,
            pos,
            tile,
            grapheme: None,
            tint: Tint::None,
        }
    }

    /// [`new`](Self::new) on an explicit layer.
    #[must_use]
    pub const fn on_layer(layer: u8, pos: Pos, tile: &'a Tile) -> Self {
        Self {
            layer,
            pos,
            tile,
            grapheme: None,
            tint: Tint::None,
        }
    }

    /// This cell with `grapheme` as its full cluster text.
    #[must_use]
    pub const fn with_grapheme(mut self, grapheme: Option<&'a str>) -> Self {
        self.grapheme = grapheme;
        self
    }

    /// This cell with `tint` applied to its sprite.
    #[must_use]
    pub const fn with_tint(mut self, tint: Tint) -> Self {
        self.tint = tint;
        self
    }
}

/// Draws grid content to a display and reports its dimensions.
///
/// This is the only one of the three backend facets ([`Output`], [`Input`], [`Cursor`]) that's
/// fallible: writing to a real display can fail (a broken pipe, a closed terminal, a lost
/// surface), so every mutating method here returns `Result<(), Self::Error>`.
///
/// # Examples
///
/// ```
/// use retroglyph_core::backend::{DrawCell, Output};
/// use retroglyph_core::grid::Size;
///
/// struct NullOutput;
///
/// impl Output for NullOutput {
///     type Error = core::convert::Infallible;
///
///     fn draw_layers<'a, I>(&mut self, _content: I) -> Result<(), Self::Error>
///     where
///         I: Iterator<Item = DrawCell<'a>>,
///     {
///         Ok(())
///     }
///
///     fn flush(&mut self) -> Result<(), Self::Error> {
///         Ok(())
///     }
///
///     fn size(&self) -> Size {
///         Size {
///             width: 4,
///             height: 2,
///         }
///     }
///
///     fn clear(&mut self) -> Result<(), Self::Error> {
///         Ok(())
///     }
/// }
/// ```
pub trait Output {
    /// Error type returned by fallible operations.
    type Error: BackendError;

    /// Draw changed cells to the output surface, layer 0 only.
    ///
    /// Every cell arrives as a [`DrawCell`], which carries the out-of-line state a [`Tile`]
    /// cannot: its full grapheme cluster and its tint. Both live in a side table on
    /// [`Grid`](crate::grid::Grid)
    /// rather than in the tile, so a backend that needs either must read it from here.
    ///
    /// The default implementation forwards to [`draw_layers`](Self::draw_layers), which every
    /// backend implements. [`crate::Terminal::present`] never calls this method directly (it
    /// always goes through `draw_layers`, pre-flattened onto layer 0 for a backend that doesn't
    /// composite; see [`composites_layers`](Self::composites_layers)), so overriding this is
    /// only worthwhile if a backend has a cheaper direct path for the known-single-layer case
    /// than its own `draw_layers` would take.
    ///
    /// # Errors
    ///
    /// See [`draw_layers`](Self::draw_layers), which this forwards to by default and shares
    /// its error contract with.
    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = DrawCell<'a>>,
    {
        self.draw_layers(content)
    }

    /// Draw changed cells across all layers.
    ///
    /// [`crate::Terminal::present`] always calls this method, never [`draw`](Self::draw)
    /// directly, for every backend. A backend that renders one glyph per cell and returns
    /// `false` from [`composites_layers`](Self::composites_layers) (the default) receives a
    /// stream `present` has already pre-flattened onto layer 0 (all allocated layers
    /// composited into one frame first, so layers 1+ still appear on every backend, not only
    /// pixel ones); implementing this is no different from what implementing single-layer
    /// `draw` used to mean. A pixel/GPU backend that returns `true` from `composites_layers`
    /// receives the real, multi-layer stream here and does its own compositing (per-pixel or
    /// per-quad, plus sub-cell offsets and transparency as needed).
    ///
    /// When [`needs_full_frame`](Self::needs_full_frame) returns `true`, this
    /// receives **all** cells from every allocated layer, and the backend
    /// should clear its output surface before drawing.
    ///
    /// Items are the same [`DrawCell`] [`draw`](Self::draw) receives, read through
    /// [`DrawCell::layer`] rather than a separate element.
    ///
    /// # Errors
    ///
    /// `Self::Error` is implementation-defined (a broken pipe or closed terminal for a
    /// process-backed display, a lost surface for a windowed one); implementations do not
    /// roll back cells already written before the failure. Because
    /// [`crate::Terminal::present`] only swaps its diff buffers into `previous` after the
    /// call that reached this method succeeds, a failed draw leaves the same cells marked
    /// dirty, so they are resent on the next successful present rather than silently
    /// dropped.
    fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = DrawCell<'a>>;

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
    /// `Self::Error` is implementation-defined (a broken pipe, a closed terminal, a lost
    /// surface). [`crate::Terminal::present`] calls this only after
    /// [`draw`](Self::draw)/[`draw_layers`](Self::draw_layers) succeed, and swaps its diff
    /// buffers only after `flush` also succeeds; a failed flush therefore leaves the
    /// current frame's cells buffered but unconfirmed, and they are resent on the next
    /// successful present.
    fn flush(&mut self) -> Result<(), Self::Error>;

    /// Return current display dimensions.
    #[must_use]
    fn size(&self) -> Size;

    /// Clear the entire display.
    ///
    /// # Errors
    ///
    /// `Self::Error` is implementation-defined (a broken pipe, a closed terminal, a lost
    /// surface). Unlike [`draw`](Self::draw)/[`flush`](Self::flush), this is not part of
    /// [`crate::Terminal::present`]'s per-frame path; callers that invoke it directly
    /// (some backends also call it internally on resize) should treat a failure as leaving
    /// the display in an unknown, possibly partially cleared state and retry or tear down
    /// rather than assume the previous contents are still intact.
    fn clear(&mut self) -> Result<(), Self::Error>;

    /// Notify the backend of a resize to `size`, updating what [`size`](Self::size) reports.
    ///
    /// Called automatically by [`crate::Terminal::resize`] after both grids are resized.
    /// Backends that maintain internal state tied to terminal dimensions (such as
    /// [`Headless`]) should override this to update that state. The default
    /// implementation is a no-op.
    ///
    /// A driver may also call this directly, ahead of and independent from
    /// [`crate::Terminal::resize`], to keep [`size`](Self::size) in sync with an underlying
    /// surface the moment it changes (a windowed backend reacting to an OS resize, for
    /// example) without waiting for the app to resize the terminal's grid content in
    /// response. Doing so does not resize the grid; only [`crate::Terminal::resize`] does that.
    fn resize(&mut self, size: Size) {
        let _ = size;
    }
}

/// Polls for and accepts input events.
///
/// Backends that never receive events from outside their own [`poll_event`](Self::poll_event)
/// implementation (e.g. `Crossterm`, which reads its own event stream) can use the default
/// no-op [`push_event`](Self::push_event) via an empty `impl Input for X {}`.
///
/// # Examples
///
/// ```
/// use core::time::Duration;
/// use retroglyph_core::backend::Input;
/// use retroglyph_core::event::Event;
/// use std::collections::VecDeque;
///
/// struct QueuedInput(VecDeque<Event>);
///
/// impl Input for QueuedInput {
///     fn poll_event(&mut self, _timeout: Duration) -> Option<Event> {
///         self.0.pop_front()
///     }
///
///     fn push_event(&mut self, event: Event) {
///         self.0.push_back(event);
///     }
/// }
/// ```
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
///
/// # Examples
///
/// ```
/// use retroglyph_core::backend::Cursor;
/// use retroglyph_core::grid::Pos;
///
/// struct TrackedCursor {
///     visible: bool,
///     position: Pos,
/// }
///
/// impl Cursor for TrackedCursor {
///     fn set_cursor_visible(&mut self, visible: bool) {
///         self.visible = visible;
///     }
///
///     fn set_cursor_position(&mut self, position: Pos) {
///         self.position = position;
///     }
/// }
/// ```
pub trait Cursor {
    /// Show or hide the cursor.
    fn set_cursor_visible(&mut self, _visible: bool) {}

    /// Move the cursor to a position.
    fn set_cursor_position(&mut self, _position: Pos) {}

    /// Set the cursor's shape (and blink behavior).
    ///
    /// Defaults to a no-op, matching [`set_cursor_visible`](Self::set_cursor_visible)/
    /// [`set_cursor_position`](Self::set_cursor_position): backends with no text cursor to
    /// manage, or that render on a terminal emulator with no shape-changing escape sequence,
    /// can ignore this via the default `impl Cursor for X {}`.
    fn set_cursor_style(&mut self, _style: CursorStyle) {}
}

/// The text cursor's visual shape and blink behavior.
///
/// Mirrors the six shapes a DEC-compatible terminal's `DECSCUSR` escape (`CSI Ps SP q`)
/// supports: block, underline, and bar, each either blinking or steady. `#[non_exhaustive]`
/// leaves room for a future shape (e.g. a hollow/outline block) without a breaking change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum CursorStyle {
    /// A blinking solid block (`█`). The default terminal cursor shape on most emulators.
    #[default]
    BlinkingBlock,
    /// A steady (non-blinking) solid block.
    SteadyBlock,
    /// A blinking underscore (`_`).
    BlinkingUnderline,
    /// A steady (non-blinking) underscore.
    SteadyUnderline,
    /// A blinking vertical bar (`|`), as commonly used for text-insertion cursors.
    BlinkingBar,
    /// A steady (non-blinking) vertical bar.
    SteadyBar,
}

/// A rendering backend that presents grid content to a display and provides input events.
///
/// This is a pure ergonomic bundle over [`Output`], [`Input`], and [`Cursor`], with no members
/// of its own: every type implementing all three gets `Backend` for free, and every generic
/// call site that only needs one or two facets should bound on those directly instead of
/// requiring all three through this trait.
///
/// # Examples
///
/// There is nothing to implement directly: a type gets `Backend` for free the moment it
/// implements all three facet traits.
///
/// ```
/// use core::time::Duration;
/// use retroglyph_core::backend::{Backend, Cursor, DrawCell, Input, Output};
/// use retroglyph_core::event::Event;
/// use retroglyph_core::grid::Size;
///
/// struct NullBackend;
///
/// impl Output for NullBackend {
///     type Error = core::convert::Infallible;
///
///     fn draw_layers<'a, I>(&mut self, _content: I) -> Result<(), Self::Error>
///     where
///         I: Iterator<Item = DrawCell<'a>>,
///     {
///         Ok(())
///     }
///
///     fn flush(&mut self) -> Result<(), Self::Error> {
///         Ok(())
///     }
///
///     fn size(&self) -> Size {
///         Size {
///             width: 1,
///             height: 1,
///         }
///     }
///
///     fn clear(&mut self) -> Result<(), Self::Error> {
///         Ok(())
///     }
/// }
///
/// impl Input for NullBackend {
///     fn poll_event(&mut self, _timeout: Duration) -> Option<Event> {
///         None
///     }
/// }
///
/// impl Cursor for NullBackend {}
///
/// fn assert_is_backend<B: Backend>(_backend: &B) {}
/// assert_is_backend(&NullBackend);
/// ```
pub trait Backend: Output + Input + Cursor {}

impl<T: Output + Input + Cursor> Backend for T {}
