//! Stateful terminal management and double-buffering.

use crate::backend::Backend;
use crate::grid::Grid;
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
