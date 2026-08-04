//! [`Grid::diff`], the zero-allocation per-cell change iterator [`Terminal::present`] and the
//! software backend build on, plus its per-layer [`LayerDiff`] helper.
//!
//! [`Terminal::present`]: crate::Terminal::present

use super::{Grid, Pos, flat_index_to_xy};
use crate::backend::DrawCell;
#[cfg(test)]
use crate::color::Style;
use crate::color::Tint;
use crate::tile::Tile;
#[cfg(test)]
use alloc::vec::Vec;

impl Grid {
    /// Yield a [`DrawCell`] for every changed position across all layers, in layer-major
    /// (`0` → `max(self.max_layer, other.max_layer)`) then row-major order.
    ///
    /// Four cases per layer:
    /// - Layer absent in both `self` and `other`: nothing yielded.
    /// - Layer in `self`, absent in `other` (newly allocated): all `width × height` tiles
    ///   yielded.
    /// - Layer in both, and `self` and `other` have matching dimensions: only positions where
    ///   the `Tile` or its side-table entry (grapheme text, tint) differs are yielded.
    /// - Layer in both, but `self` and `other` have different dimensions: all positions in
    ///   `self` are considered changed, same as a newly allocated layer.
    /// - Layer in `other` but no longer in `self` (stopped being written): every position is
    ///   yielded as a cleared, default [`Tile`], sized to `other`'s dimensions. This case is
    ///   only expressible when `self` and `other` have matching dimensions; a simultaneous size
    ///   change and layer teardown falls back to "nothing yielded" for that layer, matching a
    ///   layer absent from both.
    ///
    /// This iterator is zero-allocation: it walks the layer buffers inline.
    pub fn diff<'a>(&'a self, other: &'a Self) -> impl Iterator<Item = DrawCell<'a>> + 'a {
        let width = usize::from(self.width);
        let max = self.max_layer.max(other.max_layer);
        let same_size = self.width == other.width && self.height == other.height;
        (0..=max).flat_map(move |id| {
            // A size mismatch is treated the same as `other` never having allocated this layer:
            // `other`'s buffer can't be indexed with `self`'s flat index once the sizes differ,
            // so every position in `self` is considered changed, matching grixy's `GridDiff`
            // double-buffering contract.
            let other_layer = if same_size { other.layer(id) } else { None };
            match (self.layer(id), other_layer) {
                // Layer absent in both, or `self` never allocated it while a size mismatch makes
                // `other`'s buffer unusable for positions: nothing changed.
                (None, None) => LayerDiff::Empty,
                // Layer stopped being written: `self` no longer has it, but `other` (same
                // dimensions, checked by `other_layer` above) still does. Report every position
                // as cleared so a compositing backend can retire the layer instead of continuing
                // to show its stale content; see retroglyph#1018.
                (None, Some(prev_lb)) => LayerDiff::Cleared(
                    prev_lb.buf.as_ref().iter().enumerate().map(move |(i, _)| {
                        let (x, y) = flat_index_to_xy(i, width);
                        DrawCell {
                            layer: id,
                            pos: Pos::new(x, y),
                            tile: &Tile::EMPTY,
                            grapheme: None,
                            tint: Tint::None,
                        }
                    }),
                ),
                // Newly allocated layer: all cells are "changed".
                (Some(cur_lb), None) => LayerDiff::Full(
                    cur_lb
                        .buf
                        .as_ref()
                        .iter()
                        .enumerate()
                        .map(move |(i, tile)| {
                            let (x, y) = flat_index_to_xy(i, width);
                            DrawCell {
                                layer: id,
                                pos: Pos::new(x, y),
                                tile,
                                grapheme: cur_lb.extra_for(i, tile),
                                tint: cur_lb.tint_for(i, tile),
                            }
                        }),
                ),
                // Layer in both: only the differing cells. Compared by hand
                // (rather than delegating to grixy's `GridDiff`) because a
                // `Tile`-only comparison can't see grapheme-text changes: two
                // multi-codepoint EGCs sharing a primary codepoint but
                // different combining marks (e.g. `e\u{0301}` vs `e\u{0300}`)
                // compare equal on every `Tile` field.
                (Some(cur_lb), Some(prev_lb)) => {
                    LayerDiff::Diff(cur_lb.buf.as_ref().iter().enumerate().filter_map(
                        move |(i, tile)| {
                            let prev_tile = &prev_lb.buf.as_ref()[i];
                            // The whole entry, not just its grapheme: a `Tile`-only comparison
                            // cannot see a change to either member of the side table, and a
                            // tint-only change is as real a redraw as a combining-mark change.
                            let cur_extra = cur_lb.entry_for(i, tile);
                            let prev_extra = prev_lb.entry_for(i, prev_tile);
                            if tile == prev_tile && cur_extra == prev_extra {
                                return None;
                            }
                            let (x, y) = flat_index_to_xy(i, width);
                            Some(DrawCell {
                                layer: id,
                                pos: Pos::new(x, y),
                                tile,
                                grapheme: cur_extra.and_then(|e| e.grapheme.as_deref()),
                                tint: cur_extra.map_or(Tint::None, |e| e.tint),
                            })
                        },
                    ))
                }
            }
        })
    }
}

/// Per-layer diff iterator, replacing a boxed trait object so `diff` performs
/// no per-layer heap allocation.
enum LayerDiff<F, D, C> {
    Empty,
    Full(F),
    Diff(D),
    Cleared(C),
}

impl<'a, F, D, C> Iterator for LayerDiff<F, D, C>
where
    F: Iterator<Item = DrawCell<'a>>,
    D: Iterator<Item = DrawCell<'a>>,
    C: Iterator<Item = DrawCell<'a>>,
{
    type Item = DrawCell<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Empty => None,
            Self::Full(iter) => iter.next(),
            Self::Diff(iter) => iter.next(),
            Self::Cleared(iter) => iter.next(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grid_diff() {
        let mut g1 = Grid::new(2, 2);
        let g2 = Grid::new(2, 2);

        g1.put_tile(0, (0, 0), Tile::default().with_glyph('A'));

        let diffs: Vec<_> = g1.diff(&g2).collect();
        assert_eq!(diffs.len(), 1);
        assert_eq!(
            diffs[0],
            DrawCell::on_layer(0, Pos::new(0, 0), &g1[Pos::new(0, 0)])
        );
    }

    #[test]
    fn test_grid_diff_empty_when_identical() {
        let g = Grid::new(5, 5);
        let prev = Grid::new(5, 5);
        assert_eq!(g.diff(&prev).count(), 0);
    }

    #[test]
    fn test_grid_diff_reports_changed_cell() {
        let mut cur = Grid::new(5, 5);
        let prev = Grid::new(5, 5);
        cur.put_tile(0, (2, 3), Tile::new('X', Style::default()));
        let diffs: Vec<_> = cur.diff(&prev).collect();
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].layer, 0);
        assert_eq!(diffs[0].pos, Pos::new(2, 3));
        assert_eq!(diffs[0].tile.glyph, 'X');
    }

    #[test]
    fn test_grid_diff_new_layer_yields_all_cells() {
        let mut cur = Grid::new(3, 4);
        let prev = Grid::new(3, 4);
        cur.put_tile(1, (0, 0), Tile::new('A', Style::default()));
        let diffs: Vec<_> = cur.diff(&prev).collect();
        // All 12 cells of the newly allocated layer 1 are yielded.
        assert_eq!(diffs.len(), 12);
        assert!(diffs.iter().all(|c| c.layer == 1));
    }

    #[test]
    fn test_grid_diff_mismatched_sizes_yields_full_diff() {
        // A smaller `other` must not panic; every cell in `self` is reported as changed instead.
        let mut cur = Grid::new(3, 2);
        let prev = Grid::new(2, 2);
        cur.put_tile(0, (0, 0), Tile::new('X', Style::default()));
        let diffs: Vec<_> = cur.diff(&prev).collect();
        assert_eq!(diffs.len(), 6);
        assert!(diffs.iter().all(|c| c.layer == 0));
    }

    #[test]
    fn test_grid_diff_layer_major_order() {
        let mut cur = Grid::new(3, 3);
        let prev = Grid::new(3, 3);
        cur.put_tile(2, (0, 0), Tile::new('B', Style::default()));
        cur.put_tile(0, (1, 0), Tile::new('A', Style::default()));
        let layers: Vec<u8> = cur.diff(&prev).map(|c| c.layer).collect();
        // Layer 0's change appears first, then all of layer 2.
        assert_eq!(layers[0], 0);
        assert!(layers[1..].iter().all(|&l| l == 2));
    }

    #[test]
    fn test_grid_diff_reports_layer_that_stopped_being_written() {
        // frame 1: a HUD is drawn on layer 5.
        let mut a = Grid::new(4, 4);
        a.put_tile(5, (0, 0), Tile::new('H', Style::default()));

        // frame 2: the HUD is hidden, so `b` never allocates layer 5.
        let b = Grid::new(4, 4);

        // `b.max_layer()` is 0, but layer 5's stale content in `a` must still be reported so a
        // compositing backend can clear it instead of continuing to show it (retroglyph#1018).
        let diffs: Vec<_> = b.diff(&a).collect();
        assert_eq!(diffs.len(), 16);
        assert!(diffs.iter().all(|c| c.layer == 5));
        assert!(diffs.iter().all(|c| c.tile.glyph == ' '));
        assert!(diffs.iter().all(|c| c.grapheme.is_none()));
    }

    #[test]
    fn test_grid_diff_stopped_layer_with_size_mismatch_yields_nothing_for_that_layer() {
        // A layer that stopped being written *and* a size change happening at once can't be
        // expressed against `other`'s buffer, so it falls back to "nothing yielded", same as a
        // layer absent from both sides.
        let mut a = Grid::new(4, 4);
        a.put_tile(5, (0, 0), Tile::new('H', Style::default()));
        let b = Grid::new(3, 3);

        let diffs: Vec<_> = b.diff(&a).collect();
        assert!(diffs.iter().all(|c| c.layer != 5));
    }

    #[cfg(feature = "egc")]
    #[test]
    fn test_grid_diff_detects_grapheme_only_change() {
        // Same glyph, style, and flags on both sides: only the combining
        // mark differs. A `Tile`-only diff would miss this.
        let mut cur = Grid::new(2, 2);
        let mut prev = Grid::new(2, 2);
        cur.write_grapheme(0, 0, 0, "e\u{0301}", Style::default());
        prev.write_grapheme(0, 0, 0, "e\u{0300}", Style::default());

        let diffs: Vec<_> = cur.diff(&prev).collect();
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].pos, Pos::new(0, 0));
        assert_eq!(diffs[0].grapheme, Some("e\u{0301}"));

        // Identical grapheme text on both sides: no diff.
        let mut prev2 = Grid::new(2, 2);
        prev2.write_grapheme(0, 0, 0, "e\u{0301}", Style::default());
        assert_eq!(cur.diff(&prev2).count(), 0);
    }

    #[test]
    fn test_grid_diff_detects_tint_only_change() {
        // Same glyph, style, and flags on both sides: only the tint (the side table's other
        // member) differs. A `Tile`-only diff would miss this too, same as the grapheme case.
        let mut cur = Grid::new(2, 2);
        let mut prev = Grid::new(2, 2);
        cur.put_tile(0, (0, 0), Tile::new('@', Style::default()));
        prev.put_tile(0, (0, 0), Tile::new('@', Style::default()));
        cur.set_tint(0, 0, 0, Tint::multiply(64, 128, 192));

        let diffs: Vec<_> = cur.diff(&prev).collect();
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].pos, Pos::new(0, 0));
        assert_eq!(diffs[0].tint, Tint::multiply(64, 128, 192));

        // Identical tint on both sides: no diff.
        let mut prev2 = Grid::new(2, 2);
        prev2.put_tile(0, (0, 0), Tile::new('@', Style::default()));
        prev2.set_tint(0, 0, 0, Tint::multiply(64, 128, 192));
        assert_eq!(cur.diff(&prev2).count(), 0);
    }
}
