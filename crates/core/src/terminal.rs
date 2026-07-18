//! Stateful terminal management and double-buffering.

use crate::backend::Backend;
use crate::color::Color;
use crate::event::Event;
use crate::grid::{Grid, Pos, Rect, Size};
use crate::style::Style;
use crate::text::Line;
use crate::tile::Tile;
use core::time::Duration;
#[cfg(not(feature = "egc"))]
use unicode_width::UnicodeWidthChar;

/// A double-buffered terminal generic over a [`Backend`].
///
/// Owns the current and previous frame grids and exposes a stateful drawing
/// API (`put`, `print`, `layer`, ...). Call [`present`](Self::present) once
/// per frame to diff against the previous frame and send only the changed
/// cells to the backend.
pub struct Terminal<B: Backend> {
    current: Grid,
    previous: Grid,
    /// Single-layer scratch buffers used only when the backend does not
    /// composite layers itself. `present` flattens `current` into
    /// `flattened_current`, diffs it against `flattened_previous`, and sends the
    /// result. Unused (but allocated) for compositing backends.
    flattened_current: Grid,
    flattened_previous: Grid,
    backend: B,
    drawing_style: Style,
    queued_event: Option<Event>,
    /// The layer that `put`, `put_styled`, and `put_offset` write to.
    active_layer: u8,
    /// `true` when the flatten buffers no longer reflect the last frame sent to
    /// the backend (because the single-layer fast path bypassed them). The next
    /// multi-layer present clears `flattened_previous` first so it does a full
    /// redraw instead of diffing against stale data.
    flattened_stale: bool,
}

impl<B: Backend> Terminal<B> {
    /// Create a terminal with the given backend.
    /// Grid dimensions are queried from the backend.
    #[must_use]
    pub fn new(backend: B) -> Self {
        let size = backend.size();
        let current = Grid::new(size.width, size.height);
        let previous = Grid::new(size.width, size.height);
        let flattened_current = Grid::new(size.width, size.height);
        let flattened_previous = Grid::new(size.width, size.height);
        Self {
            current,
            previous,
            flattened_current,
            flattened_previous,
            backend,
            drawing_style: Style::default(),
            queued_event: None,
            active_layer: 0,
            flattened_stale: false,
        }
    }

    /// Sets the active drawing layer (0-255). Returns `&mut Self` for chaining.
    ///
    /// All subsequent `put`, `put_styled`, `put_offset`, `print`, and
    /// `print_styled` calls write to this layer until `layer()` is called again.
    pub const fn layer(&mut self, layer: u8) -> &mut Self {
        self.active_layer = layer;
        self
    }

    /// Sets the foreground color for the stateful API.
    pub const fn fg(&mut self, color: Color) -> &mut Self {
        self.drawing_style.fg = color;
        self
    }

    /// Sets the background color for the stateful API.
    pub const fn bg(&mut self, color: Color) -> &mut Self {
        self.drawing_style.bg = color;
        self
    }

    /// Resets the drawing style to defaults.
    pub fn reset_style(&mut self) -> &mut Self {
        self.drawing_style = Style::default();
        self
    }

    /// Returns the current drawing style.
    #[must_use]
    pub const fn style(&self) -> Style {
        self.drawing_style
    }

    /// Returns the current grid dimensions.
    #[must_use]
    pub const fn size(&self) -> Size {
        Size {
            width: self.current.width(),
            height: self.current.height(),
        }
    }

    /// Returns the full drawing surface as a [`Rect`] at the origin.
    ///
    /// Equivalent to `Rect::new(0, 0, width, height)`. Handy for passing the
    /// whole terminal to layout helpers or region-based drawing.
    #[must_use]
    pub const fn area(&self) -> Rect {
        Rect::new(0, 0, self.current.width(), self.current.height())
    }

    /// Resize both grids to `width` × `height` cells.
    ///
    /// Content within the overlapping region is preserved in the current grid.
    /// The previous grid is cleared so the next [`present`](Self::present) redraws
    /// the entire new surface rather than diffing stale data.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.current.resize(width, height);
        self.previous.resize(width, height);
        self.flattened_current.resize(width, height);
        self.flattened_previous.resize(width, height);
        // Clearing previous forces a full redraw next present(), ensuring no
        // stale cells bleed into the resized layout.
        self.previous.clear_all();
        self.flattened_previous.clear_all();
        self.backend.resize(Size { width, height });
    }

    /// Place a character at `(x, y)` on the active layer with the current style.
    ///
    /// If `ch` is a wide character (e.g. CJK or emoji) that occupies two columns,
    /// the adjacent cell at `(x + 1, y)` is set to a zero-width continuation
    /// marker so it is not rendered independently.
    ///
    /// Sub-cell offsets are always visual only — use [`put_offset`](Self::put_offset)
    /// for offset writes.
    pub fn put(&mut self, x: u16, y: u16, ch: char) {
        let style = self.drawing_style;
        #[cfg(feature = "egc")]
        {
            let mut buf = [0u8; 4];
            let s = ch.encode_utf8(&mut buf);
            self.current
                .write_grapheme(self.active_layer, x, y, s, style);
        }
        #[cfg(not(feature = "egc"))]
        {
            let tile = Tile::new(ch, style);
            self.current.put_tile(self.active_layer, x, y, tile);
        }
    }

    /// Place a character at `pos` on the active layer with the current style.
    ///
    /// Equivalent to [`put`](Self::put), but takes a [`Pos`] to match the
    /// `Rect`/`Size`-based drawing APIs elsewhere on `Terminal`.
    pub fn put_at(&mut self, pos: Pos, ch: char) {
        self.put(pos.x, pos.y, ch);
    }

    /// Returns a reference to the current grid.
    #[must_use]
    pub const fn grid(&self) -> &Grid {
        &self.current
    }

    /// Returns a mutable reference to the current grid.
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

    /// Clear the active layer.
    pub fn clear(&mut self) {
        self.current.clear(self.active_layer);
    }

    /// Clear every allocated layer.
    pub fn clear_all(&mut self) {
        self.current.clear_all();
    }

    /// Clear a rectangular region.
    pub fn clear_region(&mut self, rect: Rect) {
        for y in rect.top()..rect.bottom() {
            for x in rect.left()..rect.right() {
                if let Some(cell) = self.current.checked_get_mut(x, y) {
                    *cell = Tile::default();
                }
            }
        }
    }

    /// Place a character on the active layer with an explicit style.
    pub fn put_styled(&mut self, x: u16, y: u16, ch: char, style: Style) {
        #[cfg(feature = "egc")]
        {
            let mut buf = [0u8; 4];
            let s = ch.encode_utf8(&mut buf);
            self.current
                .write_grapheme(self.active_layer, x, y, s, style);
        }
        #[cfg(not(feature = "egc"))]
        {
            let tile = Tile::new(ch, style);
            self.current.put_tile(self.active_layer, x, y, tile);
        }
    }

    /// Place a character at `(x, y)` with a sub-cell pixel offset `(dx, dy)`.
    ///
    /// Uses the current style and active layer. Sub-cell offsets are visual
    /// only — they do not affect grid logic or hit-testing. Backends that
    /// cannot represent pixel offsets (e.g. `CrosstermBackend`) ignore them.
    pub fn put_offset(&mut self, x: u16, y: u16, dx: i16, dy: i16, ch: char) {
        let tile = Tile::new(ch, self.drawing_style).with_offset(dx, dy);
        self.current.put_tile(self.active_layer, x, y, tile);
    }

    /// Print a string starting at `(x, y)` with the current style.
    ///
    /// `\n` advances to the next row at the original `x`. Wide characters
    /// (CJK, emoji) advance the cursor by 2 columns. Characters that would
    /// extend beyond the grid width wrap to the next row.
    pub fn print(&mut self, x: u16, y: u16, text: &str) {
        let style = self.drawing_style;
        #[cfg(feature = "egc")]
        self.print_str_egc(x, y, text, style);
        #[cfg(not(feature = "egc"))]
        self.print_str_chars(x, y, text, style);
    }

    /// Print a [`Line`] of styled spans starting at `(x, y)`.
    ///
    /// Each span's style is applied independently. The terminal's current
    /// drawing style is not modified. Wide characters advance the cursor by
    /// 2 columns. Rendering stops at the grid boundary.
    pub fn print_styled(&mut self, x: u16, y: u16, line: &Line) {
        #[cfg(feature = "egc")]
        {
            use unicode_segmentation::UnicodeSegmentation;
            use unicode_width::UnicodeWidthStr;
            let mut cur_x = x;
            for span in &line.spans {
                for grapheme in span.content.graphemes(true) {
                    if grapheme == "\n" {
                        break;
                    }
                    #[allow(clippy::cast_possible_truncation)]
                    let w = grapheme.width() as u16;
                    if w == 0 {
                        continue;
                    }
                    if cur_x >= self.current.width() {
                        break;
                    }
                    self.current
                        .write_grapheme(self.active_layer, cur_x, y, grapheme, span.style);
                    cur_x += w;
                }
            }
        }
        #[cfg(not(feature = "egc"))]
        {
            use unicode_width::UnicodeWidthChar;
            let mut cur_x = x;
            for span in &line.spans {
                for ch in span.content.chars() {
                    if ch == '\n' {
                        break;
                    }
                    #[allow(clippy::cast_possible_truncation)]
                    let w = UnicodeWidthChar::width(ch).unwrap_or(1) as u16;
                    if usize::from(cur_x) >= usize::from(self.current.width()) {
                        break;
                    }
                    let tile = Tile::new(ch, span.style);
                    self.current.put_tile(self.active_layer, cur_x, y, tile);
                    cur_x += w;
                }
            }
        }
    }

    /// Render a [`Line`] of styled text into a bounded rectangle.
    ///
    /// Performs greedy word-wrapping at `rect`'s width, then positions the
    /// resulting lines according to `h_align` and `v_align`. Lines that
    /// overflow `rect`'s height are silently clipped.
    ///
    /// This is a convenience wrapper around [`TextLayout`](crate::layout::TextLayout).
    ///
    /// Only available when the `egc` feature is enabled.
    #[cfg(feature = "egc")]
    pub fn print_box(
        &mut self,
        rect: Rect,
        line: &Line,
        h_align: crate::layout::HAlign,
        v_align: crate::layout::VAlign,
    ) {
        crate::layout::TextLayout::new(line)
            .rect(rect)
            .h_align(h_align)
            .v_align(v_align)
            .render(self);
    }

    /// Present the current frame.
    ///
    /// Computes diff, sends changed cells to the backend, flushes, then swaps buffers.
    ///
    /// When the backend requires a full frame (see
    /// [`crate::Backend::needs_full_frame`]), all cells from every allocated layer are
    /// sent rather than just the diff, so pixel-based backends can clear and
    /// redraw to avoid orphaned pixels from sub-cell offsets.
    ///
    /// After swap the new current buffer is cleared so the next frame starts
    /// empty. Callers should not call `clear()` before drawing the next frame.
    ///
    /// # Immediate mode
    ///
    /// This is an immediate-mode API (the same trade [ratatui] makes): the
    /// current buffer is wiped after every present, so each frame must redraw
    /// its entire scene from scratch. Cells are **not** retained between
    /// frames. The diff only bounds what is sent to the backend (terminal or
    /// pixel I/O); it does not bound the CPU cost of your redraw.
    ///
    /// Turn-based games that render only when state changes should gate their
    /// calls to `present` on an actual state change rather than presenting on a
    /// fixed clock and expecting the previous frame's cells to persist.
    ///
    /// [ratatui]: https://docs.rs/ratatui
    ///
    /// # Errors
    ///
    /// Propagates errors from the backend's
    /// [`draw_layers`](crate::Backend::draw_layers) or
    /// [`flush`](crate::Backend::flush) operations.
    pub fn present(&mut self) -> Result<(), <B as Backend>::Error> {
        if self.backend.composites_layers() {
            // Pixel/GPU backends composite the raw layered stream themselves.
            if self.backend.needs_full_frame() {
                let all = self.current.layers();
                self.backend.draw_layers(all)?;
            } else {
                let diff = self.current.diff(&self.previous);
                self.backend.draw_layers(diff)?;
            }
        } else if self.current.max_layer() == 0 && self.previous.max_layer() == 0 {
            // Fast path: only layer 0 is in play, so flattening would be an exact
            // copy of `current`. Diff the real grids directly and skip the
            // flatten buffers entirely.
            let diff = self.current.diff(&self.previous);
            self.backend.draw_layers(diff)?;
            self.flattened_stale = true;
        } else {
            // Cell backends receive a pre-flattened, single-layer diff so layers
            // 1+ appear everywhere, not just on pixel backends.
            if self.flattened_stale {
                // The previous frame used the fast path, so `flattened_previous`
                // is stale. Clear it to force a full redraw this frame.
                self.flattened_previous.clear_all();
                self.flattened_stale = false;
            }
            self.current.flatten_into(&mut self.flattened_current);
            let diff = self.flattened_current.diff(&self.flattened_previous);
            self.backend.draw_layers(diff)?;
            core::mem::swap(&mut self.flattened_current, &mut self.flattened_previous);
        }
        self.backend.flush()?;
        core::mem::swap(&mut self.current, &mut self.previous);
        self.current.clear_all();
        Ok(())
    }

    /// Polls for an input event, waiting up to `timeout`.
    ///
    /// If an event was previously buffered by [`has_input`](Self::has_input), it is
    /// returned immediately. Otherwise, the backend is polled for a new event.
    ///
    /// [`Event::Resize`] events are automatically applied: both grids are resized
    /// before the event is returned to the caller, so the game loop can immediately
    /// redraw at the new size.
    pub fn poll(&mut self, timeout: Duration) -> Option<Event> {
        let event = self
            .queued_event
            .take()
            .or_else(|| self.backend.poll_event(timeout))?;
        if let Event::Resize(w, h) = event {
            self.resize(w, h);
        }
        Some(event)
    }

    /// Reads an input event, blocking indefinitely until one is available.
    ///
    /// Only call this on backends that genuinely block (e.g. crossterm, window). Backends
    /// that never block (e.g. [`Headless`](crate::backend::Headless), which returns
    /// immediately regardless of timeout) will panic here once their event queue is
    /// empty; use [`poll`](Self::poll) or [`drain_events`](Self::drain_events) instead if
    /// that is a possibility.
    ///
    /// # Panics
    ///
    /// Panics if the backend's [`poll_event`](crate::Backend::poll_event) returns
    /// `None` even with an unbounded timeout.
    pub fn read_blocking(&mut self) -> Event {
        self.poll(Duration::MAX)
            .expect("read_blocking() called but no events available")
    }

    /// Drains all available events without blocking.
    ///
    /// Returns an iterator that yields every pending event — the internal queued event
    /// followed by all events buffered in the backend. The iterator polls the backend
    /// with zero timeout repeatedly until `None` is returned.
    ///
    /// This is needed for frame-based game loops (e.g. software backend + WASM, where
    /// frames are gated by `requestAnimationFrame`). Multiple keypresses can arrive
    /// between frames; draining all of them ensures accumulated input doesn't replay in
    /// slow motion.
    ///
    /// Crossterm and headless backends can also use this, but the single-event `poll`
    /// pattern works for them because their loops aren't frame-capped.
    pub fn drain_events(&mut self) -> impl Iterator<Item = Event> + use<'_, B> {
        struct DrainEvents<'a, B: Backend> {
            terminal: &'a mut Terminal<B>,
        }

        impl<B: Backend> Iterator for DrainEvents<'_, B> {
            type Item = Event;

            fn next(&mut self) -> Option<Event> {
                self.terminal.poll(Duration::ZERO)
            }
        }

        impl<B: Backend> core::iter::FusedIterator for DrainEvents<'_, B> {}

        DrainEvents { terminal: self }
    }

    /// Checks if a pending input event is available without blocking.
    ///
    /// If an event is already buffered, returns `true`. Otherwise, polls the backend
    /// with zero timeout. If the backend returns an event, it is stored in the internal
    /// buffer and `true` is returned; otherwise, returns `false`.
    pub fn has_input(&mut self) -> bool {
        if self.queued_event.is_some() {
            true
        } else if let Some(event) = self.backend.poll_event(Duration::ZERO) {
            self.queued_event = Some(event);
            true
        } else {
            false
        }
    }

    /// String printing implementation used when `egc` is enabled.
    #[cfg(feature = "egc")]
    fn print_str_egc(&mut self, x: u16, y: u16, text: &str, style: Style) {
        use unicode_segmentation::UnicodeSegmentation;
        use unicode_width::UnicodeWidthStr;
        let layer = self.active_layer;
        let mut cur_x = x;
        let mut cur_y = y;
        for grapheme in text.graphemes(true) {
            if grapheme == "\n" {
                cur_x = x;
                cur_y += 1;
                continue;
            }
            #[allow(clippy::cast_possible_truncation)]
            let w = grapheme.width() as u16;
            if w == 0 {
                continue;
            }
            self.current
                .write_grapheme(layer, cur_x, cur_y, grapheme, style);
            cur_x += w;
            if cur_x >= self.current.width() {
                cur_x = x;
                cur_y += 1;
            }
        }
    }

    /// String printing implementation used when `egc` is disabled.
    #[cfg(not(feature = "egc"))]
    fn print_str_chars(&mut self, x: u16, y: u16, text: &str, style: Style) {
        let mut cur_x = x;
        let mut cur_y = y;
        for c in text.chars() {
            if c == '\n' {
                cur_x = x;
                cur_y += 1;
            } else {
                #[allow(clippy::cast_possible_truncation)]
                let w = UnicodeWidthChar::width(c).unwrap_or(1) as u16;
                let tile = Tile::new(c, style);
                self.current.put_tile(self.active_layer, cur_x, cur_y, tile);
                cur_x += w;
                if usize::from(cur_x) >= usize::from(self.current.width()) {
                    cur_x = x;
                    cur_y += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Headless;
    use crate::tile::Tile;

    #[test]
    fn test_terminal_grid_mut() {
        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);

        assert_eq!(terminal.grid().get(0, 0).glyph(), ' ');

        terminal
            .grid_mut()
            .put(0, 0, Tile::new('X', Style::default()));

        assert_eq!(terminal.grid().get(0, 0).glyph(), 'X');
    }

    #[test]
    fn test_terminal_poll_and_read() {
        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);

        assert_eq!(terminal.poll(Duration::ZERO), None);

        terminal.backend_mut().push_event(Event::Close);
        assert_eq!(terminal.poll(Duration::ZERO), Some(Event::Close));

        terminal.backend_mut().push_event(Event::Resize(80, 25));
        assert_eq!(terminal.read_blocking(), Event::Resize(80, 25));
    }

    #[test]
    fn test_terminal_has_input() {
        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);

        assert!(!terminal.has_input());

        terminal.backend_mut().push_event(Event::Close);
        assert!(terminal.has_input());
        assert!(terminal.has_input()); // Repeated calls should still be true

        // Read/Poll should retrieve the buffered event
        assert_eq!(terminal.poll(Duration::ZERO), Some(Event::Close));

        // After taking, it should be false again
        assert!(!terminal.has_input());
    }

    #[test]
    #[should_panic(expected = "read_blocking() called but no events available")]
    fn test_terminal_read_panic() {
        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);
        let _ = terminal.read_blocking();
    }

    // --- resize ---

    #[test]
    fn test_present_composites_layers_for_cell_backend() {
        // A cell backend (Headless) must see layers 1+ composited, not
        // dropped. Terrain on layer 0, entity on layer 1.
        let mut term = Terminal::new(Headless::new(3, 1));
        term.layer(0).put(0, 0, '.');
        term.layer(0).put(1, 0, '.');
        term.layer(1).put(1, 0, '@');
        term.present().expect("present failed");
        assert_eq!(term.backend().grid().get(0, 0).glyph(), '.');
        // Layer 1's glyph wins at (1, 0).
        assert_eq!(term.backend().grid().get(1, 0).glyph(), '@');
    }

    #[test]
    fn test_present_explicit_space_on_higher_layer_erases_and_sets_bg() {
        // An explicit space on a higher layer is opaque: it overwrites the
        // glyph beneath (erase) and applies its background. This is the
        // deliberate consequence of the explicit-EMPTY transparency model.
        let mut term = Terminal::new(Headless::new(2, 1));
        term.layer(0).put(0, 0, 'x');
        term.layer(1)
            .put_styled(0, 0, ' ', Style::new().bg(Color::RED));
        term.present().expect("present failed");
        let cell = term.backend().grid().get(0, 0);
        assert_eq!(cell.glyph(), ' ');
        assert_eq!(cell.style().background(), Color::RED);
    }

    #[test]
    fn test_present_single_layer_fast_path_matches_backend() {
        // Only layer 0 is ever touched: the fast path must still deliver the
        // correct cells to a cell backend across multiple frames.
        let mut term = Terminal::new(Headless::new(3, 1));
        term.put(0, 0, 'a');
        term.present().expect("present failed");
        assert_eq!(term.backend().grid().get(0, 0).glyph(), 'a');

        // Immediate mode: redraw 'a' and add 'c'. The diff updates the new
        // cell while 'a' stays put.
        term.put(0, 0, 'a');
        term.put(2, 0, 'c');
        term.present().expect("present failed");
        assert_eq!(term.backend().grid().get(0, 0).glyph(), 'a');
        assert_eq!(term.backend().grid().get(2, 0).glyph(), 'c');

        // A cell that is not redrawn is erased (immediate mode).
        term.put(0, 0, 'a');
        term.present().expect("present failed");
        assert_eq!(term.backend().grid().get(0, 0).glyph(), 'a');
        assert_eq!(term.backend().grid().get(2, 0).glyph(), ' ');
    }

    #[test]
    fn test_present_transition_single_to_multi_layer() {
        // Start single-layer (fast path), then introduce layer 1. The frame
        // that adds the layer must composite correctly despite the fast path
        // having bypassed the flatten buffers.
        let mut term = Terminal::new(Headless::new(2, 1));
        term.layer(0).put(0, 0, '.');
        term.layer(0).put(1, 0, '.');
        term.present().expect("present failed");

        term.layer(0).put(0, 0, '.');
        term.layer(0).put(1, 0, '.');
        term.layer(1).put(1, 0, '@');
        term.present().expect("present failed");
        assert_eq!(term.backend().grid().get(0, 0).glyph(), '.');
        assert_eq!(term.backend().grid().get(1, 0).glyph(), '@');
    }

    #[test]
    fn test_present_untouched_higher_layer_is_transparent() {
        // A higher layer that was allocated but not written at this cell must
        // not disturb the lower layer's glyph or background.
        let mut term = Terminal::new(Headless::new(2, 1));
        term.layer(0).put(0, 0, 'x');
        // Allocate layer 1 by writing elsewhere, leaving (0, 0) empty.
        term.layer(1).put(1, 0, 'y');
        term.present().expect("present failed");
        assert_eq!(term.backend().grid().get(0, 0).glyph(), 'x');
    }

    #[test]
    fn test_terminal_size() {
        let term = Terminal::new(Headless::new(40, 20));
        assert_eq!(
            term.size(),
            Size {
                width: 40,
                height: 20
            }
        );
    }

    #[test]
    fn test_terminal_area() {
        let term = Terminal::new(Headless::new(40, 20));
        assert_eq!(term.area(), Rect::new(0, 0, 40, 20));
    }

    #[test]
    fn test_terminal_resize_changes_dimensions() {
        let mut term = Terminal::new(Headless::new(10, 10));
        term.resize(30, 15);
        assert_eq!(
            term.size(),
            Size {
                width: 30,
                height: 15
            }
        );
        assert_eq!(term.grid().width(), 30);
        assert_eq!(term.grid().height(), 15);
    }

    #[test]
    fn test_terminal_resize_preserves_current_content() {
        let mut term = Terminal::new(Headless::new(10, 10));
        term.put(2, 2, 'X');
        term.resize(20, 20);
        assert_eq!(term.grid().get(2, 2).glyph(), 'X');
        assert_eq!(term.grid().get(15, 15).glyph(), ' ');
    }

    #[test]
    fn test_terminal_resize_event_auto_applies() {
        let mut term = Terminal::new(Headless::new(10, 10));
        term.backend_mut().push_event(Event::Resize(80, 25));
        let event = term.poll(Duration::ZERO);
        assert_eq!(event, Some(Event::Resize(80, 25)));
        assert_eq!(
            term.size(),
            Size {
                width: 80,
                height: 25
            }
        );
    }

    #[test]
    fn test_terminal_resize_new_cells_accessible() {
        // Resize to a larger area, then draw in the newly created region.
        let mut term = Terminal::new(Headless::new(3, 3));
        term.put(0, 0, 'A');
        term.present();

        term.resize(5, 5);

        // Draw into the expanded region and verify it reaches the backend.
        term.put(4, 4, 'B');
        term.present();

        assert_eq!(term.backend().grid().get(4, 4).glyph(), 'B');
        // (0,0) was not redrawn this frame; backend retains 'A' from before resize.
        assert_eq!(term.backend().grid().get(0, 0).glyph(), 'A');
    }

    // --- unicode width ---

    #[test]
    fn test_put_wide_char_sets_continuation() {
        let mut term = Terminal::new(Headless::new(10, 3));
        term.put(0, 0, '\u{4e2d}'); // '中', width 2
        assert_eq!(term.grid().get(0, 0).glyph(), '\u{4e2d}');
        // With egc: spacer uses WIDE_CHAR_SPACER flag, glyph is space.
        // Without egc: spacer is '\0'.
        #[cfg(feature = "egc")]
        {
            use crate::tile::TileFlags;
            assert!(
                term.grid()
                    .get(1, 0)
                    .flags()
                    .contains(TileFlags::WIDE_CHAR_SPACER)
            );
            assert_eq!(term.grid().get(1, 0).glyph(), ' ');
        }
        #[cfg(not(feature = "egc"))]
        assert_eq!(term.grid().get(1, 0).glyph(), '\0');
        assert_eq!(term.grid().get(2, 0).glyph(), ' '); // untouched
    }

    #[test]
    fn test_print_advances_by_char_width() {
        let mut term = Terminal::new(Headless::new(10, 3));
        term.print(0, 0, "\u{4e2d}x"); // '中' (2) then 'x' at col 2
        assert_eq!(term.grid().get(0, 0).glyph(), '\u{4e2d}');
        #[cfg(feature = "egc")]
        {
            use crate::tile::TileFlags;
            assert!(
                term.grid()
                    .get(1, 0)
                    .flags()
                    .contains(TileFlags::WIDE_CHAR_SPACER)
            );
        }
        #[cfg(not(feature = "egc"))]
        assert_eq!(term.grid().get(1, 0).glyph(), '\0');
        assert_eq!(term.grid().get(2, 0).glyph(), 'x');
    }

    #[test]
    fn test_put_at_matches_put() {
        let mut term = Terminal::new(Headless::new(10, 3));
        term.put_at(Pos::new(2, 1), 'X');
        assert_eq!(term.grid().get(2, 1).glyph(), 'X');
    }

    #[test]
    fn test_put_wide_char_at_last_column_does_not_overflow() {
        // Wide char placed at the last column: can't place a spacer.
        // write_grapheme silently refuses rather than leaving an orphan.
        let mut term = Terminal::new(Headless::new(4, 1));
        term.put(3, 0, '\u{4e2d}'); // col 3 is last; need col 4 for spacer
        assert_eq!(term.grid().get(3, 0).glyph(), ' '); // nothing written
    }

    // --- styled spans ---

    #[test]
    fn test_print_styled_basic() {
        use crate::text::{Line, Span};
        let mut term = Terminal::new(Headless::new(20, 3));
        let line = Line::from(vec![
            Span::raw("HP: "),
            Span::styled("100", Style::new().fg(Color::GREEN)),
        ]);
        term.print_styled(0, 0, &line);
        assert_eq!(term.grid().get(0, 0).glyph(), 'H');
        assert_eq!(term.grid().get(3, 0).glyph(), ' ');
        assert_eq!(term.grid().get(4, 0).glyph(), '1');
        assert_eq!(term.grid().get(4, 0).style.fg, Color::GREEN);
        assert_eq!(term.grid().get(6, 0).glyph(), '0');
    }

    #[test]
    fn test_print_styled_does_not_modify_drawing_style() {
        use crate::text::{Line, Span};
        let mut term = Terminal::new(Headless::new(20, 3));
        term.fg(Color::RED);
        let line = Line::from(vec![Span::styled("hi", Style::new().fg(Color::BLUE))]);
        term.print_styled(0, 0, &line);
        // Drawing style must be unchanged.
        assert_eq!(term.style().fg, Color::RED);
    }

    #[test]
    fn test_print_styled_wide_chars() {
        use crate::text::{Line, Span};
        let mut term = Terminal::new(Headless::new(10, 3));
        let line = Line::from(vec![Span::raw("\u{4e2d}x")]);
        term.print_styled(0, 0, &line);
        assert_eq!(term.grid().get(0, 0).glyph(), '\u{4e2d}');
        #[cfg(feature = "egc")]
        {
            use crate::tile::TileFlags;
            assert!(
                term.grid()
                    .get(1, 0)
                    .flags()
                    .contains(TileFlags::WIDE_CHAR_SPACER)
            );
        }
        #[cfg(not(feature = "egc"))]
        assert_eq!(term.grid().get(1, 0).glyph(), '\0');
        assert_eq!(term.grid().get(2, 0).glyph(), 'x');
    }
}
