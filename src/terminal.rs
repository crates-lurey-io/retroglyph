//! Stateful terminal management and double-buffering.

use crate::backend::Backend;
use crate::cell::Cell;
use crate::grid::{Grid, Rect};
use crate::style::{CellModifier, Style};
use crate::color::Color;
use crate::event::Event;
use core::time::Duration;

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
        let current = Grid::new(size.width as usize, size.height as usize);
        let previous = Grid::new(size.width as usize, size.height as usize);
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

    /// Place a character at (x, y) with the current style.
    pub fn put(&mut self, x: u16, y: u16, ch: char) {
        if let Some(cell) = self.current.checked_get_mut(x as usize, y as usize) {
            cell.glyph = ch;
            cell.style = self.drawing_style;
        }
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
                if let Some(cell) = self.current.checked_get_mut(x as usize, y as usize) {
                    *cell = Cell::default();
                }
            }
        }
    }

    /// Place a character with an explicit style, ignoring current state.
    pub fn put_styled(&mut self, x: u16, y: u16, ch: char, style: Style) {
        if let Some(cell) = self.current.checked_get_mut(x as usize, y as usize) {
            cell.glyph = ch;
            cell.style = style;
        }
    }

    /// Print a string starting at (x, y) with the current style.
    /// Characters beyond grid width are clipped. `\n` advances to the
    /// next row at the original x.
    pub fn print(&mut self, x: u16, y: u16, text: &str) {
        let mut cur_x = x;
        let mut cur_y = y;
        for c in text.chars() {
            if c == '\n' {
                cur_x = x;
                cur_y += 1;
            } else {
                self.put(cur_x, cur_y, c);
                cur_x += 1;
                // Simple clipping
                if usize::from(cur_x) >= self.current.width() {
                    cur_x = x;
                    cur_y += 1;
                }
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
    /// If an event was previously buffered by `has_input`, it is returned immediately.
    /// Otherwise, the backend is polled for a new event.
    pub fn poll(&mut self, timeout: Duration) -> Option<Event> {
        if let Some(event) = self.queued_event.take() {
            Some(event)
        } else {
            self.backend.poll_event(timeout)
        }
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

        terminal.grid_mut().put(0, 0, Cell {
            glyph: 'X',
            style: Style::default(),
        });

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
}
