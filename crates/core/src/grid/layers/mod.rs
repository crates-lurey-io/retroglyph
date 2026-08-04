//! `Grid`'s per-cell tile access: [`Grid::put_tile`], [`Grid::fill_region`], [`Grid::tile`], and
//! [`Grid::tile_mut`], plus per-layer allocation lifecycle ([`Grid::deallocate_layer`],
//! [`Grid::layer_is_empty`]).
//!
//! Per-cell tint/grapheme storage lives in `tint`, cross-grid copies in `blit`, and whole-grid
//! iteration/compositing in `flatten`.

mod blit;
mod flatten;
mod tint;

#[cfg(test)]
use super::TileExtra;
use super::{Grid, Pos, Rect, to_grixy_pos};
#[cfg(test)]
use crate::color::{Style, Tint};
use crate::tile::{Tile, TileFlags};
use alloc::vec::Vec;
use grixy::ops::{ExactSizeGrid, GridRead, GridWrite};

impl Grid {
    /// Writes a tile to `layer` at `pos`, honoring `tile`'s own precomputed
    /// [`width`](Tile::width): a fresh 2-column tile also gets a
    /// [`TileFlags::WIDE_CHAR_SPACER`] at `pos.x + 1`, the same pairing
    /// [`write_grapheme`](Self::write_grapheme) writes, on every feature combination (`Tile::width`
    /// comes from `unicode-width`, an unconditional dependency, not the `egc`-gated
    /// `unicode-segmentation`).
    ///
    /// Allocates the layer if it has not been written to yet. Returns `None` if `pos` is out of
    /// bounds, or if a fresh `tile` is 2 columns wide and `pos.x + 1` (the spacer's column) is
    /// not: the same last-column refusal `write_grapheme` makes, rather than leaving an orphaned
    /// primary cell with no spacer.
    ///
    /// To read back, use [`tile`](Self::tile).
    ///
    /// # Replaying an already-resolved tile
    ///
    /// The wide-char synthesis above only applies to a **fresh** `tile`: one built through public
    /// API ([`Tile::new`], [`with_glyph`](Tile::with_glyph), [`Tile::default`]), which can never
    /// carry [`TileFlags::WIDE_CHAR`]/[`TileFlags::WIDE_CHAR_SPACER`] (both `pub(crate)`-only to
    /// set). A `tile` that already carries either flag is, by construction, an already-resolved
    /// tile read back out of some grid (e.g. [`Headless`](crate::backend::Headless) replaying a
    /// [`DrawCell`](crate::backend::DrawCell) stream verbatim into its own copy) rather than a new
    /// glyph placement, and is written through exactly as given, with no bounds refusal, spacer
    /// synthesis, or overlap clearing of its own: those already happened on the call that
    /// produced it, and re-running them here would (for a spacer tile specifically) clear the
    /// *other* half of the very same wide pair being replayed, mistaking it for some unrelated
    /// write landing on that spacer.
    ///
    /// Any tile written this way has its extra grapheme text cleared, since a
    /// caller-constructed [`Tile`] can never legitimately carry
    /// [`TileFlags::HAS_EXTRA`] (the flag is crate-private). Internal callers
    /// that need to preserve EGC text across a copy (e.g. [`blit`](Self::blit))
    /// follow up with a direct extras-table write. Any multi-cell span the
    /// cell belongs to is cleared first, so a write can never leave an anchor
    /// pointing at cells it no longer owns; a fresh wide `tile` additionally clears any wide
    /// character it would partially overwrite, the same as `write_grapheme`.
    ///
    /// `tile`'s own [`TileFlags::SPAN_ANCHOR`]/[`TileFlags::SPAN_COVERED`] role, if it has one, is
    /// stripped too, for the same reason as `HAS_EXTRA`: those flags are crate-private, so a
    /// caller-supplied `tile` (fresh or replayed, e.g. read back via [`tile`](Self::tile)) can
    /// only carry one by copying it out of some other cell, and writing it through verbatim would
    /// plant an anchor with no covered cells (or a covered cell with no anchor) at `pos` --
    /// exactly the dangling footprint [`write_span`](Self::write_span)'s own doc calls a broken
    /// invariant. [`blit`](Self::blit) makes the same call for a copied span it cannot preserve
    /// whole.
    pub fn put_tile(&mut self, layer: u8, pos: impl Into<Pos>, mut tile: Tile) -> Option<()> {
        let pos = pos.into();

        // See "Replaying an already-resolved tile" above: only a tile that couldn't have come
        // from a public constructor gets treated as verbatim, pre-resolved storage.
        let fresh = !tile
            .flags
            .intersects(TileFlags::WIDE_CHAR | TileFlags::WIDE_CHAR_SPACER);
        let width = if fresh { tile.width() } else { 1 };

        // A 2-column tile needs a spacer at `pos.x + 1`: refuse rather than leave an orphaned
        // primary cell, matching `write_grapheme`'s own last-column refusal.
        if width == 2 && pos.x.saturating_add(1) >= self.width {
            return None;
        }

        // Refuse out-of-bounds before touching `clear_overlap`/`layer_or_alloc` below: neither
        // should allocate the layer for a write that is about to be refused anyway (retroglyph#1012).
        if pos.x >= self.width || pos.y >= self.height {
            return None;
        }

        self.clear_span_overlap(layer, pos.x, pos.y, width.max(1));
        if fresh {
            self.clear_overlap(layer, pos.x, pos.y, width.max(1));
        }

        // Capture the grid width before borrowing `self` mutably below (same reason
        // `write_grapheme` does): `self.width` isn't reachable once `lb` holds `&mut self`.
        let grid_w = usize::from(self.width);
        let gpos = to_grixy_pos(pos);
        let idx = usize::from(pos.y) * grid_w + usize::from(pos.x);
        let lb = self.layer_or_alloc(layer);
        debug_assert!(lb.buf.contains(gpos), "bounds already checked above");
        lb.extras.remove(&idx);
        tile.flags.remove(TileFlags::HAS_EXTRA);
        tile.clear_span();
        if width == 2 {
            tile.flags.insert(TileFlags::WIDE_CHAR);
        }
        let style = tile.style;
        lb.buf[gpos] = tile;

        if width == 2 {
            // The last-column refusal above guarantees `pos.x + 1` is in bounds.
            let spacer_x = pos.x + 1;
            let spacer_gpos = to_grixy_pos(Pos::new(spacer_x, pos.y));
            let spacer_idx = usize::from(pos.y) * grid_w + usize::from(spacer_x);
            lb.extras.remove(&spacer_idx);
            let spacer = &mut lb.buf[spacer_gpos];
            spacer.glyph = ' ';
            spacer.style = style;
            spacer.width = 0;
            spacer.flags = TileFlags::WIDE_CHAR_SPACER;
        }
        Some(())
    }

    /// Fills every cell of `rect` (clipped to this grid) on `layer` with `tile`.
    ///
    /// The batch counterpart to calling [`put_tile`](Self::put_tile) once per cell of `rect`:
    /// same result, but the span/extras bookkeeping and the layer allocation each happen once for
    /// the whole region rather than once per cell, and the write itself is one
    /// [`fill_rect_solid`](grixy::ops::GridWrite::fill_rect_solid) call instead of `rect.width() *
    /// rect.height()` individual cell writes. [`Surface::fill_rect`](crate::surface::Surface::fill_rect),
    /// [`Surface::clear`](crate::surface::Surface::clear), and
    /// [`Surface::clear_region`](crate::surface::Surface::clear_region) are built on this.
    ///
    /// A no-op if `rect` (after clipping to the grid) is empty. As with [`put_tile`](Self::put_tile), `tile` can
    /// never legitimately carry [`TileFlags::HAS_EXTRA`] (the flag is crate-private), so every
    /// cell's own extras entry, if any, is dropped rather than orphaned. `tile`'s own span role,
    /// if it has one, is stripped for the same reason: see `put_tile`'s doc for why writing it
    /// through verbatim would plant a dangling anchor or an anchorless covered cell in every cell
    /// of `rect`.
    ///
    /// Also a no-op if `tile.width() != 1`: unlike `put_tile`, this does not synthesize
    /// [`TileFlags::WIDE_CHAR`]/[`TileFlags::WIDE_CHAR_SPACER`] lead/spacer pairs across the
    /// region, so a wide `tile` (or a zero-width one) would otherwise leave every cell in `rect`
    /// carrying the same glyph with no spacer, desyncing any cursor-advancing consumer that
    /// trusts `Tile::width`/`TileFlags::WIDE_CHAR_SPACER` to track column position. Callers with a
    /// wide glyph need a per-cell [`put_tile`](Self::put_tile) loop instead; see
    /// [`Surface::fill_rect`](crate::surface::Surface::fill_rect)'s own fallback.
    ///
    /// ```
    /// use retroglyph_core::{Grid, Pos, Rect, Style, Tile};
    ///
    /// let mut grid = Grid::new(4, 4);
    /// grid.fill_region(0, Rect::new(1, 1, 2, 2), Tile::new('#', Style::default()));
    ///
    /// assert_eq!(grid[Pos::new(1, 1)].glyph(), '#');
    /// assert_eq!(grid[Pos::new(2, 2)].glyph(), '#');
    /// assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
    /// ```
    pub fn fill_region(&mut self, layer: u8, rect: Rect, mut tile: Tile) {
        let bounds = self.size().to_rect();
        let rect = rect.intersect(bounds);
        if rect.is_empty() {
            return;
        }

        // See this method's own doc comment: a wide `tile` would need a lead/spacer pair
        // synthesized per cell, which this batch path does not do. Refuse rather than write a
        // row of look-alike wide glyphs with no spacers.
        if tile.width() != 1 {
            return;
        }

        // Clear every span this fill would partially overwrite in one pass over the whole rect
        // (see `clear_span_overlap_rect`), rather than once per row: a span spanning several rows
        // of `rect` would otherwise be collected, and fully reset, once per row it occupies
        // (retroglyph#1020). A no-op on a grid that has never used spans.
        self.clear_span_overlap_rect(layer, rect.left(), rect.top(), rect.width(), rect.height());
        // Wide-char overlap has no region-scoped variant (yet): still one call per row, but that
        // remains O(rows) since it never re-collects a growing anchor set. Neither call is gated
        // on `egc`: `put_tile` writes a `WIDE_CHAR`/`WIDE_CHAR_SPACER` pair on every feature
        // combination (see its own doc comment), so a fill that can land inside one has to clean
        // it up regardless of `egc`.
        for row in rect.rows() {
            self.clear_overlap(layer, row.left(), row.top(), row.width());
        }

        // `tile` is a caller-constructed `Tile` (see `put_tile`'s own doc comment for why that
        // can never carry `HAS_EXTRA`), so every cell's own extras entry is now stale. Drop them
        // per row rather than scanning the whole side table: bounded by the region, not by
        // however much of the layer happens to carry extras elsewhere.
        tile.flags.remove(TileFlags::HAS_EXTRA);
        tile.clear_span();
        let grid_w = usize::from(self.width);
        let lb = self.layer_or_alloc(layer);
        for y in rect.top()..rect.bottom() {
            let row_start = usize::from(y) * grid_w + usize::from(rect.left());
            let row_end = row_start + usize::from(rect.width());
            let stale: Vec<usize> = lb
                .extras
                .range(row_start..row_end)
                .map(|(&idx, _)| idx)
                .collect();
            for idx in stale {
                lb.extras.remove(&idx);
            }
        }

        let dst = grixy::core::Rect::new(
            usize::from(rect.left()),
            usize::from(rect.top()),
            usize::from(rect.width()),
            usize::from(rect.height()),
        );
        lb.buf.fill_rect_solid(dst, tile);
    }

    /// Reads a tile on `layer` at `pos`, or `None` if the layer is
    /// unallocated or `pos` is out of bounds.
    #[must_use]
    pub fn tile(&self, layer: u8, pos: impl Into<Pos>) -> Option<&Tile> {
        let pos = to_grixy_pos(pos.into());
        self.layer(layer)?.buf.get(pos)
    }

    /// Mutably borrows a tile on `layer` at `pos`, or `None` if the layer is
    /// unallocated or `pos` is out of bounds.
    ///
    /// This hands out a direct `&mut Tile`, so it cannot intercept a write the way
    /// [`put_tile`](Self::put_tile) does: it does not clear a multi-cell span `pos` belongs to,
    /// and it does not clear grapheme extras stored for the tile. Call
    /// [`clear_span`](Self::clear_span) first if `pos` may belong to a span.
    pub fn tile_mut(&mut self, layer: u8, pos: impl Into<Pos>) -> Option<&mut Tile> {
        let pos = to_grixy_pos(pos.into());
        self.layers
            .get_mut(usize::from(layer))?
            .as_mut()?
            .buf
            .get_mut(pos)
    }

    /// [`tile_mut`](Self::tile_mut), allocating `layer` if it is not allocated yet.
    ///
    /// `None` only when `pos` is out of bounds, which (as in [`set_extra`](Self::set_extra))
    /// leaves `layer` unallocated rather than allocating a buffer nothing can be written to.
    /// Shares `tile_mut`'s caveat: this is a raw `&mut Tile`, so it performs none of
    /// [`put_tile`](Self::put_tile)'s span or overlap repair.
    pub(crate) fn tile_mut_or_alloc(&mut self, layer: u8, pos: Pos) -> Option<&mut Tile> {
        if pos.x >= self.width || pos.y >= self.height {
            return None;
        }
        let gpos = to_grixy_pos(pos);
        self.layer_or_alloc(layer).buf.get_mut(gpos)
    }

    /// Deallocates `layer`, freeing its buffer entirely rather than clearing its content in
    /// place.
    ///
    /// Unlike [`clear_all`](Self::clear_all) (and a per-layer clear via [`cells_mut`](Self::cells_mut)),
    /// which empty a layer's cells but leave it allocated, this drops the [`LayerBuf`] itself, the
    /// same table slot [`layer_or_alloc`](Self::layer_or_alloc) fills in on first write. That
    /// matters because [`max_layer`](Self::max_layer) only ever grows on write (see its own doc):
    /// a layer that is merely cleared still counts toward it, while a deallocated one does not,
    /// letting `max_layer` fall back down once every layer above it is also gone. This is what lets
    /// [`crate::Terminal::drop_layer`] undo a layer's permanent allocation and, once every layer
    /// above 0 is dropped, put [`crate::Terminal::present`] back on its single-layer fast path
    /// (retroglyph#1028).
    ///
    /// If `layer` was the current `max_layer`, the table is rescanned downward for the next
    /// highest still-allocated layer, `O(max_layer)` in the worst case; deallocating any other
    /// layer is `O(1)`. Deallocating an already-unallocated layer (or one past the table's
    /// current length) is a no-op.
    ///
    /// # Panics
    ///
    /// Panics if `layer` is 0: layer 0 is always allocated (see [`Grid::new`]) and can never be
    /// deallocated.
    pub(crate) fn deallocate_layer(&mut self, layer: u8) {
        assert_ne!(
            layer, 0,
            "layer 0 is always allocated and cannot be deallocated"
        );
        let idx = usize::from(layer);
        if idx >= self.layers.len() {
            return;
        }
        self.layers[idx] = None;
        if layer == self.max_layer {
            self.max_layer = (0..layer)
                .rev()
                .find(|&id| self.layers[usize::from(id)].is_some())
                .unwrap_or(0);
        }
    }

    /// Whether `layer` is unallocated, or allocated but every tile on it is untouched (see
    /// [`Tile::is_empty`]).
    ///
    /// Used by [`crate::Terminal::drop_layer`]'s deferred deallocation to tell a layer that is
    /// still genuinely empty (safe to free) apart from one that was redrawn after the drop was
    /// requested (some tile is no longer empty), which must cancel the drop instead of silently
    /// discarding content the app just wrote.
    pub(crate) fn layer_is_empty(&self, layer: u8) -> bool {
        self.layer(layer)
            .is_none_or(|lb| lb.buf.as_ref().iter().all(Tile::is_empty))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grid_put_get() {
        let mut grid = Grid::new(10, 10);
        let tile = Tile::default().with_glyph('X');

        grid.put_tile(0, (5, 5), tile);
        assert_eq!(grid[Pos::new(5, 5)].glyph(), 'X');
    }

    #[test]
    fn test_grid_checked_put_get() {
        let mut grid = Grid::new(10, 10);
        let tile = Tile::default().with_glyph('Y');

        assert!(grid.put_tile(0, (5, 5), tile).is_some());
        assert_eq!(grid.tile(0, (5, 5)).unwrap().glyph(), 'Y');

        assert!(grid.tile(0, (10, 0)).is_none());
        assert!(grid.put_tile(0, (0, 10), Tile::default()).is_none());
    }

    #[test]
    fn test_grid_put_tile_out_of_bounds_returns_none() {
        let mut grid = Grid::new(10, 10);
        assert!(grid.put_tile(0, (10, 0), Tile::default()).is_none());
    }

    #[test]
    fn test_grid_layers_yields_every_allocated_cell_in_layer_then_row_major_order() {
        let mut grid = Grid::new(2, 2);
        grid.put_tile(0, (1, 0), Tile::default().with_glyph('A'));
        grid.put_tile(2, (0, 1), Tile::default().with_glyph('B'));

        let cells: Vec<_> = grid
            .layers()
            .map(|c| (c.layer, c.pos, c.tile.glyph()))
            .collect();

        // Layer 1 is never allocated, so it's skipped entirely; layer 0's four cells (row-major)
        // come before layer 2's four cells.
        assert_eq!(
            cells,
            [
                (0, Pos::new(0, 0), ' '),
                (0, Pos::new(1, 0), 'A'),
                (0, Pos::new(0, 1), ' '),
                (0, Pos::new(1, 1), ' '),
                (2, Pos::new(0, 0), ' '),
                (2, Pos::new(1, 0), ' '),
                (2, Pos::new(0, 1), 'B'),
                (2, Pos::new(1, 1), ' '),
            ]
        );
    }

    #[test]
    fn test_grid_layer_zero_always_allocated() {
        let g = Grid::new(5, 5);
        assert!(g.layer(0).is_some());
        for id in 1u8..=5 {
            assert!(g.layer(id).is_none(), "layer {id} should be None");
        }
    }

    #[test]
    fn test_grid_put_tile_allocates_layer() {
        let mut g = Grid::new(5, 5);
        g.put_tile(3, (0, 0), Tile::new('@', Style::default()));
        assert!(g.layer(3).is_some());
        assert!(g.layer(4).is_none());
    }

    #[test]
    fn test_grid_new_layer_table_starts_at_a_single_slot() {
        // retroglyph#264: the layer-table `Vec` itself should start small (a single slot for
        // layer 0), not pre-allocate all 256 possible slots up front.
        let g = Grid::new(5, 5);
        assert_eq!(g.layers.len(), 1);
        assert_eq!(g.max_layer(), 0);
    }

    #[test]
    fn test_grid_layer_or_alloc_grows_table_lazily_to_the_written_id() {
        let mut g = Grid::new(5, 5);
        g.put_tile(10, (0, 0), Tile::new('@', Style::default()));
        // The table grows to exactly `id + 1` slots, not all 256.
        assert_eq!(g.layers.len(), 11);
        assert_eq!(g.max_layer(), 10);
        assert!(g.layer(10).is_some());
        for id in 1u8..10 {
            assert!(g.layer(id).is_none(), "layer {id} should be None");
        }
    }

    #[test]
    fn test_grid_layer_beyond_table_length_reads_as_none() {
        // A layer id past the current table length (never written) must read identically to an
        // in-bounds `None` slot, not panic or error.
        let g = Grid::new(5, 5);
        assert_eq!(g.layers.len(), 1);
        assert!(g.layer(255).is_none());
        assert!(g.tile(255, (0, 0)).is_none());
        assert!(crate::grid::grapheme_at(&g, 255, 0, 0).is_none());
    }

    #[test]
    fn test_grid_clear_beyond_table_length_is_a_no_op() {
        // Clearing an id past the current table length must not panic: it's equivalent to
        // clearing an unallocated in-bounds layer (does nothing).
        let mut g = Grid::new(5, 5);
        g.clear(255);
        assert_eq!(g.layers.len(), 1);
    }

    #[test]
    fn put_tile_out_of_bounds_does_not_allocate_the_layer() {
        // retroglyph#1012: a refused out-of-bounds write must not allocate the layer or raise
        // `max_layer`, matching `put_tile`'s own "does nothing" contract.
        let mut g = Grid::new(4, 4);
        assert_eq!(
            g.put_tile(200, (99, 99), Tile::new('x', Style::default())),
            None
        );
        assert_eq!(g.max_layer(), 0);
        assert!(g.tile(200, (0, 0)).is_none());
        assert!(g.layer(200).is_none());
    }

    #[test]
    fn set_tint_out_of_bounds_does_not_allocate_the_layer() {
        // retroglyph#1012: same guarantee as `put_tile`, for the `set_tint` write path.
        let mut g = Grid::new(4, 4);
        g.set_tint(200, 99, 99, Tint::multiply(1, 2, 3));
        assert_eq!(g.max_layer(), 0);
        assert!(g.layer(200).is_none());
    }

    #[test]
    #[should_panic(expected = "layer 0 is always allocated")]
    fn test_grid_deallocate_layer_zero_panics() {
        let mut g = Grid::new(4, 4);
        g.deallocate_layer(0);
    }

    #[test]
    fn test_grid_deallocate_layer_the_top_layer_lowers_max_layer() {
        let mut g = Grid::new(4, 4);
        g.put_tile(3, (0, 0), Tile::new('@', Style::default()));
        assert_eq!(g.max_layer(), 3);

        g.deallocate_layer(3);
        assert_eq!(g.max_layer(), 0);
        assert!(g.layer(3).is_none());
    }

    #[test]
    fn test_grid_deallocate_layer_rescans_down_to_the_next_allocated_layer() {
        let mut g = Grid::new(4, 4);
        g.put_tile(2, (0, 0), Tile::new('a', Style::default()));
        g.put_tile(5, (0, 0), Tile::new('b', Style::default()));
        assert_eq!(g.max_layer(), 5);

        g.deallocate_layer(5);
        assert_eq!(g.max_layer(), 2);
        assert!(g.layer(5).is_none());
        assert!(g.layer(2).is_some());
    }

    #[test]
    fn test_grid_deallocate_layer_below_the_top_leaves_max_layer_unchanged() {
        let mut g = Grid::new(4, 4);
        g.put_tile(2, (0, 0), Tile::new('a', Style::default()));
        g.put_tile(5, (0, 0), Tile::new('b', Style::default()));

        g.deallocate_layer(2);
        assert_eq!(g.max_layer(), 5);
        assert!(g.layer(2).is_none());
        assert!(g.layer(5).is_some());
    }

    #[test]
    fn test_grid_deallocate_layer_reads_back_as_unallocated() {
        let mut g = Grid::new(4, 4);
        g.put_tile(1, (0, 0), Tile::new('@', Style::default()));
        g.deallocate_layer(1);
        assert!(g.tile(1, (0, 0)).is_none());
    }

    #[test]
    fn test_grid_deallocate_layer_already_unallocated_is_a_no_op() {
        let mut g = Grid::new(4, 4);
        g.deallocate_layer(1);
        assert_eq!(g.max_layer(), 0);

        // Past the table's current length entirely.
        g.deallocate_layer(200);
        assert_eq!(g.max_layer(), 0);
    }

    #[test]
    fn set_extra_out_of_bounds_does_not_allocate_the_layer() {
        // retroglyph#1012: same guarantee as `put_tile`/`set_tint`, for the crate-private
        // `set_extra` write path (reached from `Headless::draw_layers` with whatever `pos` the
        // replayed `DrawCell` stream carries, which is not itself bounds-checked there).
        let mut g = Grid::new(4, 4);
        g.set_extra(
            200,
            99,
            99,
            TileExtra {
                grapheme: None,
                tint: Tint::multiply(1, 2, 3),
            },
        );
        assert_eq!(g.max_layer(), 0);
        assert!(g.layer(200).is_none());
    }

    #[test]
    fn test_grid_layer_table_growth_is_monotonic_across_writes() {
        // Writing to a lower layer id after a higher one must not shrink the table, and must
        // preserve the higher layer's content.
        let mut g = Grid::new(5, 5);
        g.put_tile(20, (1, 1), Tile::new('H', Style::default()));
        assert_eq!(g.layers.len(), 21);
        g.put_tile(2, (0, 0), Tile::new('L', Style::default()));
        assert_eq!(
            g.layers.len(),
            21,
            "writing a lower id must not shrink the table"
        );
        assert_eq!(g.max_layer(), 20);
        assert_eq!(g.tile(20, (1, 1)).unwrap().glyph, 'H');
        assert_eq!(g.tile(2, (0, 0)).unwrap().glyph, 'L');
    }

    #[test]
    fn test_grid_put_and_get_on_layer_2() {
        use crate::color::Style;
        let mut g = Grid::new(5, 5);
        g.put_tile(2, (1, 1), Tile::new('Z', Style::default()));
        assert_eq!(g.tile(2, (1, 1)).unwrap().glyph, 'Z');
        // Layer 0 at same position should still be default.
        assert_eq!(g[Pos::new(1, 1)].glyph, ' ');
        // Unallocated layer returns None.
        assert!(g.tile(3, (0, 0)).is_none());
    }

    #[test]
    fn test_grid_tile_mut_writes_in_place_without_clearing_spans() {
        let mut g = Grid::new(4, 4);
        g.write_span(0, 0, 0, &["C=", "[]"], Style::default())
            .unwrap();

        // Unlike `put_tile`, `tile_mut` hands out a direct `&mut Tile` and does not intercept
        // the write, so the span's other cells are left dangling on purpose here.
        g.tile_mut(0, (0, 0)).unwrap().glyph = 'x';
        assert_eq!(g[Pos::new(0, 0)].glyph(), 'x');

        // Unallocated layer and out-of-bounds position both report `None`, not a panic.
        assert!(g.tile_mut(1, (0, 0)).is_none());
        assert!(g.tile_mut(0, (10, 10)).is_none());
    }

    #[test]
    fn test_grid_clear_layer() {
        let mut g = Grid::new(5, 5);
        g.put_tile(1, (0, 0), Tile::new('Z', Style::default()));
        g.put_tile(0, (0, 0), Tile::new('A', Style::default()));
        g.clear(1);
        assert_eq!(g.tile(0, (0, 0)).unwrap().glyph, 'A');
        assert!(g.tile(1, (0, 0)).is_some());
        assert_eq!(g.tile(1, (0, 0)).unwrap().glyph, ' '); // cleared
    }

    #[test]
    fn test_grid_clear_all() {
        let mut g = Grid::new(5, 5);
        g.put_tile(1, (0, 0), Tile::new('Z', Style::default()));
        g.put_tile(0, (0, 0), Tile::new('A', Style::default()));
        g.clear_all();
        // Both layers reset to default (space).
        assert_eq!(g[Pos::new(0, 0)].glyph, ' ');
        assert_eq!(g.tile(1, (0, 0)).unwrap().glyph, ' ');
    }

    #[test]
    fn test_grid_clone_is_independent() {
        let mut g = Grid::new(3, 3);
        g.put_tile(0, (0, 0), Tile::new('A', Style::default()));
        g.put_tile(2, (1, 1), Tile::new('B', Style::default()));

        let mut cloned = g.clone();
        assert_eq!(cloned[Pos::new(0, 0)].glyph, 'A');
        assert_eq!(cloned.tile(2, (1, 1)).unwrap().glyph, 'B');
        assert_eq!(cloned.max_layer(), g.max_layer());

        // Mutating the clone must not affect the original (deep copy).
        cloned.put_tile(0, (0, 0), Tile::new('Z', Style::default()));
        assert_eq!(cloned[Pos::new(0, 0)].glyph, 'Z');
        assert_eq!(g[Pos::new(0, 0)].glyph, 'A');
    }

    /// `put_tile` is wide-char aware on every feature combination (retroglyph#869): a 2-column
    /// tile gets a `WIDE_CHAR` primary cell and a `WIDE_CHAR_SPACER` to its right, the same pair
    /// `write_grapheme` (egc-only) writes.
    #[test]
    fn put_tile_writes_a_spacer_for_a_wide_glyph() {
        let mut g = Grid::new(4, 4);
        assert_eq!(
            g.put_tile(0, (0, 0), Tile::new('\u{4e2d}', Style::default())),
            Some(())
        );

        assert_eq!(g[Pos::new(0, 0)].glyph(), '\u{4e2d}');
        assert!(g[Pos::new(0, 0)].flags().contains(TileFlags::WIDE_CHAR));
        assert_eq!(g[Pos::new(1, 0)].glyph(), ' ');
        assert!(
            g[Pos::new(1, 0)]
                .flags()
                .contains(TileFlags::WIDE_CHAR_SPACER)
        );
        // Untouched past the spacer.
        assert_eq!(g[Pos::new(2, 0)].glyph(), ' ');
        assert!(
            !g[Pos::new(2, 0)]
                .flags()
                .contains(TileFlags::WIDE_CHAR_SPACER)
        );
    }

    /// Mirrors `write_grapheme`'s own last-column refusal: a wide tile whose spacer would fall
    /// off the grid is refused outright rather than leaving an orphaned primary cell.
    #[test]
    fn put_tile_refuses_a_wide_glyph_at_the_last_column() {
        let mut g = Grid::new(4, 4);
        assert_eq!(
            g.put_tile(0, (3, 0), Tile::new('\u{4e2d}', Style::default())),
            None
        );
        assert_eq!(g[Pos::new(3, 0)].glyph(), ' ');
    }

    /// Overwriting a wide char's primary cell (with a narrow tile) must clear its now-orphaned
    /// spacer, the same overlap-clearing `write_grapheme` already does.
    #[test]
    fn put_tile_clears_the_spacer_of_a_wide_glyph_it_overwrites() {
        let mut g = Grid::new(4, 4);
        g.put_tile(0, (0, 0), Tile::new('\u{4e2d}', Style::default()));
        g.put_tile(0, (0, 0), Tile::new('a', Style::default()));

        assert_eq!(g[Pos::new(0, 0)].glyph(), 'a');
        assert!(
            !g[Pos::new(1, 0)]
                .flags()
                .contains(TileFlags::WIDE_CHAR_SPACER)
        );
    }

    /// Overwriting a wide char's spacer cell must clear its now-orphaned primary cell too.
    #[test]
    fn put_tile_clears_the_lead_of_a_wide_glyph_whose_spacer_it_overwrites() {
        let mut g = Grid::new(4, 4);
        g.put_tile(0, (0, 0), Tile::new('\u{4e2d}', Style::default()));
        g.put_tile(0, (1, 0), Tile::new('a', Style::default()));

        assert_eq!(g[Pos::new(1, 0)].glyph(), 'a');
        assert!(!g[Pos::new(0, 0)].flags().contains(TileFlags::WIDE_CHAR));
    }

    /// A caller-constructed `tile` can carry a stale [`TileFlags::SPAN_ANCHOR`]/
    /// [`TileFlags::SPAN_COVERED`] role only by having been read back out of some grid cell
    /// (e.g. via [`tile`](Grid::tile)), since neither flag has a public builder. `put_tile` must
    /// strip it rather than plant a dangling anchor: one that claims a footprint no covered cell
    /// agrees it owns (retroglyph#984).
    #[test]
    fn put_tile_strips_a_span_anchor_replayed_from_elsewhere() {
        let mut g = Grid::new(8, 4);
        g.write_span_uniform(0, (0, 0), (2u16, 2u16), 'A', '.', Style::default())
            .expect("2x2 span fits in an 8x4 grid");

        // Replay the anchor tile somewhere unrelated.
        let anchor = *g.tile(0, Pos::new(0, 0)).unwrap();
        assert!(anchor.flags().contains(TileFlags::SPAN_ANCHOR));
        g.put_tile(0, Pos::new(5, 3), anchor);

        let replayed = g.tile(0, Pos::new(5, 3)).unwrap();
        assert!(!replayed.flags().contains(TileFlags::SPAN_ANCHOR));
        assert_eq!(replayed.span(), (1, 1));
        assert_eq!(g.span_owner(0, 5, 3), None);
        // No dangling footprint means nothing beyond (5, 3) got claimed either.
        assert_eq!(g.span_owner(0, 6, 3), None);
    }

    /// The dangling anchor from the previous test would otherwise make `clear_span` (via
    /// `reset_span_at`) walk the anchor's declared `span_w`/`span_h` and reset every cell in that
    /// bogus footprint, destroying unrelated content that was never part of any span
    /// (retroglyph#984).
    #[test]
    fn put_tile_strips_a_span_anchor_so_clear_span_cannot_destroy_a_neighbour() {
        let mut g = Grid::new(8, 4);
        g.write_span_uniform(0, (0, 0), (2u16, 2u16), 'A', '.', Style::default())
            .expect("2x2 span fits in an 8x4 grid");
        let anchor = *g.tile(0, Pos::new(0, 0)).unwrap();

        g.put_tile(0, Pos::new(5, 3), anchor);
        g.put_tile(0, Pos::new(6, 3), Tile::new('Z', Style::default()));
        g.clear_span(0, 5, 3);

        assert_eq!(g.tile(0, Pos::new(6, 3)).unwrap().glyph(), 'Z');
    }

    /// A replayed `SPAN_COVERED` tile must also lose its role, or a pixel backend (which skips
    /// drawing any covered cell on the assumption its anchor drew the art) would render nothing
    /// at the destination (retroglyph#984).
    #[test]
    fn put_tile_strips_a_span_covered_role_replayed_from_elsewhere() {
        let mut g = Grid::new(8, 4);
        g.write_span_uniform(0, (0, 0), (2u16, 2u16), 'A', '.', Style::default())
            .expect("2x2 span fits in an 8x4 grid");
        let covered = *g.tile(0, Pos::new(1, 1)).unwrap();
        assert!(covered.flags().contains(TileFlags::SPAN_COVERED));

        g.put_tile(0, Pos::new(5, 3), covered);

        let replayed = g.tile(0, Pos::new(5, 3)).unwrap();
        assert!(!replayed.flags().contains(TileFlags::SPAN_COVERED));
        assert_eq!(g.span_owner(0, 5, 3), None);
    }

    /// A tile rebuilt via `with_glyph` from a spacer read back out of a grid must actually get
    /// drawn: before the fix, the stale `WIDE_CHAR_SPACER` flag made `put_tile` treat it as an
    /// already-resolved replay and store it verbatim, so backends skipped it (retroglyph#986).
    #[test]
    fn put_tile_draws_a_spacer_rebuilt_through_with_glyph() {
        let mut g = Grid::new(8, 4);
        g.put_tile(0, (0, 0), Tile::new('\u{6f22}', Style::default()));

        let spacer = g[Pos::new(1, 0)];
        assert!(spacer.flags().contains(TileFlags::WIDE_CHAR_SPACER));

        let modified = spacer.with_glyph('!');
        g.put_tile(0, (4, 0), modified);

        let placed = g[Pos::new(4, 0)];
        assert_eq!(placed.glyph(), '!');
        assert_eq!(placed.width(), 1);
        assert!(!placed.flags().contains(TileFlags::WIDE_CHAR_SPACER));
    }

    /// A tile rebuilt via `with_glyph` from a wide lead read back out of a grid must not carry a
    /// stale `WIDE_CHAR` flag: before the fix, `clear_overlap` trusted it to mean "my right
    /// neighbour is my spacer" and reset an unrelated tile on the next overlapping write
    /// (retroglyph#986).
    #[test]
    fn put_tile_narrowed_through_with_glyph_does_not_clobber_its_neighbour() {
        let mut g = Grid::new(8, 4);
        g.put_tile(0, (0, 0), Tile::new('\u{6f22}', Style::default()));

        let wide = g[Pos::new(0, 0)];
        assert!(wide.flags().contains(TileFlags::WIDE_CHAR));

        let narrow = wide.with_glyph('A');
        g.put_tile(0, (4, 0), narrow);
        g.put_tile(0, (5, 0), Tile::new('Z', Style::default()));
        g.put_tile(0, (4, 0), Tile::new('B', Style::default()));

        assert_eq!(g[Pos::new(5, 0)].glyph(), 'Z');
    }

    /// `fill_region` must clear any span it would partially overwrite the same way a per-cell
    /// `put_tile` loop would (via `clear_span_overlap`), or the surviving span's anchor would
    /// keep claiming a footprint the fill just overwrote part of.
    #[test]
    fn fill_region_clears_a_span_it_partially_overwrites() {
        let mut g = Grid::new(4, 4);
        g.write_span(0, 0, 0, &["C=", "[]"], Style::default())
            .expect("2x2 span fits in a 4x4 grid");

        // Overlaps only the span's right column, (1, 0) and (1, 1).
        g.fill_region(0, Rect::new(1, 0, 3, 3), Tile::new('#', Style::default()));

        // The anchor at (0, 0) is gone, not left claiming a footprint that no longer matches
        // reality.
        let anchor = g.tile(0, (0, 0)).unwrap();
        assert!(!anchor.flags().contains(TileFlags::SPAN_ANCHOR));
        assert_eq!(anchor.glyph(), ' ');
    }

    /// `fill_region` scans the region for overlapping spans once, not once per row (see
    /// `clear_span_overlap_rect`, retroglyph#1020): a span several rows tall, entirely inside
    /// `rect`, must still come out fully and correctly reset rather than leaving a stale anchor
    /// or covered cell behind from a row the single-pass collection missed.
    #[test]
    fn fill_region_clears_a_multi_row_span_it_fully_covers() {
        let mut g = Grid::new(6, 6);
        g.write_span(0, 1, 1, &["AB", "CD", "EF", "GH"], Style::default())
            .expect("2x4 span fits in a 6x6 grid");

        g.fill_region(0, Rect::new(0, 0, 6, 6), Tile::new('#', Style::default()));

        for y in 0..6 {
            for x in 0..6 {
                assert_eq!(g.tile(0, (x, y)).unwrap().glyph(), '#', "({x}, {y})");
            }
        }
    }

    /// `clear_overlap` runs regardless of `egc` (see its own doc comment): `fill_region` gated it
    /// behind the feature until retroglyph#1014, so a wide pair written by `put_tile` (which is
    /// not itself `egc`-gated) kept a stale `WIDE_CHAR` flag after a fill partially overwrote it
    /// with `egc` off.
    #[test]
    fn fill_region_clears_a_wide_char_it_partially_overwrites() {
        let mut g = Grid::new(4, 1);
        g.put_tile(0, (0, 0), Tile::new('\u{4e2d}', Style::default()));
        assert!(
            g.tile(0, (0, 0))
                .unwrap()
                .flags()
                .contains(TileFlags::WIDE_CHAR)
        );

        g.fill_region(0, Rect::new(1, 0, 3, 1), Tile::new('#', Style::default()));

        assert!(
            !g.tile(0, (0, 0))
                .unwrap()
                .flags()
                .contains(TileFlags::WIDE_CHAR)
        );
    }

    /// `fill_region` writing a wide `tile` raw (no lead/spacer synthesis) would desync any
    /// cursor-advancing consumer that trusts `Tile::width`/`WIDE_CHAR_SPACER` to track column
    /// position (retroglyph#1014). It refuses instead, leaving the region untouched.
    #[test]
    fn fill_region_refuses_a_wide_tile() {
        let mut g = Grid::new(4, 1);

        g.fill_region(
            0,
            Rect::new(0, 0, 4, 1),
            Tile::new('\u{4e2d}', Style::default()),
        );

        for x in 0..4 {
            let tile = g.tile(0, (x, 0)).unwrap();
            assert_eq!(tile.glyph(), ' ');
            assert_eq!(tile.flags(), TileFlags::EMPTY);
        }
    }

    /// `fill_region` writes a caller-constructed `Tile`, which (like `put_tile`) can never
    /// legitimately carry `HAS_EXTRA`, so any grapheme/tint side-table entry the fill's cells
    /// used to own must be dropped, not left dangling under the new tile.
    #[cfg(feature = "egc")]
    #[test]
    fn fill_region_drops_stale_extras() {
        let mut g = Grid::new(4, 4);
        g.write_grapheme(0, 1, 1, "e\u{0301}", Style::default());
        g.set_tint(0, 2, 2, Tint::multiply(1, 2, 3));

        g.fill_region(0, Rect::new(0, 0, 4, 4), Tile::new('#', Style::default()));

        assert_eq!(crate::grid::grapheme_at(&g, 0, 1, 1), None);
        assert_eq!(g.tint(0, 2, 2), Tint::None);
    }

    /// A `rect` that extends past the grid's own edges only fills the in-bounds overlap, the
    /// same clipping `put_tile` gets for free per cell by refusing an out-of-bounds `pos`.
    #[test]
    fn fill_region_clips_to_grid_bounds() {
        let mut g = Grid::new(4, 4);
        g.fill_region(0, Rect::new(2, 2, 10, 10), Tile::new('#', Style::default()));

        assert_eq!(g.tile(0, (3, 3)).unwrap().glyph(), '#');
        assert_eq!(g.tile(0, (0, 0)).unwrap().glyph(), ' ');
    }

    /// A `tile` carrying a replayed `SPAN_ANCHOR` role must not survive into every cell of
    /// `rect`: without stripping it, a 2x2 fill with a replayed anchor would produce four anchors
    /// each wrongly claiming their own 2x2 footprint (retroglyph#984).
    #[test]
    fn fill_region_strips_a_span_anchor_replayed_from_elsewhere() {
        let mut g = Grid::new(8, 4);
        g.write_span_uniform(0, (0, 0), (2u16, 2u16), 'A', '.', Style::default())
            .expect("2x2 span fits in an 8x4 grid");
        let anchor = *g.tile(0, Pos::new(0, 0)).unwrap();

        g.fill_region(0, Rect::new(4, 0, 2, 2), anchor);

        for y in 0..2 {
            for x in 4..6 {
                let cell = g.tile(0, Pos::new(x, y)).unwrap();
                assert!(!cell.flags().contains(TileFlags::SPAN_ANCHOR));
                assert_eq!(g.span_owner(0, x, y), None);
            }
        }
    }

    /// An empty (or fully out-of-bounds) `rect` allocates nothing: `fill_region` returns before
    /// touching `layer_or_alloc`.
    #[test]
    fn fill_region_on_an_empty_rect_does_not_allocate_the_layer() {
        let mut g = Grid::new(4, 4);
        g.fill_region(1, Rect::new(10, 10, 2, 2), Tile::new('#', Style::default()));
        assert_eq!(g.tile(1, (0, 0)), None);
    }
}
