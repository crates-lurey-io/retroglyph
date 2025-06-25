/// Represents a single character in a grid based on [Codepage 437][] encoding.
///
/// [Codepage 437]: https://en.wikipedia.org/wiki/Code_page_437
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell(u8);

impl Cell {
    /// Default empty cell, represented by the space character (`0x20`).
    pub const EMPTY: Self = Cell(0x20);

    /// Creates a new `Cell` with the given CP437 glyph index.
    #[must_use]
    pub const fn new(glyph: u8) -> Self {
        Cell(glyph)
    }

    /// Returns the CP437 glyph index of this cell.
    #[must_use]
    pub const fn glyph(self) -> u8 {
        self.0
    }
}

impl Default for Cell {
    fn default() -> Self {
        Cell::EMPTY
    }
}

impl From<u8> for Cell {
    fn from(glyph: u8) -> Self {
        Cell::new(glyph)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new() {
        let cell = Cell::new(0x41);
        assert_eq!(cell.glyph(), 0x41);
    }

    #[test]
    fn default() {
        let cell = Cell::default();
        assert_eq!(cell.glyph(), 0x20);
    }

    #[test]
    fn from_u8() {
        let cell: Cell = 0x42.into();
        assert_eq!(cell.glyph(), 0x42);
    }
}
