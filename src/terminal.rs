//! Stateful terminal management and double-buffering.

use crate::backend::Backend;
use crate::cell::Cell;
use crate::grid::{Grid, Rect};
use crate::style::{CellModifier, Style};
use crate::color::Color;

/// The main entry point for `rg`.
///
/// Generic over the backend. Owns a double-buffered grid and provides
/// a stateful drawing API.
pub struct Terminal<B: Backend> {
    current: Grid,
    previous: Grid,
    backend: B,
    drawing_style: Style,
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
        }
    }

    /// Sets the foreground color for the stateful API.
    pub fn fg(&mut self, color: Color) -> &mut Self {
        self.drawing_style.fg = color;
        self
    }

    /// Sets the background color for the stateful API.
    pub fn bg(&mut self, color: Color) -> &mut Self {
        self.drawing_style.bg = color;
        self
    }

    /// Sets text modifiers for the stateful API.
    pub fn modifier(&mut self, modifier: CellModifier) -> &mut Self {
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
    pub fn style(&self) -> Style {
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
    pub fn grid(&self) -> &Grid {
        &self.current
    }

    /// Returns a reference to the backend.
    #[must_use]
    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// Returns a mutable reference to the backend.
    pub fn backend_mut(&mut self) -> &mut B {
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
                if cur_x >= self.current.width() as u16 {
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
}
