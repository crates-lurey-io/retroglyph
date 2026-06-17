//! The grid container and backend abstraction.

use crate::cell::Cell;
use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Errors encountered during grid operations.
pub enum GridError {
    /// Attempted to access coordinates outside of the grid.
    OutOfBounds { 
        /// X coordinate.
        x: usize, 
        /// Y coordinate.
        y: usize 
    },
}

/// Abstract rendering backend for the grid.
pub trait Backend {
    /// Renders the current state of the grid.
    fn draw(&mut self, grid: &Grid);
}

/// The main grid container for the terminal.
pub struct Grid {
    width: usize,
    height: usize,
    buffer: Vec<Cell>,
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
    /// # Errors
    /// Returns `GridError::OutOfBounds` if the coordinates are invalid.
    pub fn put(&mut self, x: usize, y: usize, cell: Cell) -> Result<(), GridError> {
        let index = self.get_index(x, y)?;
        self.buffer[index] = cell;
        Ok(())
    }

    /// Gets the cell at the given coordinates.
    ///
    /// # Errors
    /// Returns `GridError::OutOfBounds` if the coordinates are invalid.
    pub fn get(&self, x: usize, y: usize) -> Result<&Cell, GridError> {
        let index = self.get_index(x, y)?;
        Ok(&self.buffer[index])
    }

    /// Clears the grid to the default cell.
    pub fn clear(&mut self) {
        self.buffer.fill(Cell::default());
    }

    fn get_index(&self, x: usize, y: usize) -> Result<usize, GridError> {
        if x < self.width && y < self.height {
            Ok(y * self.width + x)
        } else {
            Err(GridError::OutOfBounds { x, y })
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
        
        grid.put(5, 5, cell).unwrap();
        assert_eq!(grid.get(5, 5).unwrap().glyph, 'X');
    }

    #[test]
    fn test_grid_out_of_bounds() {
        let mut grid = Grid::new(10, 10);
        assert!(grid.get(10, 0).is_err());
        assert!(grid.put(0, 10, Cell::default()).is_err());
    }
}
