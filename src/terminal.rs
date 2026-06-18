//! Stateful terminal management and double-buffering.

use crate::backend::Backend;
use crate::cell::Cell;
use crate::color::Color;
use crate::event::Event;
use crate::grid::{Grid, Rect, Size};
use crate::style::{CellModifier, Style};
use crate::text::Line;
use core::time::Duration;
use unicode_width::UnicodeWidthChar;

/// The main entry point for `rg`.
///
/// Generic over the backend. Owns a double-buffered grid and provides
/// a stateful drawing API.
pub struct Terminal<B: Backend> {
    current: Grid,
    previous: Grid,
    backend: B,
    drawing_style: Style,
    queued_event: Option<Event>,
}

impl<B: Backend> Terminal<B> {
    /// Create a terminal with the given backend.
    /// Grid dimensions are queried from the backend.
    #[must_use]
    pub fn new(backend: B) -> Self {
        let size = backend.size();
        let current = Grid::new(size.width, size.height);
        let previous = Grid::new(size.width, size.height);
        Self {
            current,
            previous,
            backend,
            drawing_style: Style::default(),
            queued_event: None,
        }
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

    /// Sets text modifiers for the stateful API.
    pub const fn modifier(&mut self, modifier: CellModifier) -> &mut Self {
        self.drawing_style.modifiers = modifier;
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

    /// Resize both grids to `width` × `height` cells.
    ///
    /// Content within the overlapping region is preserved in the current grid.
    /// The previous grid is cleared so the next [`present`](Self::present) redraws
    /// the entire new surface rather than diffing stale data.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.current.resize(width, height);
        self.previous.resize(width, height);
        // Clearing previous forces a full redraw next present(), ensuring no
        // stale cells bleed into the resized layout.
        self.previous.clear();
        self.backend.resize(Size { width, height });
    }

    /// Place a character at `(x, y)` with the current style.
    ///
    /// If `ch` is a wide character (e.g. CJK or emoji) that occupies two columns,
    /// the adjacent cell at `(x + 1, y)` is set to a zero-width continuation
    /// marker so it is not rendered independently.
    pub fn put(&mut self, x: u16, y: u16, ch: char) {
        let style = self.drawing_style;
        self.put_cell(x, y, ch, style);
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

    /// Clear the entire grid.
    pub fn clear(&mut self) {
        self.current.clear();
    }

    /// Clear a rectangular region.
    pub fn clear_region(&mut self, rect: Rect) {
        for y in rect.y..(rect.y + rect.height) {
            for x in rect.x..(rect.x + rect.width) {
                if let Some(cell) = self.current.checked_get_mut(x, y) {
                    *cell = Cell::default();
                }
            }
        }
    }

    /// Place a character with an explicit style, ignoring the current drawing state.
    ///
    /// Wide characters are handled identically to [`put`](Self::put).
    pub fn put_styled(&mut self, x: u16, y: u16, ch: char, style: Style) {
        self.put_cell(x, y, ch, style);
    }

    /// Print a string starting at `(x, y)` with the current style.
    ///
    /// `\n` advances to the next row at the original `x`. Wide characters
    /// (CJK, emoji) advance the cursor by 2 columns. Characters that would
    /// extend beyond the grid width wrap to the next row.
    pub fn print(&mut self, x: u16, y: u16, text: &str) {
        let style = self.drawing_style;
        let mut cur_x = x;
        let mut cur_y = y;
        for c in text.chars() {
            if c == '\n' {
                cur_x = x;
                cur_y += 1;
            } else {
                // char width is always 1 or 2; u16 is safe.
                #[allow(clippy::cast_possible_truncation)]
                let w = UnicodeWidthChar::width(c).unwrap_or(1) as u16;
                self.put_cell(cur_x, cur_y, c, style);
                cur_x += w;
                if usize::from(cur_x) >= usize::from(self.current.width()) {
                    cur_x = x;
                    cur_y += 1;
                }
            }
        }
    }

    /// Print a [`Line`] of styled spans starting at `(x, y)`.
    ///
    /// Each span's style is applied independently. The terminal's current
    /// drawing style is not modified. Wide characters advance the cursor by
    /// 2 columns. Rendering stops at the grid boundary.
    pub fn print_styled(&mut self, x: u16, y: u16, line: &Line) {
        let mut cur_x = x;
        for span in &line.spans {
            for ch in span.content.chars() {
                if ch == '\n' {
                    break;
                }
                // char width is always 1 or 2; u16 is safe.
                #[allow(clippy::cast_possible_truncation)]
                let w = UnicodeWidthChar::width(ch).unwrap_or(1) as u16;
                if usize::from(cur_x) >= usize::from(self.current.width()) {
                    break;
                }
                self.put_cell(cur_x, y, ch, span.style);
                cur_x += w;
            }
        }
    }

    /// Present the current frame.
    ///
    /// Computes diff, sends changed cells to the backend, flushes, then swaps buffers.
    ///
    /// # Note
    /// The back buffer is **not** cleared automatically after presentation.
    /// If you want a blank frame, call `clear()` at the start of your loop.
    pub fn present(&mut self) {
        let diff = self.current.diff(&self.previous);
        self.backend.draw(diff);
        self.backend.flush();

        // Swap buffers: `previous` now holds the last rendered frame,
        // which will be the diff target for the next frame.
        core::mem::swap(&mut self.current, &mut self.previous);
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
    /// # Panics
    ///
    /// Panics if no event is available. This matches the expected behavior
    /// for headless backend tests when the event queue is empty.
    pub fn read(&mut self) -> Event {
        self.poll(Duration::MAX)
            .expect("read() called but no events available")
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

    /// Places `ch` with `style` at `(x, y)`, writing a `'\0'` continuation
    /// marker at `(x+1, y)` if `ch` occupies two terminal columns.
    fn put_cell(&mut self, x: u16, y: u16, ch: char, style: Style) {
        let w = UnicodeWidthChar::width(ch).unwrap_or(1);
        if let Some(cell) = self.current.checked_get_mut(x, y) {
            cell.glyph = ch;
            cell.style = style;
        }
        if w == 2 {
            if let Some(cell) = self.current.checked_get_mut(x.saturating_add(1), y) {
                cell.glyph = '\0';
                cell.style = style;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Headless;
    use crate::cell::Cell;

    #[test]
    fn test_terminal_grid_mut() {
        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);

        assert_eq!(terminal.grid().get(0, 0).glyph, ' ');

        terminal.grid_mut().put(
            0,
            0,
            Cell {
                glyph: 'X',
                style: Style::default(),
            },
        );

        assert_eq!(terminal.grid().get(0, 0).glyph, 'X');
    }

    #[test]
    fn test_terminal_poll_and_read() {
        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);

        assert_eq!(terminal.poll(Duration::ZERO), None);

        terminal.backend_mut().push_event(Event::Close);
        assert_eq!(terminal.poll(Duration::ZERO), Some(Event::Close));

        terminal.backend_mut().push_event(Event::Resize(80, 25));
        assert_eq!(terminal.read(), Event::Resize(80, 25));
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
    #[should_panic(expected = "read() called but no events available")]
    fn test_terminal_read_panic() {
        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);
        let _ = terminal.read();
    }

    // --- resize ---

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
        assert_eq!(term.grid().get(2, 2).glyph, 'X');
        assert_eq!(term.grid().get(15, 15).glyph, ' ');
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

        assert_eq!(term.backend().grid().get(4, 4).glyph, 'B');
        // (0,0) was not redrawn this frame; backend retains 'A' from before resize.
        assert_eq!(term.backend().grid().get(0, 0).glyph, 'A');
    }

    // --- unicode width ---

    #[test]
    fn test_put_wide_char_sets_continuation() {
        let mut term = Terminal::new(Headless::new(10, 3));
        term.put(0, 0, '\u{4e2d}'); // '中', width 2
        assert_eq!(term.grid().get(0, 0).glyph, '\u{4e2d}');
        assert_eq!(term.grid().get(1, 0).glyph, '\0');
        assert_eq!(term.grid().get(2, 0).glyph, ' '); // untouched
    }

    #[test]
    fn test_print_advances_by_char_width() {
        let mut term = Terminal::new(Headless::new(10, 3));
        term.print(0, 0, "\u{4e2d}x"); // '中' (2) then 'x' at col 2
        assert_eq!(term.grid().get(0, 0).glyph, '\u{4e2d}');
        assert_eq!(term.grid().get(1, 0).glyph, '\0');
        assert_eq!(term.grid().get(2, 0).glyph, 'x');
    }

    #[test]
    fn test_put_wide_char_at_last_column_does_not_overflow() {
        // Wide char placed at the last column: continuation would be out of bounds.
        let mut term = Terminal::new(Headless::new(4, 1));
        term.put(3, 0, '\u{4e2d}'); // col 3 is last; col 4 doesn't exist
        assert_eq!(term.grid().get(3, 0).glyph, '\u{4e2d}');
        // No panic — out-of-bounds continuation is silently ignored.
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
        assert_eq!(term.grid().get(0, 0).glyph, 'H');
        assert_eq!(term.grid().get(3, 0).glyph, ' ');
        assert_eq!(term.grid().get(4, 0).glyph, '1');
        assert_eq!(term.grid().get(4, 0).style.fg, Color::GREEN);
        assert_eq!(term.grid().get(6, 0).glyph, '0');
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
        assert_eq!(term.grid().get(0, 0).glyph, '\u{4e2d}');
        assert_eq!(term.grid().get(1, 0).glyph, '\0');
        assert_eq!(term.grid().get(2, 0).glyph, 'x');
    }
}
