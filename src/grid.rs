//! The grid container.

use crate::cell::Cell;
#[cfg(feature = "egc")]
use crate::cell::CellFlags;
#[cfg(feature = "egc")]
use crate::cell::cap_grapheme;
#[cfg(feature = "egc")]
use crate::style::Style;
use alloc::vec::Vec;
use core::fmt;
use core::ops::{Index, IndexMut};
use grixy::buf::GridBuf;
use grixy::ops::layout::RowMajor;
use grixy::ops::{ExactSizeGrid, GridDiff, GridRead, GridWrite};

/// Size of the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct Size {
    /// Width.
    pub width: u16,
    /// Height.
    pub height: u16,
}

/// Pos in the grid, in (x = column, y = row) order.
///
/// Implements [`Ord`] in row-major order (y primary, then x), which is the
/// natural ordering for terminal rendering: top-to-bottom, left-to-right within
/// each row.
pub type Pos = ixy::Pos<u16>;

/// Rectangle in the grid.
pub type Rect = ixy::Rect<u16>;

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

// ---------------------------------------------------------------------------
// Helpers: coordinate conversion between u16 and usize
// ---------------------------------------------------------------------------

fn to_grixy_pos(pos: Pos) -> grixy::core::Pos {
    grixy::core::Pos::new(usize::from(pos.x), usize::from(pos.y))
}

#[allow(clippy::missing_const_for_fn)]
fn from_grixy_pos(pos: grixy::core::Pos) -> Pos {
    #[allow(clippy::cast_possible_truncation)]
    Pos::new(pos.x as u16, pos.y as u16)
}

// ---------------------------------------------------------------------------
// Grid iterators
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Grid
// ---------------------------------------------------------------------------

/// The main grid container for the terminal.
///
/// Note: This uses `alloc::vec::Vec`, requiring an allocator in `no_std` environments.
/// For strictly static, no-alloc environments, a static-sized grid type may be added in the future.
pub struct Grid {
    buf: GridBuf<Cell, Vec<Cell>, RowMajor>,
}

impl Grid {
    /// Creates a new grid of the given dimensions.
    #[must_use]
    pub fn new(width: u16, height: u16) -> Self {
        let capacity = usize::from(width) * usize::from(height);
        Self {
            buf: GridBuf::from_buffer(alloc::vec![Cell::default(); capacity], usize::from(width)),
        }
    }

    /// Returns the width of the grid.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn width(&self) -> u16 {
        self.buf.width() as u16
    }

    /// Returns the height of the grid.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn height(&self) -> u16 {
        self.buf.height() as u16
    }

    /// Sets the cell at the given coordinates.
    ///
    /// # Panics
    ///
    /// Panics if the coordinates are out of bounds.
    pub fn put(&mut self, x: u16, y: u16, cell: Cell) {
        let pos = to_grixy_pos(Pos::new(x, y));
        assert!(
            self.buf.contains(pos),
            "coordinates out of bounds: ({x}, {y})"
        );
        self.buf[pos] = cell;
    }

    /// Gets the cell at the given coordinates.
    ///
    /// # Panics
    ///
    /// Panics if the coordinates are out of bounds.
    #[must_use]
    pub fn get(&self, x: u16, y: u16) -> &Cell {
        let pos = to_grixy_pos(Pos::new(x, y));
        &self.buf[pos]
    }

    /// Tries to set the cell at the given coordinates.
    ///
    /// Returns `None` if the coordinates are out of bounds.
    pub fn checked_put(&mut self, x: u16, y: u16, cell: Cell) -> Option<()> {
        let pos = to_grixy_pos(Pos::new(x, y));
        if self.buf.contains(pos) {
            self.buf[pos] = cell;
            Some(())
        } else {
            None
        }
    }

    /// Tries to get the cell at the given coordinates.
    ///
    /// Returns `None` if the coordinates are out of bounds.
    #[must_use]
    pub fn checked_get(&self, x: u16, y: u16) -> Option<&Cell> {
        let pos = to_grixy_pos(Pos::new(x, y));
        self.buf.get(pos)
    }

    /// Tries to get a mutable reference to the cell at the given coordinates.
    ///
    /// Returns `None` if the coordinates are out of bounds.
    pub fn checked_get_mut(&mut self, x: u16, y: u16) -> Option<&mut Cell> {
        let pos = to_grixy_pos(Pos::new(x, y));
        self.buf.get_mut(pos)
    }

    /// Iterates all cells with their `(x, y)` coordinates.
    #[must_use]
    pub fn cells(&self) -> Cells<'_> {
        Cells {
            iter: self.buf.as_ref().iter().enumerate(),
            width: self.buf.width(),
        }
    }

    /// Iterates all cells mutably with their `(x, y)` coordinates.
    pub fn cells_mut(&mut self) -> CellsMut<'_> {
        let width = self.buf.width();
        CellsMut {
            iter: self.buf.as_mut().iter_mut().enumerate(),
            width,
        }
    }

    /// Clears the grid to the default cell.
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    /// Resize the grid to `width` × `height` cells.
    ///
    /// Content within the overlapping region is preserved. New cells are
    /// initialised to the default cell. Shrinking discards cells outside the
    /// new bounds.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.buf.resize(usize::from(width), usize::from(height));
    }

    /// Yield positions where `self` differs from `other`.
    ///
    /// If dimensions differ, all cells in `self` are considered changed.
    pub fn diff<'a>(&'a self, other: &'a Self) -> impl Iterator<Item = (Pos, &'a Cell)> + 'a {
        self.buf
            .diff(&other.buf)
            .map(|(pos, cell)| (from_grixy_pos(pos), cell))
    }

    /// Write a grapheme cluster at `(x, y)`, enforcing wide-character invariants.
    ///
    /// This is the canonical way to place content into the grid when the `egc`
    /// feature is enabled. It:
    ///
    /// - Clears any wide character whose primary or spacer cell would be
    ///   overwritten.
    /// - Sets [`CellFlags::WIDE_CHAR`] on the primary cell and places a
    ///   [`CellFlags::WIDE_CHAR_SPACER`] in the adjacent cell for 2-column
    ///   characters.
    /// - Stores multi-codepoint EGCs (combining marks, ZWJ sequences) in
    ///   `Cell::extra`, capped at 8 codepoints total.
    ///
    /// Does nothing if `(x, y)` is out of bounds, if the grapheme has zero
    /// display width, or if a 2-column wide character would overflow the grid
    /// (the last column needs both its own cell and a spacer).
    ///
    /// # Panics
    ///
    /// Panics if the grapheme's display width exceeds [`u16::MAX`]. In
    /// practice this cannot happen: the maximum Unicode grapheme width is 2.
    ///
    /// Only present when the `egc` feature is enabled.
    #[cfg(feature = "egc")]
    pub fn write_grapheme(&mut self, x: u16, y: u16, grapheme: &str, style: Style) {
        use unicode_width::UnicodeWidthStr;

        let width = u16::try_from(grapheme.width()).expect("grapheme width exceeds u16");
        if width == 0 {
            return;
        }

        let w = self.buf.width();
        let idx = usize::from(y) * w + usize::from(x);
        if idx >= self.buf.as_ref().len() {
            return;
        }

        // A 2-column char needs a spacer at x+1. If that's out of bounds,
        // silently refuse rather than leaving an orphaned primary cell.
        if width == 2 && x.saturating_add(1) as usize >= w {
            return;
        }

        // Clear any wide-char cell that would be partially overwritten.
        self.clear_overlap(x, y, width);

        // Build cell content.
        let mut chars = grapheme.chars();
        let first = chars.next().unwrap_or(' ');
        let has_extra = chars.next().is_some();
        let extra = if has_extra {
            Some(alloc::sync::Arc::new(cap_grapheme(grapheme)))
        } else {
            None
        };
        let flags = if width == 2 {
            CellFlags::WIDE_CHAR
        } else {
            CellFlags::empty()
        };

        self.buf.as_mut()[idx].glyph = first;
        self.buf.as_mut()[idx].style = style;
        self.buf.as_mut()[idx].extra = extra;
        self.buf.as_mut()[idx].flags = flags;

        // Place spacer for wide characters.
        if width == 2 {
            let spacer_idx = usize::from(y) * w + usize::from(x + 1);
            if spacer_idx < self.buf.as_ref().len() {
                let spacer = &mut self.buf.as_mut()[spacer_idx];
                spacer.glyph = ' ';
                spacer.style = style;
                spacer.extra = None;
                spacer.flags = CellFlags::WIDE_CHAR_SPACER;
            }
        }
    }

    /// Clears wide-character cells that would be partially overwritten by a
    /// write starting at `(x, y)` spanning `width` columns.
    ///
    /// Iterates each column from `x` to `x + width - 1` (inclusive, clamped to
    /// the grid width). For each column:
    ///
    /// - If the cell is a [`WIDE_CHAR_SPACER`](CellFlags::WIDE_CHAR_SPACER),
    ///   the primary cell to its left is reset (orphan prevention).
    /// - If the cell is a [`WIDE_CHAR`](CellFlags::WIDE_CHAR) primary, its
    ///   spacer cell to the right is reset (its right half gets overwritten).
    ///
    /// This includes the cell at `x` itself (the one we're about to write),
    /// ensuring the old spacer is cleared before we place ours.
    #[cfg(feature = "egc")]
    fn clear_overlap(&mut self, x: u16, y: u16, width: u16) {
        let w = self.buf.width();
        for cx in x..x.saturating_add(width) {
            let idx = usize::from(y) * w + usize::from(cx);
            if idx >= self.buf.as_ref().len() {
                continue;
            }
            let flags = self.buf.as_ref()[idx].flags;

            // Overwriting a spacer — clear the primary cell to its left.
            if flags.contains(CellFlags::WIDE_CHAR_SPACER) && cx > 0 {
                let pidx = usize::from(y) * w + usize::from(cx - 1);
                if pidx < self.buf.as_ref().len() {
                    self.buf.as_mut()[pidx].reset();
                }
            }

            // Overwriting a primary wide cell — clear the spacer to its right.
            if flags.contains(CellFlags::WIDE_CHAR) {
                let sidx = usize::from(y) * w + usize::from(cx + 1);
                if sidx < self.buf.as_ref().len() {
                    self.buf.as_mut()[sidx].reset();
                }
            }
        }
    }
}

impl Index<Pos> for Grid {
    type Output = Cell;

    fn index(&self, pos: Pos) -> &Cell {
        &self.buf[to_grixy_pos(pos)]
    }
}

impl IndexMut<Pos> for Grid {
    fn index_mut(&mut self, pos: Pos) -> &mut Cell {
        let pos = to_grixy_pos(pos);
        &mut self.buf[pos]
    }
}

impl fmt::Display for Grid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for y in 0..self.height() {
            for x in 0..self.width() {
                let cell = self.get(x, y);
                #[cfg(feature = "egc")]
                let is_spacer = cell.flags.contains(CellFlags::WIDE_CHAR_SPACER);
                #[cfg(not(feature = "egc"))]
                let is_spacer = cell.glyph == '\0';
                let c = if is_spacer {
                    ' ' // right half of a wide char — don't print twice
                } else if cell.glyph == ' ' {
                    '·' // empty cell marker
                } else {
                    cell.glyph
                };
                write!(f, "{c}")?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

impl fmt::Debug for Grid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Grid")
            .field("width", &self.buf.width())
            .field("height", &self.buf.height())
            .finish_non_exhaustive()
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
        assert_eq!(grid.get(5, 5).glyph(), 'X');
    }

    #[test]
    fn test_grid_checked_put_get() {
        let mut grid = Grid::new(10, 10);
        let cell = Cell::default().with_glyph('Y');

        assert!(grid.checked_put(5, 5, cell).is_some());
        assert_eq!(grid.checked_get(5, 5).unwrap().glyph(), 'Y');

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
        assert_eq!(diffs[0], (Pos::new(0, 0), g1.get(0, 0)));
    }

    #[test]
    fn test_grid_resize_expand() {
        let mut grid = Grid::new(3, 3);
        grid.put(1, 1, Cell::default().with_glyph('X'));
        grid.resize(6, 6);
        assert_eq!(grid.width(), 6);
        assert_eq!(grid.height(), 6);
        assert_eq!(grid.get(1, 1).glyph(), 'X'); // preserved
        assert_eq!(grid.get(5, 5).glyph(), ' '); // new cells default
    }

    #[test]
    fn test_grid_resize_shrink() {
        let mut grid = Grid::new(10, 10);
        grid.put(1, 1, Cell::default().with_glyph('A'));
        grid.resize(5, 5);
        assert_eq!(grid.width(), 5);
        assert_eq!(grid.height(), 5);
        assert_eq!(grid.get(1, 1).glyph(), 'A'); // still in bounds, preserved
    }

    #[test]
    fn test_grid_resize_preserves_overlap() {
        let mut grid = Grid::new(4, 4);
        grid.put(0, 0, Cell::default().with_glyph('@'));
        grid.put(3, 3, Cell::default().with_glyph('X'));
        grid.resize(3, 3); // shrink: (3,3) falls outside
        assert_eq!(grid.get(0, 0).glyph(), '@');
        assert_eq!(grid.get(2, 2).glyph(), ' '); // was default, still default
    }

    #[test]
    fn test_grid_display() {
        let mut grid = Grid::new(3, 2);
        grid.put(0, 0, Cell::default().with_glyph('A'));

        let s = alloc::format!("{grid}");
        assert_eq!(s, "A··\n···\n");
    }

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
            vec![(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1),]
        );
    }

    #[test]
    fn test_grid_cells_mut() {
        use crate::style::Style;
        let mut grid = Grid::new(2, 2);
        for (x, y, cell) in grid.cells_mut() {
            #[allow(clippy::cast_possible_truncation)]
            let idx = (y * 2 + x) as u8;
            *cell = Cell::new(char::from(b'A' + idx), Style::default());
        }
        assert_eq!(grid.get(0, 0).glyph(), 'A');
        assert_eq!(grid.get(1, 0).glyph(), 'B');
        assert_eq!(grid.get(0, 1).glyph(), 'C');
        assert_eq!(grid.get(1, 1).glyph(), 'D');
    }

    #[test]
    fn test_rect_contains() {
        let r = Rect::new(2, 3, 4, 5);
        assert!(r.contains_pos(Pos::new(2, 3)));
        assert!(r.contains_pos(Pos::new(5, 7)));
        assert!(!r.contains_pos(Pos::new(6, 3))); // x == x+width, exclusive
        assert!(!r.contains_pos(Pos::new(2, 8))); // y == y+height, exclusive
        assert!(!r.contains_pos(Pos::new(1, 3)));
    }

    #[test]
    fn test_rect_area() {
        assert_eq!(Rect::new(0, 0, 5, 3).area(), 15);
        assert_eq!(Rect::default().area(), 0);
    }

    #[test]
    fn test_rect_top_left_bottom_right() {
        let r = Rect::new(1, 2, 3, 4);
        assert_eq!(r.top_left(), Pos::new(1, 2));
        assert_eq!(r.bottom_right(), Pos::new(4, 6));
    }

    #[test]
    fn test_rect_intersects() {
        let a = Rect::new(0, 0, 4, 4);
        let b = Rect::new(2, 2, 4, 4);
        let c = Rect::new(4, 0, 4, 4); // touches edge, no overlap
        assert!(!a.intersect(b).is_empty());
        assert!(a.intersect(c).is_empty());
    }

    #[test]
    fn test_rect_positions() {
        let r = Rect::new(1, 2, 2, 2);
        let pts: Vec<Pos> = r.pos_iter().collect();
        assert_eq!(
            pts,
            vec![
                Pos::new(1, 2),
                Pos::new(2, 2),
                Pos::new(1, 3),
                Pos::new(2, 3),
            ]
        );
    }

    #[test]
    fn test_index_position() {
        let mut grid = Grid::new(5, 5);
        let pos = Pos::new(2, 3);
        grid[pos] = Cell::default().with_glyph('Z');
        assert_eq!(grid[pos].glyph(), 'Z');
    }

    #[test]
    fn test_position_from_tuple() {
        let p: Pos = (3u16, 7u16).into();
        assert_eq!(p, Pos::new(3, 7));
        let t: (u16, u16) = p.into();
        assert_eq!(t, (3, 7));
    }

    #[test]
    fn test_size_from_tuple() {
        let s: Size = (80u16, 25u16).into();
        assert_eq!(
            s,
            Size {
                width: 80,
                height: 25
            }
        );
        let t: (u16, u16) = s.into();
        assert_eq!(t, (80, 25));
    }

    #[test]
    fn test_position_ord_row_major() {
        let mut positions = vec![Pos::new(5, 0), Pos::new(0, 1), Pos::new(3, 0)];
        positions.sort();
        assert_eq!(
            positions,
            vec![Pos::new(3, 0), Pos::new(5, 0), Pos::new(0, 1),]
        );
    }

    #[test]
    fn test_size_ord() {
        assert!(
            Size {
                width: 1,
                height: 2
            } < Size {
                width: 2,
                height: 1
            }
        );
    }
}
