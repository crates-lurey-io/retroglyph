//! `Grid`'s trait impls for layer 0: [`Index`]/[`IndexMut`] by [`Pos`](crate::grid::Pos), and its
//! [`Display`](fmt::Display)/[`Debug`](fmt::Debug) implementations.

use super::{Grid, Pos, to_grixy_pos};
#[cfg(test)]
use crate::color::Style;
use crate::tile::{Tile, TileFlags};
use core::fmt;
use core::ops::{Index, IndexMut};

impl Index<Pos> for Grid {
    type Output = Tile;

    /// Reads the tile on layer 0 at `pos`.
    ///
    /// # Panics
    ///
    /// Panics if `pos` is outside the grid's `0..width` x `0..height` bounds. This is the
    /// unchecked, layer-0-only counterpart to [`tile`](Self::tile), which instead returns `None`
    /// on either an out-of-bounds `pos` or an unallocated layer; reach for `tile` when `pos`
    /// isn't already known to be in bounds.
    fn index(&self, pos: Pos) -> &Tile {
        &self.layer0().buf[to_grixy_pos(pos)]
    }
}

impl IndexMut<Pos> for Grid {
    /// Mutably borrows the tile on layer 0 at `pos`.
    ///
    /// # Panics
    ///
    /// Panics if `pos` is outside the grid's `0..width` x `0..height` bounds, the same bound as
    /// [`Index`]'s `index`. Reach for [`tile_mut`](Self::tile_mut) when `pos` isn't already known
    /// to be in bounds; it returns `None` instead of panicking.
    fn index_mut(&mut self, pos: Pos) -> &mut Tile {
        let pos = to_grixy_pos(pos);
        &mut self.layer0_mut().buf[pos]
    }
}

// ---------------------------------------------------------------------------
// Display / Debug: layer 0
// ---------------------------------------------------------------------------

/// Renders layer 0 only, one character per cell, with `·` in place of a plain space.
impl fmt::Display for Grid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for y in 0..self.height() {
            for x in 0..self.width() {
                let tile = &self[Pos::new(x, y)];
                let is_spacer = tile.flags.contains(TileFlags::WIDE_CHAR_SPACER);
                let c = if is_spacer {
                    ' ' // right half of a wide char, don't print twice
                } else if tile.glyph == ' ' {
                    '·' // empty cell marker
                } else {
                    tile.glyph
                };
                write!(f, "{c}")?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

/// Shows `width`, `height`, `max_layer`, and `has_spans`; the layer buffers themselves are
/// omitted (see [`Display`](fmt::Display) for a rendering of layer 0).
impl fmt::Debug for Grid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Grid")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("max_layer", &self.max_layer)
            .field("has_spans", &self.has_spans)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn index_panics_out_of_bounds() {
        let grid = Grid::new(10, 10);
        let _ = &grid[Pos::new(0, 10)];
    }

    #[test]
    fn index_by_pos_reads_back_the_written_glyph() {
        let mut grid = Grid::new(5, 5);
        let pos = Pos::new(2, 3);
        grid[pos] = Tile::default().with_glyph('Z');
        assert_eq!(grid[pos].glyph(), 'Z');
    }

    #[test]
    fn display_renders_glyphs_row_major_with_a_middle_dot_for_empty_cells() {
        let mut grid = Grid::new(3, 2);
        grid.put_tile(0, (0, 0), Tile::default().with_glyph('A'));

        let s = alloc::format!("{grid}");
        assert_eq!(s, "A··\n···\n");
    }

    #[cfg(feature = "egc")]
    #[test]
    fn display_wide_char_spacer_renders_as_a_plain_space() {
        // A wide char's right-half spacer cell prints as a plain space, not the wide
        // char's own glyph repeated.
        let mut grid = Grid::new(3, 1);
        grid.write_grapheme(0, 0, 0, "\u{4e2d}", Style::default()); // wide (CJK)

        let s = alloc::format!("{grid}");
        assert_eq!(s, "\u{4e2d} \u{b7}\n");
    }

    #[test]
    fn debug_reports_layer_and_span_state() {
        let mut grid = Grid::new(3, 2);
        grid.put_tile(2, (0, 0), Tile::default().with_glyph('A'));
        grid.write_span(0, 0, 0, &["hi"], Style::default());

        let s = alloc::format!("{grid:?}");
        assert!(s.contains("width: 3"));
        assert!(s.contains("height: 2"));
        assert!(s.contains("max_layer: 2"));
        assert!(s.contains("has_spans: true"));
    }
}
