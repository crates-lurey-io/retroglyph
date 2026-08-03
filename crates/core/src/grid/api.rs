//! `Grid`'s public API forwarding to layer 0: construction, dimensions, per-cell reads,
//! iteration, resizing, and (`egc`-gated) grapheme writes.

#[cfg(feature = "egc")]
use super::TileExtra;
use super::{Cells, CellsMut, Grid, LayerBuf, Pos, Rect, Size, to_grixy_pos};
#[cfg(feature = "egc")]
use crate::color::Style;
#[cfg(any(test, feature = "egc"))]
use crate::color::Tint;
#[cfg(feature = "egc")]
use crate::tile::cap_grapheme;
use crate::tile::{Tile, TileFlags};
#[cfg(feature = "egc")]
use alloc::sync::Arc;
use grixy::ops::{GridRead, GridWrite};

impl Grid {
    /// Creates a new grid of the given dimensions.
    ///
    /// Layer 0 is allocated immediately. Layers 1–255 are `None` until first
    /// write via [`put_tile`](Self::put_tile); the layer table itself only
    /// grows as far as the highest layer id ever written, not all 256 slots
    /// up front.
    ///
    /// `height` may be 0 (an empty grid with no rows). [`resize`](Self::resize) may shrink an
    /// existing grid to 0 on either axis, including width; only construction requires a nonzero
    /// width.
    ///
    /// # Panics
    ///
    /// Panics if `width` is 0.
    #[must_use]
    pub fn new(width: u16, height: u16) -> Self {
        assert!(width > 0, "Grid width must be at least 1, got 0");
        Self {
            width,
            height,
            layers: alloc::vec![Some(LayerBuf::new(width, height))],
            max_layer: 0,
            has_spans: false,
        }
    }

    /// Builds a grid from a rectangular character map, one [`Tile`] per cell.
    ///
    /// `map` is split on `\n`; the grid width is the longest line's display
    /// width (`unicode-width`'s [`UnicodeWidthStr`](unicode_width::UnicodeWidthStr))
    /// and the height is the number of lines. Lines shorter than the widest are
    /// padded with the default tile. `f` maps each character to its tile,
    /// called once per character in reading order.
    ///
    /// Each character is written through [`put_tile`](Self::put_tile) at its own
    /// display column, so a 2-column (wide) character gets the same
    /// [`TileFlags::WIDE_CHAR`]/[`TileFlags::WIDE_CHAR_SPACER`] lead/spacer pair
    /// `put_tile` writes for any other fresh wide tile; the next character in the
    /// line lands one column further along, past the spacer. A wide character in
    /// the map's last column has no room for its spacer and is refused, the same
    /// as any other `put_tile` call in that position.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{Grid, Pos, Style, Tile};
    ///
    /// // A ragged map: the second line is shorter than the first.
    /// let grid = Grid::from_charmap("###\n#.", |c| match c {
    ///     '#' => Tile::new('#', Style::default()),
    ///     _ => Tile::default(),
    /// });
    ///
    /// // Width comes from the longest line; the shorter line is padded with the default
    /// // tile rather than truncating the grid to the shortest line.
    /// assert_eq!((grid.width(), grid.height()), (3, 2));
    /// assert_eq!(grid[Pos::new(0, 0)].glyph(), '#');
    /// assert_eq!(grid[Pos::new(1, 1)].glyph(), ' '); // '.' maps to the default tile
    /// assert_eq!(grid[Pos::new(2, 1)].glyph(), ' '); // padding past the short line's end
    /// ```
    #[must_use]
    pub fn from_charmap<F>(map: &str, mut f: F) -> Self
    where
        F: FnMut(char) -> Tile,
    {
        use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

        let mut width: u16 = 0;
        let mut height: u16 = 0;
        for line in map.lines() {
            let len = u16::try_from(line.width()).unwrap_or(u16::MAX);
            width = width.max(len);
            height = height.saturating_add(1);
        }
        let mut grid = Self::new(width, height);
        for (y, line) in map.lines().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let y = y as u16;
            let mut x: u16 = 0;
            for ch in line.chars() {
                grid.put_tile(0, Pos::new(x, y), f(ch));
                #[allow(clippy::cast_possible_truncation)]
                let ch_width = ch.width().unwrap_or(0) as u16;
                x = x.saturating_add(ch_width);
            }
        }
        grid
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

    /// Returns the grid's dimensions.
    #[must_use]
    pub const fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }

    /// Returns the full grid as a [`Rect`] at the origin.
    ///
    /// Equivalent to `Rect::new(0, 0, width, height)`. Handy for passing the whole grid to
    /// layout helpers or `blit`'s `src_rect`.
    #[must_use]
    pub const fn rect(&self) -> Rect {
        Rect::new(0, 0, self.width, self.height)
    }

    /// Returns the highest layer id that has ever been allocated.
    ///
    /// Always at least 0 (layer 0 is always allocated). This only grows:
    /// clearing a layer ([`clear`](Self::clear)) does not deallocate it, so
    /// the value does not shrink once a higher layer has been written.
    ///
    /// This is the layer id's steady-state cost: every present, diff, and
    /// full-grid iteration walks `0..=max_layer`, skipping unallocated slots
    /// with an O(1) `None` check, so compositing is `O(max_layer)` per cell
    /// rather than `O(topmost opaque layer)`. Writing once to layer 200 and
    /// never touching layers 1-199 means every future frame walks past 199
    /// `None` slots to reach it: cheap per skipped layer, but not free, which
    /// is why low, contiguous ids are preferred for frequently-updated
    /// content.
    #[must_use]
    pub const fn max_layer(&self) -> u8 {
        self.max_layer
    }

    /// Returns the full grapheme cluster stored for the tile at `(x, y)` on
    /// `layer`, if any.
    ///
    /// `Some` only when the tile has [`TileFlags::HAS_EXTRA`] set, i.e. it
    /// was written via [`write_grapheme`](Self::write_grapheme) with a
    /// multi-codepoint EGC (combining marks, ZWJ sequences, etc.). For the
    /// common single-codepoint case, or without the `egc` feature, this is
    /// always `None`; use [`tile`](Self::tile)'s
    /// [`Tile::glyph`](crate::tile::Tile::glyph) and
    /// [`encode_utf8`](char::encode_utf8) to reconstruct the string instead.
    ///
    /// Returns `None` if the layer is unallocated or the coordinates are out
    /// of bounds.
    #[must_use]
    pub fn grapheme(&self, layer: u8, x: u16, y: u16) -> Option<&str> {
        let lb = self.layer(layer)?;
        let pos = to_grixy_pos(Pos::new(x, y));
        let tile = lb.buf.get(pos)?;
        let idx = usize::from(y) * usize::from(self.width) + usize::from(x);
        lb.extra_for(idx, tile)
    }

    /// Iterates all tiles on `layer` with their `(x, y)` coordinates.
    ///
    /// Returns `None` if the layer is unallocated.
    #[must_use]
    pub fn cells(&self, layer: u8) -> Option<Cells<'_>> {
        let lb = self.layer(layer)?;
        Some(Cells {
            iter: lb.buf.as_ref().iter().enumerate(),
            width: usize::from(self.width),
        })
    }

    /// Iterates all tiles on `layer` mutably with their `(x, y)` coordinates.
    ///
    /// Returns `None` if the layer is unallocated, mirroring [`cells`](Self::cells)'s
    /// fallibility. Use [`cells_mut_or_alloc`](Self::cells_mut_or_alloc) to allocate the layer
    /// first instead of failing.
    pub fn cells_mut(&mut self, layer: u8) -> Option<CellsMut<'_>> {
        let width = usize::from(self.width);
        let lb = self.layers.get_mut(usize::from(layer))?.as_mut()?;
        Some(CellsMut {
            iter: lb.buf.as_mut().iter_mut().enumerate(),
            width,
        })
    }

    /// Iterates all tiles on `layer` mutably with their `(x, y)` coordinates, allocating the
    /// layer first if it has not been written to yet.
    ///
    /// Prefer [`cells_mut`](Self::cells_mut) unless an empty layer legitimately needs to exist
    /// after this call returns; unlike that method, this one never fails, at the cost of always
    /// allocating.
    pub fn cells_mut_or_alloc(&mut self, layer: u8) -> CellsMut<'_> {
        let width = usize::from(self.width);
        let lb = self.layer_or_alloc(layer);
        CellsMut {
            iter: lb.buf.as_mut().iter_mut().enumerate(),
            width,
        }
    }

    /// Clears a specific layer, resetting all tiles to the default.
    ///
    /// Does nothing if the layer is unallocated.
    pub fn clear(&mut self, layer: u8) {
        if let Some(lb) = self
            .layers
            .get_mut(usize::from(layer))
            .and_then(Option::as_mut)
        {
            lb.buf.clear();
            lb.extras.clear();
        }
    }

    /// Resizes the grid to `width` × `height` tiles.
    ///
    /// Content within the overlapping region is preserved on all allocated
    /// layers. New cells are initialised to the default tile. Shrinking
    /// discards tiles outside the new bounds.
    pub fn resize(&mut self, width: u16, height: u16) {
        let old_width = usize::from(self.width);
        let new_width = usize::from(width);
        let new_height = usize::from(height);
        self.width = width;
        self.height = height;
        for layer in self.layers.iter_mut().flatten() {
            // The extras side-table is keyed by flat row-major index, which
            // shifts whenever the width changes: remap it in lockstep with
            // `buf.resize` (below) rather than leaving it pointing at stale
            // (or now out-of-bounds) cells.
            if !layer.extras.is_empty() {
                layer.extras = layer
                    .extras
                    .iter()
                    .filter_map(|(&old_idx, s)| {
                        let x = old_idx % old_width;
                        let y = old_idx / old_width;
                        (x < new_width && y < new_height).then(|| (y * new_width + x, s.clone()))
                    })
                    .collect();
            }
            layer.buf.resize(new_width, new_height);
        }
    }

    // ------------------------------------------------------------------
    // Write grapheme: layer 0 only
    // ------------------------------------------------------------------

    /// Writes a grapheme cluster at `(x, y)` on layer 0, enforcing wide-
    /// character invariants.
    ///
    /// This is the canonical way to place content into the grid when the `egc`
    /// feature is enabled. It:
    ///
    /// - Clears any wide character whose primary or spacer cell would be
    ///   overwritten.
    /// - Sets [`TileFlags::WIDE_CHAR`] on the primary cell and places a
    ///   [`TileFlags::WIDE_CHAR_SPACER`] in the adjacent cell for 2-column
    ///   characters.
    /// - Stores multi-codepoint EGCs (combining marks, ZWJ sequences) in the
    ///   layer's EGC side-table (see [`grapheme`](Self::grapheme)), capped at
    ///   8 codepoints total.
    ///
    /// Also does nothing if the grapheme has zero display width, or if a 2-column wide character
    /// would overflow the grid (the last column needs both its own cell and a spacer).
    ///
    /// # Panics
    ///
    /// Panics if the grapheme's display width exceeds [`u16::MAX`]. In
    /// practice this cannot happen: the maximum Unicode grapheme width is 2.
    ///
    /// Only present when the `egc` feature is enabled.
    #[cfg(feature = "egc")]
    pub fn write_grapheme(&mut self, layer: u8, x: u16, y: u16, grapheme: &str, style: Style) {
        use unicode_width::UnicodeWidthStr;

        let width = u16::try_from(grapheme.width()).expect("grapheme width exceeds u16");
        if width == 0 {
            return;
        }

        if x >= self.width || y >= self.height {
            return;
        }

        // Capture dimensions as plain values to avoid borrow conflicts.
        let w = usize::from(self.width);
        let cap = w * usize::from(self.height);
        let idx = usize::from(y) * w + usize::from(x);
        if idx >= cap {
            return;
        }

        // A 2-column char needs a spacer at x+1. If that's out of bounds,
        // silently refuse rather than leaving an orphaned primary cell.
        if width == 2 && x.saturating_add(1) as usize >= w {
            return;
        }

        // Clear any wide-char cell, or any multi-cell span, that would be partially overwritten.
        self.clear_span_overlap(layer, x, y, width);
        self.clear_overlap(layer, x, y, width);

        // Capture width before borrowing self mutably.
        let grid_w = usize::from(self.width);
        let idx = usize::from(y) * grid_w + usize::from(x);

        let lb = self.layer_or_alloc(layer);
        // Build cell content.
        let mut chars = grapheme.chars();
        let first = chars.next().unwrap_or(' ');
        let has_extra = chars.next().is_some();
        let flags = if width == 2 {
            TileFlags::WIDE_CHAR
        } else {
            TileFlags::empty()
        };
        let flags = if has_extra {
            flags | TileFlags::HAS_EXTRA
        } else {
            flags
        };

        lb.buf.as_mut()[idx].glyph = first;
        lb.buf.as_mut()[idx].style = style;
        lb.buf.as_mut()[idx].flags = flags;
        // `width` here is the full grapheme's display width (1 or 2), not just `first`'s: more
        // accurate than recomputing from the primary codepoint alone, and exactly what the
        // terminal renderer needs to advance the cursor after printing this cell.
        #[allow(clippy::cast_possible_truncation)]
        {
            lb.buf.as_mut()[idx].width = width as u8;
        }
        // A fresh glyph write replaces the cell's out-of-line data outright rather than merging
        // with it: a tint belongs to the artwork that was drawn here, not to the cell, so
        // overwriting the glyph drops it. `Grid::set_tint` is the follow-up that puts one back.
        if has_extra {
            lb.extras.insert(
                idx,
                TileExtra {
                    grapheme: Some(Arc::from(cap_grapheme(grapheme))),
                    tint: Tint::None,
                },
            );
        } else {
            lb.extras.remove(&idx);
        }

        // Place spacer for wide characters.
        if width == 2 {
            let spacer_idx = usize::from(y) * grid_w + usize::from(x + 1);
            if spacer_idx < cap {
                let spacer = &mut lb.buf.as_mut()[spacer_idx];
                spacer.glyph = ' ';
                spacer.style = style;
                spacer.width = 0;
                spacer.flags = TileFlags::WIDE_CHAR_SPACER;
                lb.extras.remove(&spacer_idx);
            }
        }
    }

    /// Clears wide-character cells that would be partially overwritten by a
    /// write starting at `(x, y)` spanning `width` columns.
    ///
    /// `clear_span_overlap` is the multi-cell-span analogue. Not gated behind `egc`: `write_grapheme`
    /// is `egc`-only, but [`put_tile`](Self::put_tile) writes a wide-character pair on every
    /// feature combination (see its own doc comment), so a write that can land inside either kind
    /// of multi-cell structure calls both regardless of `egc`.
    pub(super) fn clear_overlap(&mut self, layer: u8, x: u16, y: u16, width: u16) {
        let w = usize::from(self.width);
        let cap = w * usize::from(self.height);
        // An unallocated layer has never written a wide-character cell, so there is nothing to
        // clear: return before `layer_or_alloc` would allocate one just to find that (retroglyph#1012).
        let Some(lb) = self
            .layers
            .get_mut(usize::from(layer))
            .and_then(Option::as_mut)
        else {
            return;
        };
        for cx in x..x.saturating_add(width) {
            let idx = usize::from(y) * w + usize::from(cx);
            if idx >= cap {
                continue;
            }
            // flags is Copy, so reading through the shared ref is fine.
            let flags = lb.buf.as_ref()[idx].flags;

            if flags.contains(TileFlags::WIDE_CHAR_SPACER) && cx > 0 {
                let pidx = usize::from(y) * w + usize::from(cx - 1);
                if pidx < cap {
                    lb.buf.as_mut()[pidx].reset();
                    lb.extras.remove(&pidx);
                }
            }

            if flags.contains(TileFlags::WIDE_CHAR) {
                let sidx = usize::from(y) * w + usize::from(cx + 1);
                if sidx < cap {
                    lb.buf.as_mut()[sidx].reset();
                    lb.extras.remove(&sidx);
                }
            }
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
    #[should_panic(expected = "Grid width must be at least 1")]
    fn test_grid_new_zero_width_panics() {
        let _ = Grid::new(0, 5);
    }

    #[test]
    fn test_grid_new_zero_height_is_allowed() {
        let grid = Grid::new(5, 0);
        assert_eq!(grid.width(), 5);
        assert_eq!(grid.height(), 0);
    }

    #[test]
    #[should_panic(expected = "Grid width must be at least 1")]
    fn test_grid_new_zero_by_zero_panics() {
        let _ = Grid::new(0, 0);
    }

    #[test]
    fn test_grid_resize_to_zero_width_is_allowed() {
        let mut grid = Grid::new(5, 5);
        grid.resize(0, 5);
        assert_eq!(grid.width(), 0);
        assert_eq!(grid.height(), 5);
    }

    #[test]
    fn test_grid_resize_to_zero_height_is_allowed() {
        let mut grid = Grid::new(5, 5);
        grid.resize(5, 0);
        assert_eq!(grid.width(), 5);
        assert_eq!(grid.height(), 0);
    }

    #[test]
    fn test_grid_resize_to_zero_by_zero_is_allowed() {
        let mut grid = Grid::new(5, 5);
        grid.resize(0, 0);
        assert_eq!(grid.width(), 0);
        assert_eq!(grid.height(), 0);
    }

    #[test]
    fn test_grid_resize_expand() {
        let mut grid = Grid::new(3, 3);
        grid.put_tile(0, (1, 1), Tile::default().with_glyph('X'));
        grid.resize(6, 6);
        assert_eq!(grid.width(), 6);
        assert_eq!(grid.height(), 6);
        assert_eq!(grid[Pos::new(1, 1)].glyph(), 'X'); // preserved
        assert_eq!(grid[Pos::new(5, 5)].glyph(), ' '); // new cells default
    }

    #[test]
    fn test_grid_resize_shrink() {
        let mut grid = Grid::new(10, 10);
        grid.put_tile(0, (1, 1), Tile::default().with_glyph('A'));
        grid.resize(5, 5);
        assert_eq!(grid.width(), 5);
        assert_eq!(grid.height(), 5);
        assert_eq!(grid[Pos::new(1, 1)].glyph(), 'A'); // still in bounds, preserved
    }

    #[test]
    fn test_grid_resize_preserves_overlap() {
        let mut grid = Grid::new(4, 4);
        grid.put_tile(0, (0, 0), Tile::default().with_glyph('@'));
        grid.put_tile(0, (3, 3), Tile::default().with_glyph('X'));
        grid.resize(3, 3); // shrink: (3,3) falls outside
        assert_eq!(grid[Pos::new(0, 0)].glyph(), '@');
        assert_eq!(grid[Pos::new(2, 2)].glyph(), ' '); // was default, still default
    }

    #[test]
    fn test_grid_cells_count() {
        let grid = Grid::new(4, 3);
        assert_eq!(grid.cells(0).unwrap().count(), 12);
    }

    #[test]
    fn test_grid_cells_coordinates() {
        use alloc::vec;
        use alloc::vec::Vec;

        let grid = Grid::new(3, 2);
        let coords: Vec<(u16, u16)> = grid.cells(0).unwrap().map(|(x, y, _)| (x, y)).collect();
        assert_eq!(
            coords,
            vec![(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1),]
        );
    }

    #[test]
    fn test_grid_cells_mut() {
        use crate::color::Style;
        let mut grid = Grid::new(2, 2);
        for (x, y, tile) in grid.cells_mut(0).unwrap() {
            #[allow(clippy::cast_possible_truncation)]
            let idx = (y * 2 + x) as u8;
            *tile = Tile::new(char::from(b'A' + idx), Style::default());
        }
        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'A');
        assert_eq!(grid[Pos::new(1, 0)].glyph(), 'B');
        assert_eq!(grid[Pos::new(0, 1)].glyph(), 'C');
        assert_eq!(grid[Pos::new(1, 1)].glyph(), 'D');
    }

    #[test]
    fn test_grid_cells_mut_unallocated_layer_is_none() {
        // Mirrors `cells`: an unwritten layer is `None`, not an empty iterator, and critically
        // it must not allocate the layer as a side effect of checking.
        let mut grid = Grid::new(2, 2);
        assert!(grid.cells_mut(3).is_none());
        assert!(grid.tile(3, (0, 0)).is_none());
    }

    #[test]
    fn test_grid_cells_mut_or_alloc_allocates_an_unwritten_layer() {
        use crate::color::Style;
        let mut grid = Grid::new(2, 2);
        assert!(grid.cells_mut(2).is_none());
        for (_, _, tile) in grid.cells_mut_or_alloc(2) {
            *tile = Tile::new('x', Style::default());
        }
        // Now allocated: the non-allocating accessor finds it too.
        assert!(grid.cells_mut(2).is_some());
        assert_eq!(grid.tile(2, (0, 0)).unwrap().glyph(), 'x');
    }

    // --- Extra grapheme text (EGC side-table) ---
    #[cfg(feature = "egc")]
    #[test]
    fn test_grid_write_grapheme_stores_and_reads_extra() {
        let mut g = Grid::new(5, 5);
        g.write_grapheme(0, 1, 1, "e\u{0301}", Style::default());
        assert_eq!(g[Pos::new(1, 1)].glyph, 'e');
        assert_eq!(g.grapheme(0, 1, 1), Some("e\u{0301}"));

        // Single-codepoint writes never populate the side-table.
        g.write_grapheme(0, 2, 2, "a", Style::default());
        assert_eq!(g.grapheme(0, 2, 2), None);
    }

    #[cfg(feature = "egc")]
    #[test]
    fn write_grapheme_x_past_width_wraps_onto_the_next_row() {
        let mut grid = Grid::new(10, 10);
        grid.write_grapheme(0, 12, 0, "A", Style::default()); // x = 12 on a 10-wide grid
        assert_eq!(grid[Pos::new(2, 0)].glyph(), ' ');
        assert_eq!(grid[Pos::new(2, 1)].glyph(), ' ');
    }

    #[cfg(feature = "egc")]
    #[test]
    fn write_grapheme_y_past_height_is_refused() {
        let mut grid = Grid::new(10, 10);
        grid.write_grapheme(0, 0, 12, "A", Style::default()); // y = 12 on a 10-tall grid
        for y in 0..10 {
            assert_eq!(grid[Pos::new(0, y)].glyph(), ' ');
        }
    }

    #[cfg(feature = "egc")]
    #[test]
    fn write_grapheme_in_bounds_still_writes() {
        let mut grid = Grid::new(10, 10);
        grid.write_grapheme(0, 2, 1, "A", Style::default());
        assert_eq!(grid[Pos::new(2, 1)].glyph(), 'A');
    }

    #[cfg(feature = "egc")]
    #[test]
    fn test_grid_overwrite_clears_extra() {
        let mut g = Grid::new(5, 5);
        g.write_grapheme(0, 0, 0, "e\u{0301}", Style::default());
        assert_eq!(g.grapheme(0, 0, 0), Some("e\u{0301}"));

        // A plain `put` (or a later single-codepoint `write_grapheme`) must
        // drop the stale side-table entry, not just leave it unreachable.
        g.put_tile(0, (0, 0), Tile::new('X', Style::default()));
        assert_eq!(g.grapheme(0, 0, 0), None);
        assert!(!g[Pos::new(0, 0)].flags().contains(TileFlags::HAS_EXTRA));
    }

    #[cfg(feature = "egc")]
    #[test]
    fn test_grid_resize_remaps_extras_to_new_stride() {
        let mut g = Grid::new(4, 4);
        g.write_grapheme(0, 3, 1, "e\u{0301}", Style::default());
        assert_eq!(g.grapheme(0, 3, 1), Some("e\u{0301}"));

        // Widening changes the row stride, so the flat index for (3, 1)
        // changes even though the cell itself is preserved.
        g.resize(8, 4);
        assert_eq!(g[Pos::new(3, 1)].glyph, 'e');
        assert_eq!(g.grapheme(0, 3, 1), Some("e\u{0301}"));
        // No ghost entry landed on some other cell at the old flat index.
        assert_eq!(g.grapheme(0, 7, 0), None);

        // Shrinking past the cell drops its extras entry along with the tile.
        g.resize(2, 4);
        assert_eq!(g.grapheme(0, 3, 1), None);
    }

    #[test]
    fn clear_drops_every_tint_on_the_layer() {
        let mut g = Grid::new(4, 4);
        g.set_tint(0, 1, 1, Tint::multiply(1, 2, 3));
        g.set_tint(1, 1, 1, Tint::multiply(4, 5, 6));

        g.clear(0);
        assert_eq!(g.tint(0, 1, 1), Tint::None);
        assert_eq!(g.tint(1, 1, 1), Tint::multiply(4, 5, 6));
    }

    #[cfg(feature = "egc")]
    #[test]
    fn resize_remaps_a_tint_to_the_new_stride() {
        let mut g = Grid::new(4, 4);
        g.write_grapheme(0, 3, 1, "@", Style::default());
        g.set_tint(0, 3, 1, Tint::mix(200, 100, 50, 128));

        // Widening changes the row stride, so (3, 1)'s flat index moves.
        g.resize(8, 4);
        assert_eq!(g.tint(0, 3, 1), Tint::mix(200, 100, 50, 128));
        // No ghost entry landed on whatever cell now holds the old flat index.
        assert_eq!(g.tint(0, 7, 0), Tint::None);

        // Shrinking past the cell drops its entry with the tile.
        g.resize(2, 4);
        assert_eq!(g.tint(0, 3, 1), Tint::None);
    }

    #[test]
    fn from_charmap_lone_wide_char() {
        use crate::color::Style;

        // A single wide char needs 2 columns of width; there is no narrower char after it to
        // clobber its spacer, but sizing must give it the room in the first place.
        let g = Grid::from_charmap("\u{4e2d}", |c| Tile::new(c, Style::default()));
        assert_eq!(g.width(), 2);
        assert_eq!(g.height(), 1);
        assert_eq!(g.tile(0, (0, 0)).unwrap().glyph(), '\u{4e2d}');
        assert!(
            g.tile(0, (0, 0))
                .unwrap()
                .flags()
                .contains(TileFlags::WIDE_CHAR)
        );
        assert_eq!(g.tile(0, (1, 0)).unwrap().glyph(), ' ');
        assert!(
            g.tile(0, (1, 0))
                .unwrap()
                .flags()
                .contains(TileFlags::WIDE_CHAR_SPACER)
        );
    }

    #[test]
    fn from_charmap_wide_char_mid_line() {
        use crate::color::Style;

        // The wide char's spacer occupies column 1; the following 'x' must land at column 2,
        // not column 1 where it would clobber the spacer and clear the wide lead.
        let g = Grid::from_charmap("\u{4e2d}x", |c| Tile::new(c, Style::default()));
        assert_eq!(g.width(), 3);
        assert_eq!(g.tile(0, (0, 0)).unwrap().glyph(), '\u{4e2d}');
        assert!(
            g.tile(0, (0, 0))
                .unwrap()
                .flags()
                .contains(TileFlags::WIDE_CHAR)
        );
        assert_eq!(g.tile(0, (1, 0)).unwrap().glyph(), ' ');
        assert!(
            g.tile(0, (1, 0))
                .unwrap()
                .flags()
                .contains(TileFlags::WIDE_CHAR_SPACER)
        );
        assert_eq!(g.tile(0, (2, 0)).unwrap().glyph(), 'x');
        assert_eq!(g.tile(0, (2, 0)).unwrap().flags(), TileFlags::empty());
    }

    #[test]
    fn from_charmap_wide_char_at_end_of_line_fits_exactly() {
        use crate::color::Style;

        // The wide char is the last character of the widest (and only) line, so `from_charmap`
        // must size the grid with room for both its lead and its spacer: unlike a caller passing
        // an already-fixed width to `put_tile` directly, there is no way for this to hit
        // `put_tile`'s last-column refusal, since the sizing pass and the write pass measure the
        // same display width.
        let g = Grid::from_charmap("a\u{4e2d}", |c| Tile::new(c, Style::default()));
        assert_eq!(g.width(), 3);
        assert_eq!(g.height(), 1);
        assert_eq!(g.tile(0, (0, 0)).unwrap().glyph(), 'a');
        assert_eq!(g.tile(0, (1, 0)).unwrap().glyph(), '\u{4e2d}');
        assert!(
            g.tile(0, (1, 0))
                .unwrap()
                .flags()
                .contains(TileFlags::WIDE_CHAR)
        );
        assert_eq!(g.tile(0, (2, 0)).unwrap().glyph(), ' ');
        assert!(
            g.tile(0, (2, 0))
                .unwrap()
                .flags()
                .contains(TileFlags::WIDE_CHAR_SPACER)
        );
    }

    #[test]
    fn from_charmap_ragged_map_mixing_wide_and_narrow_rows() {
        use crate::color::Style;

        // Row 0 is all narrow ("ab", width 2); row 1 is one wide char ("\u{4e2d}", width 2). Both
        // rows have the same display width, so the grid is 2 columns wide and neither row needs
        // padding, but row 1's single char must still claim both columns via the spacer.
        let g = Grid::from_charmap("ab\n\u{4e2d}", |c| Tile::new(c, Style::default()));
        assert_eq!(g.width(), 2);
        assert_eq!(g.height(), 2);
        assert_eq!(g.tile(0, (0, 0)).unwrap().glyph(), 'a');
        assert_eq!(g.tile(0, (1, 0)).unwrap().glyph(), 'b');
        assert_eq!(g.tile(0, (0, 1)).unwrap().glyph(), '\u{4e2d}');
        assert!(
            g.tile(0, (0, 1))
                .unwrap()
                .flags()
                .contains(TileFlags::WIDE_CHAR)
        );
        assert_eq!(g.tile(0, (1, 1)).unwrap().glyph(), ' ');
        assert!(
            g.tile(0, (1, 1))
                .unwrap()
                .flags()
                .contains(TileFlags::WIDE_CHAR_SPACER)
        );
    }

    #[cfg(all(test, feature = "egc"))]
    mod egc_proptests {
        use super::*;
        use crate::color::Style;
        use proptest::prelude::*;

        const W: u16 = 8;
        const H: u16 = 4;

        /// Narrow, wide (CJK), combining-mark, and wide-emoji graphemes.
        const GRAPHEMES: &[&str] = &["a", "\u{4e2d}", "e\u{0301}", "\u{1f600}"];

        /// Every `WIDE_CHAR` has its spacer to the right, every `WIDE_CHAR_SPACER`
        /// has its lead to the left, and no cell is both.
        fn assert_wide_invariants(grid: &Grid) {
            for y in 0..grid.height() {
                for x in 0..grid.width() {
                    let flags = grid[Pos::new(x, y)].flags();
                    let lead = flags.contains(TileFlags::WIDE_CHAR);
                    let spacer = flags.contains(TileFlags::WIDE_CHAR_SPACER);

                    assert!(
                        !(lead && spacer),
                        "cell ({x}, {y}) is both wide lead and spacer"
                    );

                    if lead {
                        assert!(x + 1 < grid.width(), "wide lead at ({x}, {y}) has no room");
                        assert!(
                            grid[Pos::new(x + 1, y)]
                                .flags()
                                .contains(TileFlags::WIDE_CHAR_SPACER),
                            "wide lead at ({x}, {y}) is missing its spacer"
                        );
                    }

                    if spacer {
                        assert!(x > 0, "orphan spacer at ({x}, {y}) (no cell to the left)");
                        assert!(
                            grid[Pos::new(x - 1, y)]
                                .flags()
                                .contains(TileFlags::WIDE_CHAR),
                            "orphan spacer at ({x}, {y}) (left cell is not a wide lead)"
                        );
                    }
                }
            }
        }

        proptest! {
            #[test]
            fn wide_char_bookkeeping_never_desyncs(
                ops in prop::collection::vec(
                    (0u16..W, 0u16..H, 0usize..GRAPHEMES.len()),
                    0..64,
                ),
            ) {
                let mut grid = Grid::new(W, H);
                for (x, y, gi) in ops {
                    grid.write_grapheme(0, x, y, GRAPHEMES[gi], Style::default());
                    // The invariant must hold after every single write, not just
                    // at the end: an intermediate orphan would be a real bug.
                    assert_wide_invariants(&grid);
                }
            }
        }
    }
}
