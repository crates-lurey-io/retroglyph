//! The grid container.

use crate::cell::Cell;
use alloc::vec::Vec;
use core::fmt;
use core::ops::{Index, IndexMut};

/// Size of the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct Size {
    /// Width.
    pub width: u16,
    /// Height.
    pub height: u16,
}

/// Position in the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Position {
    /// X coordinate.
    pub x: u16,
    /// Y coordinate.
    pub y: u16,
}

// Row-major ordering: y is the primary key.
impl PartialOrd for Position {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Position {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.y.cmp(&other.y).then(self.x.cmp(&other.x))
    }
}

impl From<(u16, u16)> for Position {
    fn from((x, y): (u16, u16)) -> Self {
        Self { x, y }
    }
}

impl From<Position> for (u16, u16) {
    fn from(p: Position) -> Self {
        (p.x, p.y)
    }
}

impl From<(u16, u16)> for Size {
    fn from((width, height): (u16, u16)) -> Self {
        Self { width, height }
    }
}

impl From<Size> for (u16, u16) {
    fn from(s: Size) -> Self {
        (s.width, s.height)
    }
}

/// Rectangle in the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rect {
    /// X coordinate.
    pub x: u16,
    /// Y coordinate.
    pub y: u16,
    /// Width.
    pub width: u16,
    /// Height.
    pub height: u16,
}

impl Rect {
    /// Returns `true` if `pos` is inside this rectangle.
    #[must_use]
    pub const fn contains(self, pos: Position) -> bool {
        pos.x >= self.x
            && pos.x < self.x + self.width
            && pos.y >= self.y
            && pos.y < self.y + self.height
    }

    /// Total number of cells in this rectangle.
    #[must_use]
    pub const fn area(self) -> u32 {
        self.width as u32 * self.height as u32
    }

    /// Top-left corner as a [`Position`].
    #[must_use]
    pub const fn top_left(self) -> Position {
        Position {
            x: self.x,
            y: self.y,
        }
    }

    /// Exclusive bottom-right corner as a [`Position`].
    #[must_use]
    pub const fn bottom_right(self) -> Position {
        Position {
            x: self.x + self.width,
            y: self.y + self.height,
        }
    }

    /// Returns `true` if this rectangle overlaps with `other`.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.x < other.x + other.width
            && self.x + self.width > other.x
            && self.y < other.y + other.height
            && self.y + self.height > other.y
    }

    /// Iterates every [`Position`] inside this rectangle in row-major order.
    pub fn positions(self) -> impl Iterator<Item = Position> {
        (self.y..self.y + self.height)
            .flat_map(move |y| (self.x..self.x + self.width).map(move |x| Position { x, y }))
    }
}

/// Iterator over all cells with their `(x, y)` coordinates.
pub struct Cells<'a> {
    iter: core::iter::Enumerate<core::slice::Iter<'a, Cell>>,
    width: usize,
}

impl<'a> Iterator for Cells<'a> {
    type Item = (u16, u16, &'a Cell);

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|(i, cell)| {
            #[allow(clippy::cast_possible_truncation)]
            let x = (i % self.width) as u16;
            #[allow(clippy::cast_possible_truncation)]
            let y = (i / self.width) as u16;
            (x, y, cell)
        })
    }
}

/// Mutable iterator over all cells with their `(x, y)` coordinates.
pub struct CellsMut<'a> {
    iter: core::iter::Enumerate<core::slice::IterMut<'a, Cell>>,
    width: usize,
}

impl<'a> Iterator for CellsMut<'a> {
    type Item = (u16, u16, &'a mut Cell);

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|(i, cell)| {
            #[allow(clippy::cast_possible_truncation)]
            let x = (i % self.width) as u16;
            #[allow(clippy::cast_possible_truncation)]
            let y = (i / self.width) as u16;
            (x, y, cell)
        })
    }
}

/// The main grid container for the terminal.
///
/// Note: This uses `alloc::vec::Vec`, requiring an allocator in `no_std` environments.
/// For strictly static, no-alloc environments, a static-sized grid type may be added in the future.
#[derive(Debug)]
pub struct Grid {
    width: u16,
    height: u16,
    buffer: Vec<Cell>,
}

impl Grid {
    /// Creates a new grid of the given dimensions.
    #[must_use]
    pub fn new(width: u16, height: u16) -> Self {
        let capacity = usize::from(width) * usize::from(height);
        Self {
            width,
            height,
            buffer: alloc::vec![Cell::default(); capacity],
        }
    }

    /// Returns the width of the grid.
    #[must_use]
    pub const fn width(&self) -> u16 {
        self.width
    }

    /// Returns the height of the grid.
    #[must_use]
    pub const fn height(&self) -> u16 {
        self.height
    }

    /// Sets the cell at the given coordinates.
    ///
    /// # Panics
    ///
    /// Panics if the coordinates are out of bounds.
    pub fn put(&mut self, x: u16, y: u16, cell: Cell) {
        let index = self.get_index(x, y).expect("coordinates out of bounds");
        self.buffer[index] = cell;
    }

    /// Gets the cell at the given coordinates.
    ///
    /// # Panics
    ///
    /// Panics if the coordinates are out of bounds.
    #[must_use]
    pub fn get(&self, x: u16, y: u16) -> &Cell {
        let index = self.get_index(x, y).expect("coordinates out of bounds");
        &self.buffer[index]
    }

    /// Tries to set the cell at the given coordinates.
    ///
    /// Returns `None` if the coordinates are out of bounds.
    pub fn checked_put(&mut self, x: u16, y: u16, cell: Cell) -> Option<()> {
        let index = self.get_index(x, y)?;
        self.buffer[index] = cell;
        Some(())
    }

    /// Tries to get the cell at the given coordinates.
    ///
    /// Returns `None` if the coordinates are out of bounds.
    #[must_use]
    pub fn checked_get(&self, x: u16, y: u16) -> Option<&Cell> {
        let index = self.get_index(x, y)?;
        Some(&self.buffer[index])
    }

    /// Tries to get a mutable reference to the cell at the given coordinates.
    ///
    /// Returns `None` if the coordinates are out of bounds.
    pub fn checked_get_mut(&mut self, x: u16, y: u16) -> Option<&mut Cell> {
        let index = self.get_index(x, y)?;
        Some(&mut self.buffer[index])
    }

    /// Iterates all cells with their `(x, y)` coordinates.
    #[must_use]
    pub fn cells(&self) -> Cells<'_> {
        Cells {
            iter: self.buffer.iter().enumerate(),
            width: usize::from(self.width),
        }
    }

    /// Iterates all cells mutably with their `(x, y)` coordinates.
    pub fn cells_mut(&mut self) -> CellsMut<'_> {
        CellsMut {
            iter: self.buffer.iter_mut().enumerate(),
            width: usize::from(self.width),
        }
    }

    /// Clears the grid to the default cell.
    pub fn clear(&mut self) {
        self.buffer.fill(Cell::default());
    }

    /// Resize the grid to `width` × `height` cells.
    ///
    /// Content within the overlapping region is preserved. New cells are
    /// initialised to the default cell. Shrinking discards cells outside the
    /// new bounds.
    pub fn resize(&mut self, width: u16, height: u16) {
        let w = usize::from(width);
        let h = usize::from(height);
        let mut new_buffer = alloc::vec![Cell::default(); w * h];
        let copy_width = usize::from(self.width).min(w);
        let copy_height = usize::from(self.height).min(h);
        let old_w = usize::from(self.width);
        for y in 0..copy_height {
            for x in 0..copy_width {
                new_buffer[y * w + x] = self.buffer[y * old_w + x];
            }
        }
        self.width = width;
        self.height = height;
        self.buffer = new_buffer;
    }

    /// Yield positions where `self` differs from `other`.
    ///
    /// If dimensions differ, all cells in `self` are considered changed.
    pub fn diff<'a>(
        &'a self,
        other: &'a Self,
    ) -> impl Iterator<Item = (Position, &'a Cell)> + 'a {
        let w = usize::from(self.width);
        let iter: DiffIterator<'a, _, _> =
            if self.width != other.width || self.height != other.height {
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
            #[allow(clippy::cast_possible_truncation)]
            let x = (i % w) as u16;
            #[allow(clippy::cast_possible_truncation)]
            let y = (i / w) as u16;
            (Position { x, y }, cell)
        })
    }

    fn get_index(&self, x: u16, y: u16) -> Option<usize> {
        if x < self.width && y < self.height {
            Some(usize::from(y) * usize::from(self.width) + usize::from(x))
        } else {
            None
        }
    }
}

impl Index<Position> for Grid {
    type Output = Cell;

    fn index(&self, pos: Position) -> &Cell {
        self.get(pos.x, pos.y)
    }
}

impl IndexMut<Position> for Grid {
    fn index_mut(&mut self, pos: Position) -> &mut Cell {
        let idx = self
            .get_index(pos.x, pos.y)
            .expect("position out of bounds");
        &mut self.buffer[idx]
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
                let c = match cell.glyph {
                    '\0' => ' ', // second column of a wide char
                    ' ' => '·',  // empty cell
                    c => c,
                };
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
        assert_eq!(
            diffs[0],
            (Position { x: 0, y: 0 }, g1.get(0, 0))
        );
    }

    #[test]
    fn test_grid_resize_expand() {
        let mut grid = Grid::new(3, 3);
        grid.put(1, 1, Cell::default().with_glyph('X'));
        grid.resize(6, 6);
        assert_eq!(grid.width(), 6);
        assert_eq!(grid.height(), 6);
        assert_eq!(grid.get(1, 1).glyph, 'X'); // preserved
        assert_eq!(grid.get(5, 5).glyph, ' '); // new cells default
    }

    #[test]
    fn test_grid_resize_shrink() {
        let mut grid = Grid::new(10, 10);
        grid.put(1, 1, Cell::default().with_glyph('A'));
        grid.resize(5, 5);
        assert_eq!(grid.width(), 5);
        assert_eq!(grid.height(), 5);
        assert_eq!(grid.get(1, 1).glyph, 'A'); // still in bounds, preserved
    }

    #[test]
    fn test_grid_resize_preserves_overlap() {
        let mut grid = Grid::new(4, 4);
        grid.put(0, 0, Cell::default().with_glyph('@'));
        grid.put(3, 3, Cell::default().with_glyph('X'));
        grid.resize(3, 3); // shrink: (3,3) falls outside
        assert_eq!(grid.get(0, 0).glyph, '@');
        assert_eq!(grid.get(2, 2).glyph, ' '); // was default, still default
    }

    #[test]
    fn test_grid_display() {
        let mut grid = Grid::new(3, 2);
        grid.put(0, 0, Cell::default().with_glyph('A'));

        let s = alloc::format!("{grid}");
        assert_eq!(s, "A··\n···\n");
    }

    // --- items 2, 3, 4, 5, 11 ---

    #[test]
    fn test_grid_cells_count() {
        let grid = Grid::new(4, 3);
        assert_eq!(grid.cells().count(), 12);
    }

    #[test]
    fn test_grid_cells_coordinates() {
        let grid = Grid::new(3, 2);
        let coords: Vec<(u16, u16)> = grid.cells().map(|(x, y, _)| (x, y)).collect();
        assert_eq!(
            coords,
            vec![
                (0, 0), (1, 0), (2, 0),
                (0, 1), (1, 1), (2, 1),
            ]
        );
    }

    #[test]
    fn test_grid_cells_mut() {
        let mut grid = Grid::new(2, 2);
        for (x, y, cell) in grid.cells_mut() {
            #[allow(clippy::cast_possible_truncation)]
            let idx = (y * 2 + x) as u8;
            cell.glyph = char::from(b'A' + idx);
        }
        assert_eq!(grid.get(0, 0).glyph, 'A');
        assert_eq!(grid.get(1, 0).glyph, 'B');
        assert_eq!(grid.get(0, 1).glyph, 'C');
        assert_eq!(grid.get(1, 1).glyph, 'D');
    }

    #[test]
    fn test_rect_contains() {
        let r = Rect { x: 2, y: 3, width: 4, height: 5 };
        assert!(r.contains(Position { x: 2, y: 3 }));
        assert!(r.contains(Position { x: 5, y: 7 }));
        assert!(!r.contains(Position { x: 6, y: 3 })); // x == x+width, exclusive
        assert!(!r.contains(Position { x: 2, y: 8 })); // y == y+height, exclusive
        assert!(!r.contains(Position { x: 1, y: 3 }));
    }

    #[test]
    fn test_rect_area() {
        assert_eq!(Rect { x: 0, y: 0, width: 5, height: 3 }.area(), 15);
        assert_eq!(Rect::default().area(), 0);
    }

    #[test]
    fn test_rect_top_left_bottom_right() {
        let r = Rect { x: 1, y: 2, width: 3, height: 4 };
        assert_eq!(r.top_left(), Position { x: 1, y: 2 });
        assert_eq!(r.bottom_right(), Position { x: 4, y: 6 });
    }

    #[test]
    fn test_rect_intersects() {
        let a = Rect { x: 0, y: 0, width: 4, height: 4 };
        let b = Rect { x: 2, y: 2, width: 4, height: 4 };
        let c = Rect { x: 4, y: 0, width: 4, height: 4 }; // touches edge, no overlap
        assert!(a.intersects(b));
        assert!(!a.intersects(c));
    }

    #[test]
    fn test_rect_positions() {
        let r = Rect { x: 1, y: 2, width: 2, height: 2 };
        let pts: Vec<Position> = r.positions().collect();
        assert_eq!(
            pts,
            vec![
                Position { x: 1, y: 2 },
                Position { x: 2, y: 2 },
                Position { x: 1, y: 3 },
                Position { x: 2, y: 3 },
            ]
        );
    }

    #[test]
    fn test_index_position() {
        let mut grid = Grid::new(5, 5);
        let pos = Position { x: 2, y: 3 };
        grid[pos] = Cell::default().with_glyph('Z');
        assert_eq!(grid[pos].glyph, 'Z');
    }

    #[test]
    fn test_position_from_tuple() {
        let p: Position = (3u16, 7u16).into();
        assert_eq!(p, Position { x: 3, y: 7 });
        let t: (u16, u16) = p.into();
        assert_eq!(t, (3, 7));
    }

    #[test]
    fn test_size_from_tuple() {
        let s: Size = (80u16, 25u16).into();
        assert_eq!(s, Size { width: 80, height: 25 });
        let t: (u16, u16) = s.into();
        assert_eq!(t, (80, 25));
    }

    #[test]
    fn test_position_ord_row_major() {
        let mut positions = vec![
            Position { x: 5, y: 0 },
            Position { x: 0, y: 1 },
            Position { x: 3, y: 0 },
        ];
        positions.sort();
        assert_eq!(
            positions,
            vec![
                Position { x: 3, y: 0 },
                Position { x: 5, y: 0 },
                Position { x: 0, y: 1 },
            ]
        );
    }

    #[test]
    fn test_size_ord() {
        assert!(Size { width: 1, height: 2 } < Size { width: 2, height: 1 });
    }
}
