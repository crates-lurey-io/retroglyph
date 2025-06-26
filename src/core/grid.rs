use crate::core::Cell;

/// A fixed-size 2D grid of cells where the dimensions are known at compile-time.
#[derive(Debug, Clone)]
pub struct Grid<const LENGTH: usize> {
    cells: [Cell; LENGTH],
    width: usize,
}

impl<const LENGTH: usize> Grid<LENGTH> {
    /// Creates a new grid with the given width, deriving height from the `LENGTH` type parameter.
    ///
    /// See also: [`grid!`].
    ///
    /// # Panics
    ///
    /// Panics if `LENGTH` is not a multiple of `width`.
    #[must_use]
    pub const fn new(width: usize) -> Self {
        assert!(LENGTH % width == 0, "LENGTH must be a multiple of width");
        Self {
            cells: [Cell::EMPTY; LENGTH],
            width,
        }
    }

    /// Creates a new grid with the specified cells and width.
    ///
    /// # Panics
    ///
    /// Panics if the length of `cells` does not equal `LENGTH`.
    #[must_use]
    pub const fn with_cells(cells: [Cell; LENGTH], width: usize) -> Self {
        Self { cells, width }
    }

    /// Returns the width of the grid.
    #[must_use]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Returns the height of the grid.
    #[must_use]
    pub const fn height(&self) -> usize {
        LENGTH / self.width
    }

    /// Gets the `Cell` at the specified coordinates.
    ///
    /// Returns `None` if the coordinates are out of bounds.
    #[must_use]
    pub fn get(&self, x: usize, y: usize) -> Option<&Cell> {
        if x < self.width && y < self.height() {
            // SAFETY: Bounds are checked above, so this is safe.
            Some(unsafe { self.cells.get_unchecked(y * self.width + x) })
        } else {
            None
        }
    }

    /// Gets a mutable reference to the `Cell` at the specified coordinates.
    ///
    /// Returns `None` if the coordinates are out of bounds.
    pub fn get_mut(&mut self, x: usize, y: usize) -> Option<&mut Cell> {
        if x < self.width && y < self.height() {
            // SAFETY: Bounds are checked above, so this is safe.
            Some(unsafe { self.cells.get_unchecked_mut(y * self.width + x) })
        } else {
            None
        }
    }

    /// Returns an iterator over the cells in the grid in row-major order.
    pub fn iter(&self) -> impl Iterator<Item = &Cell> {
        self.cells.iter()
    }

    /// Returns each row of the grid as a slice.
    ///
    /// Each row is represented as a slice of `Cell` references.
    pub fn rows(&self) -> impl Iterator<Item = &[Cell]> {
        self.cells.chunks(self.width)
    }
}

/// Creates a new fixed-size grid with the specified width and height.
#[macro_export]
macro_rules! grid {
    ($width:expr, $height:expr) => {
        $crate::core::Grid::<{ $width * $height }>::new($width)
    };
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn new() {
        let grid: Grid<2000> = grid!(80, 25);
        assert_eq!(grid.width(), 80);
        assert_eq!(grid.height(), 25);
    }

    #[test]
    fn cells_are_empty_by_default() {
        let grid: Grid<2000> = grid!(80, 25);
        assert!(grid.iter().all(|cell| cell.glyph() == Cell::EMPTY.glyph()));
    }

    #[test]
    fn get_cells() {
        let grid: Grid<2000> = grid!(80, 25);
        assert!(grid.get(0, 0).is_some());
        assert!(grid.get(79, 24).is_some());
        assert!(grid.get(80, 25).is_none()); // Out of bounds
        assert!(grid.get(100, 100).is_none()); // Out of bounds
    }

    #[test]
    fn get_mut_cells() {
        let mut grid: Grid<2000> = grid!(80, 25);
        if let Some(cell) = grid.get_mut(0, 0) {
            *cell = Cell::new(0x41); // Set to 'A'
        }
        assert_eq!(grid.get(0, 0).unwrap().glyph(), 0x41);
        assert!(grid.get_mut(80, 25).is_none()); // Out of bounds
    }

    #[test]
    fn iter() {
        #[rustfmt::skip]
        let grid = Grid::<6>::with_cells([
            Cell::new(0x41), Cell::new(0x42), Cell::new(0x43),
            Cell::new(0x44), Cell::new(0x45), Cell::new(0x46),
        ], 3);

        let cells: Vec<_> = grid.iter().collect();
        assert_eq!(cells.len(), 6);
    }

    #[test]
    fn rows() {
        #[rustfmt::skip]
        let grid = Grid::<6>::with_cells([
            Cell::new(0x41), Cell::new(0x42), Cell::new(0x43),
            Cell::new(0x44), Cell::new(0x45), Cell::new(0x46),
        ], 3);

        let rows: Vec<_> = grid.rows().collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0],
            &[Cell::new(0x41), Cell::new(0x42), Cell::new(0x43)]
        );
        assert_eq!(
            rows[1],
            &[Cell::new(0x44), Cell::new(0x45), Cell::new(0x46)]
        );
    }
}
