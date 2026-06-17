//! Fundamental unit of the grid.

use crate::style::Style;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// A single cell in the terminal grid.
pub struct Cell {
    /// Character displayed in the cell.
    pub glyph: char,
    /// Style applied to this cell.
    pub style: Style,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            glyph: ' ',
            style: Style::default(),
        }
    }
}

impl Cell {
    /// Creates a new cell with the given glyph and style.
    #[must_use]
    pub fn new(glyph: char, style: Style) -> Self {
        Self { glyph, style }
    }

    /// Sets the glyph for this cell.
    #[must_use]
    pub fn with_glyph(mut self, glyph: char) -> Self {
        self.glyph = glyph;
        self
    }

    /// Sets the style for this cell.
    #[must_use]
    pub fn with_style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;

    #[test]
    fn test_cell_defaults() {
        let cell = Cell::default();
        assert_eq!(cell.glyph, ' ');
        assert_eq!(cell.style, Style::default());
    }

    #[test]
    fn test_cell_builder() {
        let style = Style::new().fg(Color::RED);
        let cell = Cell::new('A', style);
        
        assert_eq!(cell.glyph, 'A');
        assert_eq!(cell.style, style);
        
        let cell = cell.with_glyph('B');
        assert_eq!(cell.glyph, 'B');
    }
}
