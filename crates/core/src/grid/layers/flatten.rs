//! Whole-grid iteration and clearing: [`Grid::layers`], [`Grid::clear_all`], and the
//! single-layer compositing [`Grid::flatten_into`] uses for cell backends.

use super::super::{Grid, Pos, flat_index_to_xy};
use crate::backend::DrawCell;
use crate::color::Color;
#[cfg(test)]
use crate::color::Style;
#[cfg(test)]
use crate::tile::Tile;
use crate::tile::TileFlags;
use grixy::ops::GridWrite;

impl Grid {
    /// Yield a [`DrawCell`] for every allocated cell across all layers, in
    /// layer-major (0 → `max_layer`) then row-major order. `grapheme` is
    /// `Some` only when [`TileFlags::HAS_EXTRA`] is set.
    ///
    /// Unallocated layers are skipped. This is used by backends that need
    /// the full frame on every draw (see [`crate::Output::needs_full_frame`]).
    ///
    /// This iterator is zero-allocation: it walks the layer buffers inline.
    pub fn layers(&self) -> impl Iterator<Item = DrawCell<'_>> + '_ {
        let width = usize::from(self.width);
        (0..=self.max_layer)
            .filter_map(move |id| self.layer(id).map(|lb| (id, lb)))
            .flat_map(move |(id, lb)| {
                lb.buf.as_ref().iter().enumerate().map(move |(i, tile)| {
                    let (x, y) = flat_index_to_xy(i, width);
                    DrawCell {
                        layer: id,
                        pos: Pos::new(x, y),
                        tile,
                        grapheme: lb.extra_for(i, tile),
                        tint: lb.tint_for(i, tile),
                    }
                })
            })
    }

    /// Clears every allocated layer.
    pub fn clear_all(&mut self) {
        for layer in self.layers.iter_mut().flatten() {
            layer.buf.clear();
            layer.extras.clear();
        }
    }

    /// Composites every allocated layer into `dst`'s layer 0, one tile per cell.
    ///
    /// Used by [`crate::Terminal::present`] for backends that do not composite
    /// layers themselves (see [`crate::Output::composites_layers`]). The rule
    /// matches the software renderer's pixel semantics and the [`blit`](Self::blit)
    /// transparency convention:
    ///
    /// - Start from layer 0's tile (its `bg` fills the cell).
    /// - For each higher allocated layer, in ascending order: if the tile is
    ///   not empty (see [`Tile::is_empty`](crate::tile::Tile::is_empty)) replace the glyph, foreground,
    ///   offsets, flags, span, and extra; if its background is not
    ///   [`Color::Default`], replace the background.
    ///
    /// The span fields travel with the flags they are keyed by (see [`Tile::span`](crate::tile::Tile::span)): a
    /// multi-cell span on a higher layer must arrive at a cell backend intact, or its covered
    /// cells lose the anchor they name.
    ///
    /// Because an explicit space is not empty, drawing one on a higher layer
    /// overwrites (erases) the glyph beneath it.
    ///
    /// `dst` must have the same dimensions as `self`.
    ///
    /// Walks layer buffers directly by flat index instead of calling
    /// [`tile`](Self::tile) per cell (see retroglyph#262): that recomputes a coordinate
    /// conversion and a bounds check per cell, which a flat scan over each layer's backing
    /// buffer (the same style [`layers`](Self::layers) and [`diff`](Self::diff) already use)
    /// avoids entirely.
    pub(crate) fn flatten_into(&self, dst: &mut Self) {
        dst.has_spans |= self.has_spans;
        let layer0 = self.layer0();
        let cell_count = layer0.buf.as_ref().len();

        // Seed every destination cell from layer 0: its tile verbatim, and its extra text
        // filtered through `HAS_EXTRA` (the flag is authoritative, see `LayerBuf::extras`'
        // doc comment, so a stale, unflagged entry in `layer0.extras` is not carried over).
        let dst_layer0 = dst.layer0_mut();
        dst_layer0.buf.as_mut().copy_from_slice(layer0.buf.as_ref());
        dst_layer0.extras.clear();
        for (&idx, extra) in &layer0.extras {
            if layer0.buf.as_ref()[idx]
                .flags
                .contains(TileFlags::HAS_EXTRA)
            {
                dst_layer0.extras.insert(idx, extra.clone());
            }
        }

        // Overlay every higher allocated layer, in ascending order, index-for-index.
        for id in 1..=self.max_layer {
            let Some(lb) = self.layer(id) else {
                continue;
            };
            let src_buf = lb.buf.as_ref();
            debug_assert_eq!(src_buf.len(), cell_count);
            let dst_layer0 = dst.layer0_mut();
            for (idx, tile) in src_buf.iter().enumerate() {
                if !tile.flags.contains(TileFlags::EMPTY) {
                    {
                        let out = &mut dst_layer0.buf.as_mut()[idx];
                        out.glyph = tile.glyph;
                        out.width = tile.width;
                        out.style.fg = tile.style.fg;
                        out.dx = tile.dx;
                        out.dy = tile.dy;
                        out.flags = tile.flags;
                        out.span_w = tile.span_w;
                        out.span_h = tile.span_h;
                    }
                    if tile.flags.contains(TileFlags::HAS_EXTRA) {
                        if let Some(extra) = lb.extra_entry_for(idx, tile) {
                            dst_layer0.extras.insert(idx, extra);
                        }
                    } else {
                        dst_layer0.extras.remove(&idx);
                    }
                }
                if tile.style.bg != Color::Default {
                    dst_layer0.buf.as_mut()[idx].style.bg = tile.style.bg;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "egc")]
    #[test]
    fn test_grid_clone_preserves_extra() {
        let mut g = Grid::new(2, 2);
        g.write_grapheme(0, 0, 0, "e\u{0301}", Style::default());
        let cloned = g.clone();
        assert_eq!(
            crate::grid::grapheme_at(&cloned, 0, 0, 0),
            Some("e\u{0301}")
        );
    }

    #[cfg(feature = "egc")]
    #[test]
    fn test_grid_flatten_into_carries_extra_from_higher_layer() {
        let mut g = Grid::new(2, 2);
        g.write_grapheme(1, 0, 0, "e\u{0301}", Style::default());
        let mut flattened = Grid::new(2, 2);
        g.flatten_into(&mut flattened);
        assert_eq!(flattened[Pos::new(0, 0)].glyph, 'e');
        assert_eq!(
            crate::grid::grapheme_at(&flattened, 0, 0, 0),
            Some("e\u{0301}")
        );
    }

    #[test]
    fn test_grid_flatten_into_single_layer_is_a_plain_copy() {
        let mut g = Grid::new(2, 2);
        g.put_tile(0, (0, 0), Tile::new('a', Style::default()));
        g.put_tile(0, (1, 1), Tile::new('b', Style::default()));
        let mut flattened = Grid::new(2, 2);
        g.flatten_into(&mut flattened);
        assert_eq!(flattened[Pos::new(0, 0)].glyph(), 'a');
        assert_eq!(flattened[Pos::new(1, 1)].glyph(), 'b');
        assert_eq!(flattened[Pos::new(1, 0)].glyph(), ' ');
    }

    #[test]
    fn test_grid_flatten_into_higher_layer_overwrites_glyph_and_fg_but_not_default_bg() {
        let mut g = Grid::new(1, 1);
        g.put_tile(
            0,
            (0, 0),
            Tile::new('a', Style::new().fg(Color::BLACK).bg(Color::WHITE)),
        );
        g.put_tile(1, (0, 0), Tile::new('b', Style::new().fg(Color::WHITE)));

        let mut flattened = Grid::new(1, 1);
        g.flatten_into(&mut flattened);
        let out = flattened[Pos::new(0, 0)];
        assert_eq!(out.glyph(), 'b');
        assert_eq!(out.style().fg, Color::WHITE);
        // Layer 1's tile has a `Default` background, so layer 0's background shows through.
        assert_eq!(out.style().bg, Color::WHITE);
    }

    #[test]
    fn test_grid_flatten_into_empty_higher_layer_cell_is_transparent() {
        let mut g = Grid::new(2, 1);
        g.put_tile(0, (0, 0), Tile::new('a', Style::default()));
        g.put_tile(0, (1, 0), Tile::new('b', Style::default()));
        // Only touch (0, 0) on layer 1; (1, 0) on layer 1 stays at its default (EMPTY) tile.
        g.put_tile(1, (0, 0), Tile::new('c', Style::default()));

        let mut flattened = Grid::new(2, 1);
        g.flatten_into(&mut flattened);
        assert_eq!(flattened[Pos::new(0, 0)].glyph(), 'c');
        // Untouched by the transparent layer-1 cell: layer 0's glyph shows through.
        assert_eq!(flattened[Pos::new(1, 0)].glyph(), 'b');
    }

    #[test]
    fn test_grid_flatten_into_multi_layer_stale_dst_extra_is_cleared() {
        // `dst` may be a reused scratch buffer with stale content from a previous frame (see
        // `Terminal::present`): `flatten_into` must fully overwrite it, not merge with it.
        let mut flattened = Grid::new(1, 1);
        flattened.put_tile(0, (0, 0), Tile::new('z', Style::default()));

        let g = Grid::new(1, 1);
        g.flatten_into(&mut flattened);
        assert_eq!(flattened[Pos::new(0, 0)].glyph(), ' ');
    }

    #[test]
    fn flatten_into_carries_span() {
        // Cell backends receive the flattened grid, so a span on a higher layer has to survive
        // flattening with both its flags *and* its span fields, or every covered cell ends up
        // naming an anchor that isn't there.
        let mut grid = Grid::new(4, 4);
        grid.write_span(2, 1, 1, &["C=", "[]"], Style::default())
            .unwrap();

        let mut flat = Grid::new(4, 4);
        grid.flatten_into(&mut flat);

        assert_eq!(flat[Pos::new(1, 1)].span(), (2, 2));
        assert!(
            flat[Pos::new(1, 1)]
                .flags()
                .contains(TileFlags::SPAN_ANCHOR)
        );
        assert_eq!(flat.span_owner(0, 2, 2), Some(Pos::new(1, 1)));
        assert_eq!(flat[Pos::new(2, 2)].glyph(), ']');
    }

    #[test]
    fn copy_layer_from_overwrites_a_cell_the_destination_wrote_this_frame() {
        // retroglyph#956: unlike `blit`, an empty source tile is not transparent, so a cell the
        // destination wrote but the source never touched is erased, not left standing.
        let mut src = Grid::new(4, 1);
        src.put_tile(0, (0, 0), Tile::new('W', Style::default()));

        let mut dst = Grid::new(4, 1);
        dst.put_tile(0, (0, 0), Tile::new('W', Style::default()));
        dst.put_tile(0, (2, 0), Tile::new('X', Style::default()));
        dst.copy_layer_from(0, &src);

        assert_eq!(dst[Pos::new(0, 0)].glyph(), 'W');
        assert_eq!(dst[Pos::new(2, 0)].glyph(), ' ');
    }

    #[test]
    fn copy_layer_from_preserves_span_flags_verbatim() {
        // Unlike `blit`, `copy_layer_from` never clips or offsets, so it has no reason to
        // degrade a span to its fallback glyphs the way
        // `blit_degrades_a_span_to_its_fallback_glyphs` documents for `blit`.
        let mut src = Grid::new(4, 4);
        src.write_span(0, 0, 0, &["C=", "[]"], Style::default())
            .unwrap();

        let mut dst = Grid::new(4, 4);
        dst.copy_layer_from(0, &src);

        assert_eq!(dst[Pos::new(0, 0)].span(), (2, 2));
        assert!(dst[Pos::new(0, 0)].flags().contains(TileFlags::SPAN_ANCHOR));
        assert_eq!(dst.span_owner(0, 1, 1), Some(Pos::new(0, 0)));
        assert_eq!(dst[Pos::new(1, 1)].glyph(), ']');
    }

    #[test]
    fn copy_layer_from_clears_the_destination_when_the_source_layer_is_unallocated() {
        // `src` never wrote layer 1, so `copy_layer_from` treats that as "nothing", clearing
        // whatever `dst` had on layer 1 rather than leaving it untouched.
        let src = Grid::new(4, 1);

        let mut dst = Grid::new(4, 1);
        dst.put_tile(1, (0, 0), Tile::new('X', Style::default()));
        dst.copy_layer_from(1, &src);

        assert_eq!(dst.tile(1, (0, 0)), None);
    }

    #[test]
    fn copy_layer_from_is_a_noop_when_neither_side_has_the_layer_allocated() {
        let src = Grid::new(4, 1);
        let mut dst = Grid::new(4, 1);
        dst.copy_layer_from(1, &src);

        assert_eq!(dst.tile(1, (0, 0)), None);
    }
}
