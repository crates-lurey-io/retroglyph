//! The grid container.

use crate::cell::Cell;
use alloc::vec::Vec;
use core::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
/// Position in the grid.
pub struct Position {
    /// X coordinate.
    pub x: u16,
    /// Y coordinate.
    pub y: u16,
}

/// The main grid container for the terminal.
///
/// Note: This uses `alloc::vec::Vec`, requiring an allocator in `no_std` environments.
/// For strictly static, no-alloc environments, a static-sized grid type may be added in the future.
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
    pub const fn width(&self) -> usize { self.width }

    /// Returns the height of the grid.
    #[must_use]
    pub const fn height(&self) -> usize { self.height }

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
    #[must_use]
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
    #[must_use]
    pub fn checked_get(&self, x: usize, y: usize) -> Option<&Cell> {
        let index = self.get_index(x, y).ok()?;
        Some(&self.buffer[index])
    }

    /// Clears the grid to the default cell.
    pub fn clear(&mut self) {
        self.buffer.fill(Cell::default());
    }

    /// Yield positions where `self` differs from `other`.
    ///
    /// If dimensions differ, all cells in `self` are considered changed.
    pub fn diff<'a>(&'a self, other: &'a Self) -> impl Iterator<Item = (usize, usize, &'a Cell)> + 'a {
        let iter: DiffIterator<'a, _, _> = if self.width != other.width || self.height != other.height {
            DiffIterator::All(self.buffer.iter().enumerate(), core::marker::PhantomData)
        } else {
            DiffIterator::Changed(
                self.buffer.iter().enumerate().filter_map(move |(i, cell)| {
                    if cell == &other.buffer[i] {
                        None
                    } else {
                        Some((i, cell))
                    }
                }),
                core::marker::PhantomData,
            )
        };

        iter.map(move |(i, cell)| {
            let y = i / self.width;
            let x = i % self.width;
            (x, y, cell)
        })
    }

    const fn get_index(&self, x: usize, y: usize) -> Result<usize, ()> {
        if x < self.width && y < self.height {
            Ok(y * self.width + x)
        } else {
            Err(())
        }
    }
}

/// An iterator that encapsulates the different strategies for diffing grid content.
///
/// This is used to unify the types of the full-grid iterator and the selective
/// diff iterator, allowing `Grid::diff` to return a single type.
enum DiffIterator<'a, I1, I2> {
    /// Iterates over all cells.
    All(I1, core::marker::PhantomData<&'a ()>),
    /// Iterates only over changed cells.
    Changed(I2, core::marker::PhantomData<&'a ()>),
}

impl<'a, I1, I2> Iterator for DiffIterator<'a, I1, I2>
where
    I1: Iterator<Item = (usize, &'a Cell)>,
    I2: Iterator<Item = (usize, &'a Cell)>,
{
    type Item = (usize, &'a Cell);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            DiffIterator::All(iter, _) => iter.next(),
            DiffIterator::Changed(iter, _) => iter.next(),
        }
    }
}

impl fmt::Display for Grid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for y in 0..self.height {
            for x in 0..self.width {
                let cell = self.get(x, y);
                let c = if cell.glyph == ' ' { '·' } else { cell.glyph };
                write!(f, "{c}")?;
            }
            writeln!(f)?;
        }
        Ok(())
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
    #[should_panic(expected = "coordinates out of bounds")]
    fn test_grid_panic_put() {
        let mut grid = Grid::new(10, 10);
        grid.put(10, 0, Cell::default());
    }

    #[test]
    fn test_grid_diff() {
        let mut g1 = Grid::new(2, 2);
        let g2 = Grid::new(2, 2);
        
        g1.put(0, 0, Cell::default().with_glyph('A'));
        
        let diffs: Vec<_> = g1.diff(&g2).collect();
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0], (0, 0, g1.get(0, 0)));
    }

    #[test]
    fn test_grid_display() {
        let mut grid = Grid::new(3, 2);
        grid.put(0, 0, Cell::default().with_glyph('A'));
        
        let s = alloc::format!("{grid}");
        assert_eq!(s, "A··\n···\n");
    }
}
