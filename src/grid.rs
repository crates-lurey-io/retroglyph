//! The grid container.

use crate::cell::Cell;

/// The main grid container for the terminal.
pub struct Grid {
    width: usize,
    height: usize,
    buffer: alloc::vec::Vec<Cell>,
}

impl Grid {
    /// Creates a new grid of the given dimensions.
    #[must_use]
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            buffer: alloc::vec![Cell::default(); width * height],
        }
    }

    /// Returns the width of the grid.
    #[must_use]
    pub fn width(&self) -> usize { self.width }

    /// Returns the height of the grid.
    #[must_use]
    pub fn height(&self) -> usize { self.height }

    /// Sets the cell at the given coordinates.
    ///
    /// # Panics
    ///
    /// Panics if the coordinates are out of bounds.
    pub fn put(&mut self, x: usize, y: usize, cell: Cell) {
        let index = self.get_index(x, y).expect("coordinates out of bounds");
        self.buffer[index] = cell;
    }

    /// Gets the cell at the given coordinates.
    ///
    /// # Panics
    ///
    /// Panics if the coordinates are out of bounds.
    pub fn get(&self, x: usize, y: usize) -> &Cell {
        let index = self.get_index(x, y).expect("coordinates out of bounds");
        &self.buffer[index]
    }

    /// Tries to set the cell at the given coordinates.
    ///
    /// Returns `None` if the coordinates are out of bounds.
    pub fn checked_put(&mut self, x: usize, y: usize, cell: Cell) -> Option<()> {
        let index = self.get_index(x, y).ok()?;
        self.buffer[index] = cell;
        Some(())
    }

    /// Tries to get the cell at the given coordinates.
    ///
    /// Returns `None` if the coordinates are out of bounds.
    pub fn checked_get(&self, x: usize, y: usize) -> Option<&Cell> {
        let index = self.get_index(x, y).ok()?;
        Some(&self.buffer[index])
    }

    /// Clears the grid to the default cell.
    pub fn clear(&mut self) {
        self.buffer.fill(Cell::default());
    }

    fn get_index(&self, x: usize, y: usize) -> Result<usize, ()> {
        if x < self.width && y < self.height {
            Ok(y * self.width + x)
        } else {
            Err(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grid_new() {
        let grid = Grid::new(80, 25);
        assert_eq!(grid.width(), 80);
        assert_eq!(grid.height(), 25);
    }

    #[test]
    fn test_grid_put_get() {
        let mut grid = Grid::new(10, 10);
        let cell = Cell::default().with_glyph('X');
        
        grid.put(5, 5, cell);
        assert_eq!(grid.get(5, 5).glyph, 'X');
    }

    #[test]
    fn test_grid_checked_put_get() {
        let mut grid = Grid::new(10, 10);
        let cell = Cell::default().with_glyph('Y');
        
        assert!(grid.checked_put(5, 5, cell).is_some());
        assert_eq!(grid.checked_get(5, 5).unwrap().glyph, 'Y');
        
        assert!(grid.checked_get(10, 0).is_none());
        assert!(grid.checked_put(0, 10, Cell::default()).is_none());
    }

    #[test]
    #[should_panic]
    fn test_grid_panic_put() {
        let mut grid = Grid::new(10, 10);
        grid.put(10, 0, Cell::default());
    }
}
