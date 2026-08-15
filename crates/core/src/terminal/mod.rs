//! [`Terminal`]: construction, sizing, resizing, cursor control, and raw grid/backend access.
//!
//! [`Terminal::present`] and event polling (starting at [`Terminal::poll`]) are the other two
//! axes of `Terminal`'s API, defined in private submodules; this module holds everything that
//! isn't specifically about presenting a frame or reading input.

use crate::backend::{Backend, CursorStyle};
use crate::event::Event;
use crate::grid::{Grid, HasSize, Pos, Rect, Size};
use crate::surface::Surface;
use alloc::collections::VecDeque;
use alloc::vec::Vec;

mod input;
mod present;
mod retain;

/// A double-buffered terminal generic over a [`Backend`].
///
/// Owns the current and previous frame grids and the backend's lifecycle (resize, present,
/// events). Drawing itself goes entirely through [`Surface`]: see [`draw`](Self::draw) for the
/// common case (draw a frame, then present it) and [`surface`](Self::surface) for manual control
/// over presenting.
///
/// # Out-of-bounds drawing
///
/// [`Surface`] clips any write that falls outside its own area rather than panicking; see
/// [`Surface`]'s own "out-of-bounds drawing" documentation.
///
/// # Examples
///
/// ```
/// use retroglyph_core::backend::Headless;
/// use retroglyph_core::color::Color;
/// use retroglyph_core::terminal::Terminal;
///
/// let mut term = Terminal::new(Headless::new(20, 5));
/// term.draw(|surface| {
///     surface.put((2, 1), '@', retroglyph_core::color::Style::new().fg(Color::GREEN));
/// })
/// .unwrap();
/// ```
#[doc(alias = "console")] // libtcod / bracket-lib
#[doc(alias = "screen")] // tcell, blessed
#[doc(alias = "buffer")] // ratatui
pub struct Terminal<B: Backend> {
    current: Grid,
    previous: Grid,
    /// Single-layer scratch buffers used only when the backend does not
    /// composite layers itself and more than one layer is in play. `present`
    /// flattens `current` into `flattened_current`, diffs it against
    /// `flattened_previous`, and sends the result. Lazily allocated on first use
    /// (see `present`'s flatten branch) so compositing backends, and cell
    /// backends that never draw past layer 0, never pay for them.
    flattened_current: Option<Grid>,
    flattened_previous: Option<Grid>,
    backend: B,
    /// Events waiting to be handed out by [`poll`](Self::poll) before the backend is polled
    /// again: the [`has_input`](Self::has_input)/[`wait_for_input`](Self::wait_for_input) lookahead
    /// buffers a single event here, and [`requeue_events`](Self::requeue_events) lets a wrapper
    /// (e.g. `PerfOverlayApp`) hand back events it drained but didn't consume, entirely through
    /// `Terminal` itself rather than a backend-specific input path.
    queued_events: VecDeque<Event>,
    /// `true` when the flatten buffers no longer reflect the last frame sent to
    /// the backend (because the single-layer fast path bypassed them). The next
    /// multi-layer present clears `flattened_previous` first so it does a full
    /// redraw instead of diffing against stale data.
    flattened_stale: bool,
    /// Incremented every time [`present`](Self::present) is called.
    ///
    /// Lets embedding drivers detect whether application code already presented during a frame,
    /// so they can skip a redundant driver-side present.
    present_count: u64,
    /// The one-shot op, if any, that [`retain_layer`](Self::retain_layer) or
    /// [`drop_layer`](Self::drop_layer) has queued for each layer on the next
    /// [`present`](Self::present).
    ///
    /// Indexed by layer id. A later call for the same layer within a frame simply overwrites the
    /// earlier one (last call wins), so `Retain` and `Drop` can never both be pending for the
    /// same layer at once, unlike the two-`Vec<bool>` representation this replaced. Reset to all
    /// [`LayerOp::None`] once each op is consumed in `present` (it's a one-shot opt-in, not a
    /// sticky mode) and on [`resize`](Self::resize).
    pending_layer_ops: Vec<LayerOp>,
}

/// The one-shot op, if any, queued on a layer for the next [`present`](Terminal::present).
///
/// See [`Terminal::retain_layer`] and [`Terminal::drop_layer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum LayerOp {
    #[default]
    None,
    Retain,
    Drop,
}

impl<B: Backend> Terminal<B> {
    /// Create a terminal with the given backend.
    /// Grid dimensions are queried from the backend.
    ///
    /// # Panics
    ///
    /// Panics if the backend reports a width of 0 (e.g. a minimized window, or a surface queried
    /// before the first configure); see [`Grid::new`]. A reported height of 0 is fine.
    #[must_use]
    pub fn new(backend: B) -> Self {
        let size = backend.size();
        let current = Grid::new(size.width(), size.height());
        let previous = Grid::new(size.width(), size.height());
        Self {
            current,
            previous,
            flattened_current: None,
            flattened_previous: None,
            backend,
            queued_events: VecDeque::new(),
            flattened_stale: false,
            present_count: 0,
            pending_layer_ops: Vec::new(),
        }
    }

    /// A [`Surface`] scoped to the whole terminal on layer 0, for manual control over presenting
    /// (e.g. partial updates spread across several calls, or conditionally skipping a present).
    /// Most callers want [`draw`](Self::draw) instead.
    pub const fn surface(&mut self) -> Surface<'_> {
        let area = self.area();
        Surface::new(&mut self.current, area, 0)
    }

    /// Returns the current grid dimensions.
    #[must_use]
    pub const fn size(&self) -> Size {
        self.current.size()
    }

    /// Returns the full drawing surface as a [`Rect`] at the origin.
    ///
    /// Equivalent to `Rect::new(0, 0, width, height)`. Handy for passing the
    /// whole terminal to layout helpers or region-based drawing.
    #[must_use]
    pub const fn area(&self) -> Rect {
        self.current.size().to_rect()
    }

    /// Resize both grids to `width` × `height` cells.
    ///
    /// Unlike [`new`](Self::new), a `width` of 0 does not panic here: a terminal can be resized
    /// down to zero columns (a minimized or zero-width window) and back up again, and the
    /// single-layer present path keeps working at zero width. A `height` of 0 is likewise fine.
    ///
    /// Content within the overlapping region is preserved in the current grid.
    /// The previous grid is cleared so the next [`present`](Self::present) redraws
    /// the entire new surface rather than diffing stale data.
    ///
    /// # Panics
    ///
    /// A zero-width terminal only supports the single-layer fast path. If any layer above 0 is
    /// allocated when [`present`](Self::present) runs at zero width, `present` panics while
    /// building its flatten buffers (see [`Grid::new`]); either avoid multi-layer drawing at zero
    /// width, or [`drop_layer`](Self::drop_layer) every layer above 0 first.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.current.resize(width, height);
        self.previous.resize(width, height);
        // Only resize the flatten buffers if they've actually been allocated (see their field
        // docs); an unallocated buffer has nothing to preserve and will be sized correctly by
        // `present` on first use anyway.
        if let Some(flattened_current) = &mut self.flattened_current {
            flattened_current.resize(width, height);
        }
        if let Some(flattened_previous) = &mut self.flattened_previous {
            flattened_previous.resize(width, height);
        }
        // Clearing previous forces a full redraw next present(), ensuring no
        // stale cells bleed into the resized layout.
        self.previous.clear_all();
        if let Some(flattened_previous) = &mut self.flattened_previous {
            flattened_previous.clear_all();
        }
        // Defensive: `resize` already clears `previous` unconditionally, so the next `present`
        // would just copy empty content forward for a still-marked layer. Dropping any pending
        // op here too keeps that a non-event rather than relying on it: a pending retention would
        // just copy empty content forward, and a pending drop is meaningless once `resize` has
        // already reallocated every layer's buffers at the new dimensions.
        self.pending_layer_ops.clear();
        self.backend.resize(Size::new(width, height));
    }

    /// Show or hide the cursor.
    ///
    /// Forwards to [`Cursor::set_cursor_visible`](crate::backend::Cursor::set_cursor_visible) on
    /// the backend.
    pub fn set_cursor_visible(&mut self, visible: bool) {
        self.backend.set_cursor_visible(visible);
    }

    /// Move the cursor to a position.
    ///
    /// Forwards to [`Cursor::set_cursor_position`](crate::backend::Cursor::set_cursor_position)
    /// on the backend.
    pub fn set_cursor_position(&mut self, position: Pos) {
        self.backend.set_cursor_position(position);
    }

    /// Set the cursor's shape (and blink behavior).
    ///
    /// Forwards to [`Cursor::set_cursor_style`](crate::backend::Cursor::set_cursor_style) on the
    /// backend.
    pub fn set_cursor_style(&mut self, style: CursorStyle) {
        self.backend.set_cursor_style(style);
    }

    /// Returns a reference to the current grid.
    #[must_use]
    pub const fn grid(&self) -> &Grid {
        &self.current
    }

    /// Returns a mutable reference to the current grid, with no clipping or layer scoping.
    ///
    /// Escape hatch for whole-grid operations that don't fit [`Surface`]'s clipped,
    /// single-layer model (e.g. [`Grid::blit`]). Most drawing should go through
    /// [`draw`](Self::draw)/[`surface`](Self::surface) instead.
    pub const fn grid_mut(&mut self) -> &mut Grid {
        &mut self.current
    }

    /// Returns a reference to the backend.
    #[must_use]
    pub const fn backend(&self) -> &B {
        &self.backend
    }

    /// Returns a mutable reference to the backend.
    pub const fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }
}

impl<B: Backend> core::fmt::Debug for Terminal<B> {
    /// Prints `size` and `present_count`; elides the frame buffers and the backend.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Terminal")
            .field("size", &self.size())
            .field("present_count", &self.present_count)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Headless;
    use crate::color::Style;
    use crate::tile::Tile;

    #[test]
    fn test_terminal_grid_mut() {
        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);

        assert_eq!(terminal.grid()[Pos::new(0, 0)].glyph(), ' ');

        terminal
            .grid_mut()
            .put_tile(0, (0, 0), Tile::new('X', Style::default()));

        assert_eq!(terminal.grid()[Pos::new(0, 0)].glyph(), 'X');
    }

    #[test]
    fn test_terminal_size() {
        let term = Terminal::new(Headless::new(40, 20));
        assert_eq!(term.size(), Size::new(40, 20));
    }

    #[test]
    fn test_terminal_area() {
        let term = Terminal::new(Headless::new(40, 20));
        assert_eq!(term.area(), Rect::new(0, 0, 40, 20));
    }

    #[test]
    #[should_panic(expected = "Grid width must be at least 1")]
    fn test_terminal_new_zero_width_backend_panics() {
        let _ = Terminal::new(Headless::new(0, 5));
    }

    #[test]
    fn test_terminal_new_zero_height_backend_does_not_panic() {
        let term = Terminal::new(Headless::new(5, 0));
        assert_eq!(term.size(), Size::new(5, 0));
    }

    #[test]
    #[should_panic(expected = "Grid width must be at least 1")]
    fn test_terminal_new_zero_by_zero_backend_panics() {
        let _ = Terminal::new(Headless::new(0, 0));
    }

    #[test]
    fn test_terminal_resize_to_zero_by_zero_is_allowed() {
        let mut term = Terminal::new(Headless::new(10, 10));
        term.resize(0, 0);
        assert_eq!(term.size(), Size::new(0, 0));
        // Resizing back up afterwards still works.
        term.resize(10, 10);
        assert_eq!(term.size(), Size::new(10, 10));
    }

    #[test]
    #[should_panic(expected = "Grid width must be at least 1")]
    fn test_terminal_present_multi_layer_at_zero_width_panics() {
        // Pins the `# Panics` case documented on `resize`: once layer 1 is allocated, resizing
        // down to zero width and presenting hits `present`'s multi-layer flatten branch, which
        // rebuilds its buffers with `Grid::new` at the current size and panics (retroglyph#1130).
        //
        // Allocate layer 1 through `surface()` rather than `draw()`: `draw` also presents, which
        // swaps `current`/`previous` and clears the new `current`, undoing the allocation before
        // this test can resize. A `put` on a layer at zero width would also miss: it is clipped
        // out by the (now zero-width) surface area and never allocates the layer at all.
        let mut term = Terminal::new(Headless::new(10, 10));
        term.surface()
            .on_layer(1)
            .put((0, 0), 'B', Style::default());
        term.resize(0, 10);
        let _ = term.present();
    }

    #[test]
    fn test_terminal_cursor_passthroughs_forward_to_backend() {
        let mut term = Terminal::new(Headless::new(10, 10));

        term.set_cursor_visible(true);
        assert!(term.backend().cursor_visible());

        term.set_cursor_position(Pos::new(3, 4));
        assert_eq!(term.backend().cursor_position(), Pos::new(3, 4));

        term.set_cursor_style(CursorStyle::SteadyBar);
        assert_eq!(term.backend().cursor_style(), CursorStyle::SteadyBar);
    }

    #[test]
    fn test_terminal_resize_changes_dimensions() {
        let mut term = Terminal::new(Headless::new(10, 10));
        term.resize(30, 15);
        assert_eq!(term.size(), Size::new(30, 15));
        assert_eq!(term.grid().width(), 30);
        assert_eq!(term.grid().height(), 15);
    }

    #[test]
    fn test_terminal_resize_preserves_current_content() {
        // Writes through `surface()` rather than `draw()`, so `current` is inspected before any
        // `present()` clears it: `draw()` always presents, which would swap this content out to
        // `previous` and clear the new `current` before the assertions below could see it.
        let mut term = Terminal::new(Headless::new(10, 10));
        term.surface().put((2, 2), 'X', Style::default());
        term.resize(20, 20);
        assert_eq!(term.grid()[Pos::new(2, 2)].glyph(), 'X');
        assert_eq!(term.grid()[Pos::new(15, 15)].glyph(), ' ');
    }

    #[test]
    fn test_terminal_resize_event_auto_applies() {
        let mut term = Terminal::new(Headless::new(10, 10));
        term.backend_mut().push_event(Event::Resize(80, 25));
        let event = term.poll(core::time::Duration::ZERO);
        assert_eq!(event, Some(Event::Resize(80, 25)));
        assert_eq!(term.size(), Size::new(80, 25));
    }

    #[test]
    fn test_terminal_resize_after_flatten_buffers_allocated() {
        // Draw to layer 1 first so `present` takes the flatten path and lazily allocates
        // `flattened_current`/`flattened_previous` (see their field docs). `resize` must then
        // resize and clear those buffers too, not just `current`/`previous`, or a later present
        // would diff against stale, wrongly-sized flattened content.
        let mut term = Terminal::new(Headless::new(3, 3));
        term.draw(|s| {
            s.put((0, 0), 'A', Style::default());
            s.on_layer(1).put((1, 1), 'B', Style::default());
        })
        .expect("draw failed");

        term.resize(5, 5);
        assert_eq!(term.size(), Size::new(5, 5));

        // A full redraw is expected after resize; if the flattened buffers weren't resized and
        // cleared alongside `current`/`previous`, this would panic on mismatched grid sizes or
        // silently under-diff instead of redrawing everything.
        term.draw(|s| {
            s.put((0, 0), 'A', Style::default());
            s.on_layer(1).put((1, 1), 'B', Style::default());
        })
        .expect("draw failed");
        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), 'A');
        assert_eq!(term.backend().grid()[Pos::new(1, 1)].glyph(), 'B');
    }

    #[test]
    fn test_terminal_resize_new_cells_accessible() {
        // Resize to a larger area, then draw into the newly created region.
        let mut term = Terminal::new(Headless::new(3, 3));
        term.draw(|s| s.put((0, 0), 'A', Style::default()))
            .expect("draw failed");

        term.resize(5, 5);

        // Draw into the expanded region and verify it reaches the backend.
        term.draw(|s| s.put((4, 4), 'B', Style::default()))
            .expect("draw failed");

        assert_eq!(term.backend().grid()[Pos::new(4, 4)].glyph(), 'B');
        // (0,0) was not redrawn this frame; backend retains 'A' from before resize.
        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), 'A');
    }

    #[test]
    fn test_terminal_resize_clears_pending_retention() {
        use crate::surface::Layer;

        // `resize` clearing `previous` alone can't distinguish "retention cleared" from
        // "retention still active": a retained blit from an already-blank `previous` looks the
        // same as no retention at all. So this redraws `World` with new content right after the
        // resize: if retention had survived, `present`'s pre-diff blit would silently overwrite
        // that fresh draw with `previous`'s (blank) content before the diff ever ran, leaving the
        // backend showing stale pre-resize content instead of the new frame.
        let mut term = Terminal::new(Headless::new(3, 1));
        term.draw(|s| s.on_tier(Layer::World).put((0, 0), 'W', Style::default()))
            .expect("draw failed");

        term.retain_layer(Layer::World);
        term.resize(3, 1);

        term.draw(|s| s.on_tier(Layer::World).put((0, 0), 'X', Style::default()))
            .expect("draw failed");
        assert_eq!(
            term.backend().grid()[Pos::new(0, 0)].glyph(),
            'X',
            "pending retention must not survive resize: a still-active blit from the \
             (resize-cleared) previous frame would have overwritten this fresh draw"
        );
    }
}
