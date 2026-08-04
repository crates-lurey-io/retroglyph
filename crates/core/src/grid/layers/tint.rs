//! Per-cell tint and grapheme-extras storage: [`Grid::tint`] and [`Grid::set_tint`], plus the
//! shared side-table primitive [`Grid::set_extra`] both write through.

use super::super::{Grid, Pos, TileExtra, to_grixy_pos};
#[cfg(test)]
use crate::color::Style;
use crate::color::Tint;
#[cfg(test)]
use crate::tile::Tile;
use crate::tile::TileFlags;
use grixy::ops::GridRead;

impl Grid {
    /// Sets the whole side-table entry for an already-written tile at `(x, y)` on `layer`,
    /// setting [`TileFlags::HAS_EXTRA`] to match. Does nothing if out of bounds. Crate-private:
    /// the external ways in are [`write_grapheme`](Self::write_grapheme) and
    /// [`set_tint`](Self::set_tint).
    ///
    /// An empty entry is removed rather than stored, so the flag means exactly "an entry
    /// exists".
    pub(crate) fn set_extra(&mut self, layer: u8, x: u16, y: u16, extra: TileExtra) {
        if x >= self.width || y >= self.height {
            return;
        }
        let pos = to_grixy_pos(Pos::new(x, y));
        let idx = usize::from(y) * usize::from(self.width) + usize::from(x);
        let lb = self.layer_or_alloc(layer);
        if extra.is_empty() {
            lb.buf[pos].flags.remove(TileFlags::HAS_EXTRA);
            lb.extras.remove(&idx);
        } else {
            lb.buf[pos].flags.insert(TileFlags::HAS_EXTRA);
            lb.extras.insert(idx, extra);
        }
    }

    /// How a pixel backend recolours the sprite drawn for the cell at `(x, y)` on `layer`.
    ///
    /// [`Tint::None`] for a cell that has never been tinted, for a cell whose glyph was
    /// overwritten since (a glyph write drops the tint with the artwork it belonged to), and for
    /// coordinates outside the grid or on an unallocated layer.
    ///
    /// A tint is grid state rather than [`Tile`](crate::tile::Tile) state, for the same reason a multi-codepoint
    /// grapheme is: it is rare per cell and `Tile` has no room
    /// left. So it is read here, not through [`Tile::style`](crate::tile::Tile::style).
    ///
    /// Cell backends have no sprite to recolour and ignore this entirely.
    #[must_use]
    pub fn tint(&self, layer: u8, x: u16, y: u16) -> Tint {
        let Some(lb) = self.layer(layer) else {
            return Tint::None;
        };
        let Some(tile) = lb.buf.get(to_grixy_pos(Pos::new(x, y))) else {
            return Tint::None;
        };
        let idx = usize::from(y) * usize::from(self.width) + usize::from(x);
        lb.tint_for(idx, tile)
    }

    /// Sets how a pixel backend recolours the sprite drawn for the cell at `(x, y)` on `layer`.
    ///
    /// Applies to the cell as it stands, so it belongs *after* the write that put the glyph
    /// there: writing a glyph over a tinted cell drops the tint, on the grounds that a tint
    /// describes the artwork rather than the position. For a multi-cell span, tint the anchor;
    /// that is the cell a pixel backend draws the sprite from.
    ///
    /// Setting [`Tint::None`] clears the tint, and drops the cell's side-table entry entirely if
    /// it held nothing else. Does nothing if `(x, y)` is out of bounds.
    pub fn set_tint(&mut self, layer: u8, x: u16, y: u16, tint: Tint) {
        if x >= self.width || y >= self.height {
            return;
        }
        let idx = usize::from(y) * usize::from(self.width) + usize::from(x);
        let pos = to_grixy_pos(Pos::new(x, y));
        let lb = self.layer_or_alloc(layer);
        // Preserve any grapheme already stored for this cell: the two members of the entry are
        // written by separate calls and neither should clobber the other.
        let grapheme = if lb.buf[pos].flags.contains(TileFlags::HAS_EXTRA) {
            lb.extras.get(&idx).and_then(|e| e.grapheme.clone())
        } else {
            None
        };
        let entry = TileExtra { grapheme, tint };
        if entry.is_empty() {
            lb.buf[pos].flags.remove(TileFlags::HAS_EXTRA);
            lb.extras.remove(&idx);
        } else {
            lb.buf[pos].flags.insert(TileFlags::HAS_EXTRA);
            lb.extras.insert(idx, entry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Tint storage ──────────────────────────────────────────────────────
    //
    // A tint lives in the same sparse side table as a grapheme, so it inherits every path that
    // table already has to get right: rekeying on resize, copying on blit, and being dropped
    // when the cell it belongs to is overwritten or cleared. These cover each of those, plus the
    // interaction between the two members now sharing one entry and one flag.
    #[cfg(feature = "egc")]
    #[test]
    fn tint_round_trips_and_defaults_to_none() {
        let mut g = Grid::new(4, 4);
        assert_eq!(g.tint(0, 1, 1), Tint::None);

        g.write_grapheme(0, 1, 1, "@", Style::default());
        g.set_tint(0, 1, 1, Tint::multiply(128, 64, 32));
        assert_eq!(g.tint(0, 1, 1), Tint::multiply(128, 64, 32));

        // Setting None clears it again.
        g.set_tint(0, 1, 1, Tint::None);
        assert_eq!(g.tint(0, 1, 1), Tint::None);
    }

    #[test]
    fn tint_is_per_layer_and_per_cell() {
        let mut g = Grid::new(4, 4);
        g.set_tint(0, 1, 1, Tint::multiply(10, 20, 30));
        g.set_tint(3, 1, 1, Tint::mix(1, 2, 3, 4));

        assert_eq!(g.tint(0, 1, 1), Tint::multiply(10, 20, 30));
        assert_eq!(g.tint(3, 1, 1), Tint::mix(1, 2, 3, 4));
        assert_eq!(g.tint(0, 1, 2), Tint::None);
        assert_eq!(g.tint(1, 1, 1), Tint::None);
    }

    #[test]
    fn tint_out_of_bounds_reads_none_and_writes_nothing() {
        let mut g = Grid::new(2, 2);
        g.set_tint(0, 9, 9, Tint::multiply(1, 2, 3));
        assert_eq!(g.tint(0, 9, 9), Tint::None);
        assert_eq!(g.tint(0, 0, 0), Tint::None);
    }

    #[cfg(feature = "egc")]
    #[test]
    fn writing_a_glyph_over_a_tinted_cell_drops_the_tint() {
        let mut g = Grid::new(4, 4);
        g.write_grapheme(0, 1, 1, "@", Style::default());
        g.set_tint(0, 1, 1, Tint::multiply(128, 128, 128));

        // A tint describes the artwork that was drawn, not the position, so replacing the
        // artwork drops it rather than silently recolouring whatever lands there next.
        g.write_grapheme(0, 1, 1, "#", Style::default());
        assert_eq!(g.tint(0, 1, 1), Tint::None);
    }

    #[test]
    fn put_tile_drops_the_tint() {
        let mut g = Grid::new(4, 4);
        g.set_tint(0, 1, 1, Tint::multiply(128, 128, 128));
        g.put_tile(0, Pos::new(1, 1), Tile::new('x', Style::default()));
        assert_eq!(g.tint(0, 1, 1), Tint::None);
    }

    #[cfg(feature = "egc")]
    #[test]
    fn a_tint_and_a_grapheme_share_one_entry_without_clobbering_each_other() {
        let mut g = Grid::new(4, 4);
        g.write_grapheme(0, 1, 1, "e\u{0301}", Style::default());
        g.set_tint(0, 1, 1, Tint::multiply(128, 128, 128));

        // Both members survive: `set_tint` preserves the grapheme already stored.
        assert_eq!(crate::grid::grapheme_at(&g, 0, 1, 1), Some("e\u{0301}"));
        assert_eq!(g.tint(0, 1, 1), Tint::multiply(128, 128, 128));

        // Clearing the tint leaves the grapheme, and so leaves the entry in place.
        g.set_tint(0, 1, 1, Tint::None);
        assert_eq!(crate::grid::grapheme_at(&g, 0, 1, 1), Some("e\u{0301}"));
        assert_eq!(g.tint(0, 1, 1), Tint::None);
    }

    #[cfg(feature = "egc")]
    #[test]
    fn a_tint_alone_keeps_grapheme_reads_answering_none() {
        let mut g = Grid::new(4, 4);
        g.write_grapheme(0, 1, 1, "@", Style::default());
        g.set_tint(0, 1, 1, Tint::multiply(128, 128, 128));

        // HAS_EXTRA is now set for a cell with no grapheme text. `grapheme` must still say None
        // rather than reaching into the entry and finding an empty slot.
        assert_eq!(crate::grid::grapheme_at(&g, 0, 1, 1), None);
        assert_eq!(g.tint(0, 1, 1), Tint::multiply(128, 128, 128));
    }
}
