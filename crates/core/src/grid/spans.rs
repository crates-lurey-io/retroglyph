//! `Grid`'s multi-cell span API: [`Grid::write_span`], [`Grid::write_span_uniform`],
//! [`Grid::span_owner`], and [`Grid::clear_span`], plus the anchor/covered-cell bookkeeping they
//! share.

use super::{Grid, Pos, Size, to_grixy_pos};
use crate::color::Style;
use crate::tile::{Tile, TileFlags};
use alloc::collections::BTreeSet;
use alloc::vec::Vec;
use grixy::ops::GridRead;
use ixy::HasSize;

/// A span's largest representable extent on either axis (see `Tile::span_w`/`Tile::span_h`),
/// and so the widest band [`Grid::repair_spans_after_resize`] ever needs to scan near a shrunk
/// edge: no anchor further than this from the edge can have a stale footprint reaching it.
const MAX_SPAN_EXTENT: u16 = u8::MAX as u16;

impl Grid {
    /// Writes a multi-cell span at `(x, y)` on `layer`: one piece of artwork occupying a block of
    /// cells rather than one.
    ///
    /// `rows` holds one string per row of the footprint, so the span is `rows.len()` cells tall
    /// and `rows[0]`'s character count wide, and every row must be that same width. Any
    /// `AsRef<str>` row works, so a literal footprint (`&["[==]", "|__|"]`) and a computed one
    /// (`&Vec<String>`) both pass without a borrowing pass over the rows. The first
    /// character goes to the **anchor** cell at `(x, y)` with [`TileFlags::SPAN_ANCHOR`]; each
    /// remaining character goes to its own cell with [`TileFlags::SPAN_COVERED`]. `style` applies
    /// to every cell.
    ///
    /// # Text fallback
    ///
    /// The covered cells keep real glyphs, which is what lets one call render correctly on every
    /// backend with no capability check:
    ///
    /// - A **cell backend** (`Headless`, `retroglyph-crossterm`, `retroglyph-terminal`) ignores
    ///   [`TileFlags::SPAN_COVERED`] and prints all of them, so `["[==]", "|__|"]` reads as a
    ///   small piece of ASCII art.
    /// - A **pixel backend** (`retroglyph-software`, `retroglyph-gl`) looks the anchor glyph up in
    ///   its sprite cache, draws that one sprite across the whole footprint, and skips every
    ///   covered cell's glyph.
    ///
    /// This is the deliberate difference from [`TileFlags::WIDE_CHAR_SPACER`], which every
    /// backend skips.
    ///
    /// Any existing span or wide character the footprint would partially overwrite is cleared
    /// first, in full, as [`write_grapheme`](Self::write_grapheme) does for its own 1- or 2-cell
    /// write.
    ///
    /// For the common sprite case (one runtime-chosen anchor glyph, blanks in every covered
    /// cell), [`write_span_uniform`](Self::write_span_uniform) says the same thing without
    /// building the rows.
    ///
    /// # Returns
    ///
    /// `Some(())` once the whole span is written, or `None` having written nothing at all when
    /// `rows` is empty, its first row is empty, its rows differ in width, either axis exceeds 255
    /// cells, or the footprint would not fit in the grid at `(x, y)`.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() {
    /// # fn run() -> Option<()> {
    /// use retroglyph_core::{Grid, Pos, Style};
    ///
    /// let mut grid = Grid::new(8, 4);
    /// grid.write_span(0, 1, 1, &["[==]", "|__|"], Style::default())?;
    ///
    /// assert_eq!(grid.tile(0, Pos::new(1, 1))?.span(), (4, 2));
    /// // Covered cells keep their fallback glyphs, and name their anchor.
    /// assert_eq!(grid.tile(0, Pos::new(4, 2))?.glyph(), '|');
    /// assert_eq!(grid.span_owner(0, 4, 2), Some(Pos::new(1, 1)));
    /// # Some(())
    /// # }
    /// # run().unwrap();
    /// # }
    /// ```
    pub fn write_span<S: AsRef<str>>(
        &mut self,
        layer: u8,
        x: u16,
        y: u16,
        rows: &[S],
        style: Style,
    ) -> Option<()> {
        let cols = rows.first()?.as_ref().chars().count();
        if cols == 0 || rows.iter().any(|r| r.as_ref().chars().count() != cols) {
            return None;
        }
        // `Tile` stores a span's dimensions in one byte each (see `Tile::span_w`), so a span
        // wider or taller than 255 cells is not representable.
        let footprint = (u8::try_from(cols).ok()?, u8::try_from(rows.len()).ok()?);

        self.write_span_cells(
            layer,
            Pos::new(x, y),
            footprint,
            style,
            rows.iter().map(|row| row.as_ref().chars()),
        )
    }

    /// Writes a `size` multi-cell span at `pos` on `layer`: `anchor` in the anchor cell, `fill`
    /// in every other cell of the footprint.
    ///
    /// The uniform case of [`write_span`](Self::write_span), and the shape a sheet-driven
    /// renderer usually wants: one sprite, chosen at runtime, with the cells it covers blanked so
    /// nothing shows through its transparent pixels. Spelling that as an array of blank rows
    /// carries no information and, for a computed anchor, has to be allocated per draw.
    ///
    /// `fill` is what a *cell* backend prints for the covered cells (a pixel backend skips them
    /// and draws the sprite instead), so it is the span's text fallback: `' '` blanks them, and a
    /// visible character keeps the footprint legible in a terminal. See
    /// [`write_span`](Self::write_span) for the full write semantics.
    ///
    /// # Returns
    ///
    /// `Some(())` once the whole span is written, or `None` having written nothing at all when
    /// either axis of `size` is `0` or exceeds 255 cells, or the footprint would not fit in the
    /// grid at `pos`.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() {
    /// # fn run() -> Option<()> {
    /// use retroglyph_core::{Grid, Pos, Style};
    ///
    /// let mut grid = Grid::new(8, 4);
    /// let anchor = '\u{E000}'; // chosen at runtime from a tilesheet
    /// grid.write_span_uniform(0, (1, 1), (2, 2), anchor, ' ', Style::default())?;
    ///
    /// assert_eq!(grid.tile(0, Pos::new(1, 1))?.span(), (2, 2));
    /// assert_eq!(grid.span_owner(0, 2, 2), Some(Pos::new(1, 1)));
    /// # Some(())
    /// # }
    /// # run().unwrap();
    /// # }
    /// ```
    pub fn write_span_uniform(
        &mut self,
        layer: u8,
        pos: impl Into<Pos>,
        size: impl Into<Size>,
        anchor: char,
        fill: char,
        style: Style,
    ) -> Option<()> {
        let size = size.into();
        // `Tile` stores a span's dimensions in one byte each (see `Tile::span_w`), so a span
        // wider or taller than 255 cells is not representable.
        let footprint = (
            u8::try_from(size.width()).ok()?,
            u8::try_from(size.height()).ok()?,
        );
        if footprint.0 == 0 || footprint.1 == 0 {
            return None;
        }

        let rows = (0..footprint.1).map(move |row| {
            (0..footprint.0).map(move |col| if (row, col) == (0, 0) { anchor } else { fill })
        });
        self.write_span_cells(layer, pos.into(), footprint, style, rows)
    }

    /// Writes a `footprint` (`w`, `h`) span at `pos` on `layer`, taking its glyphs row by row.
    ///
    /// The shared body of [`write_span`](Self::write_span) and
    /// [`write_span_uniform`](Self::write_span_uniform): both have already narrowed the footprint
    /// to a `u8` per axis, so all that is left is the grid-fit check and the write itself.
    /// `rows` must yield exactly `footprint.1` rows of exactly `footprint.0` glyphs.
    fn write_span_cells<R: Iterator<Item = char>>(
        &mut self,
        layer: u8,
        pos: Pos,
        footprint: (u8, u8),
        style: Style,
        rows: impl Iterator<Item = R>,
    ) -> Option<()> {
        let (footprint_w, footprint_h) = footprint;
        let (x, y) = (pos.x, pos.y);

        let grid_w = usize::from(self.width);
        if usize::from(x) + usize::from(footprint_w) > grid_w
            || usize::from(y) + usize::from(footprint_h) > usize::from(self.height)
        {
            return None;
        }

        // Clear anything the footprint would partially overwrite. Every rejection above happens
        // first, so a refused write can never have already destroyed the caller's content.
        // Neither call is gated on `egc`: `put_tile` writes a `WIDE_CHAR`/`WIDE_CHAR_SPACER` pair
        // on every feature combination, so a span that can land inside one has to clean it up
        // regardless of `egc` (same reasoning as `fill_region`'s matching pair of calls).
        for row in 0..footprint_h {
            let cy = y + u16::from(row);
            self.clear_span_overlap(layer, x, cy, u16::from(footprint_w));
            self.clear_overlap(layer, x, cy, u16::from(footprint_w));
        }

        self.has_spans = true;
        let lb = self.layer_or_alloc(layer);
        for (row, line) in rows.enumerate() {
            for (col, ch) in line.enumerate() {
                let idx = (usize::from(y) + row) * grid_w + usize::from(x) + col;
                let mut tile = Tile::new(ch, style);
                if row == 0 && col == 0 {
                    tile.flags = TileFlags::SPAN_ANCHOR;
                    tile.span_w = footprint_w;
                    tile.span_h = footprint_h;
                } else {
                    // Both fit in a `u8`: they are strictly less than the footprint, which was
                    // already narrowed to one above.
                    #[allow(clippy::cast_possible_truncation)]
                    {
                        tile.flags = TileFlags::SPAN_COVERED;
                        tile.span_w = col as u8;
                        tile.span_h = row as u8;
                    }
                }
                lb.buf.as_mut()[idx] = tile;
                lb.extras.remove(&idx);
            }
        }
        Some(())
    }

    /// The anchor of the multi-cell span occupying `(x, y)` on `layer`, or `None` when the cell
    /// belongs to no span or is out of bounds.
    ///
    /// An anchor cell reports itself, so every cell of one span answers with the same position
    /// and hit-testing multi-cell artwork is a single comparison:
    ///
    /// ```
    /// # fn main() {
    /// # fn run() -> Option<()> {
    /// # use retroglyph_core::{Grid, Pos, Style};
    /// # let mut grid = Grid::new(8, 4);
    /// grid.write_span(0, 2, 1, &["[==]", "|__|"], Style::default())?;
    /// let chest = Pos::new(2, 1);
    /// // Any of the eight cells counts as standing on the chest.
    /// assert_eq!(grid.span_owner(0, 2, 1), Some(chest));
    /// assert_eq!(grid.span_owner(0, 5, 2), Some(chest));
    /// assert_eq!(grid.span_owner(0, 6, 2), None);
    /// # Some(())
    /// # }
    /// # run().unwrap();
    /// # }
    /// ```
    ///
    /// O(1): a covered tile stores its offset back to the anchor (see [`Tile::span_offset`]), so
    /// this is a lookup and a subtraction, not a scan.
    #[must_use]
    pub fn span_owner(&self, layer: u8, x: u16, y: u16) -> Option<Pos> {
        self.span_anchor_at(layer, x, y)
    }

    /// Clears the whole multi-cell span that `(x, y)` on `layer` belongs to, anchor included,
    /// resetting every one of its cells to the default (empty) tile.
    ///
    /// Works from any cell of the span, so it pairs with [`span_owner`](Self::span_owner): hit-test
    /// a cell, then clear the artwork it belongs to. Does nothing if the cell is not part of a
    /// span, is out of bounds, or the layer is unallocated.
    pub fn clear_span(&mut self, layer: u8, x: u16, y: u16) {
        if let Some(anchor) = self.span_anchor_at(layer, x, y) {
            self.reset_span_at(layer, anchor);
        }
    }

    /// The anchor of the span `(x, y)` belongs to, treating an anchor cell as its own anchor.
    fn span_anchor_at(&self, layer: u8, x: u16, y: u16) -> Option<Pos> {
        let tile = self.layer(layer)?.buf.get(to_grixy_pos(Pos::new(x, y)))?;
        if tile.flags.contains(TileFlags::SPAN_ANCHOR) {
            return Some(Pos::new(x, y));
        }
        let (dx, dy) = tile.span_offset()?;
        Some(Pos::new(x.checked_sub(dx)?, y.checked_sub(dy)?))
    }

    /// Resets every cell of the span anchored at `anchor` on `layer`. No-op if that cell is not
    /// a [`TileFlags::SPAN_ANCHOR`], or the layer is unallocated.
    fn reset_span_at(&mut self, layer: u8, anchor: Pos) {
        let w = usize::from(self.width);
        let h = usize::from(self.height);
        let Some(lb) = self
            .layers
            .get_mut(usize::from(layer))
            .and_then(Option::as_mut)
        else {
            return;
        };
        let anchor_idx = usize::from(anchor.y) * w + usize::from(anchor.x);
        let Some(anchor_tile) = lb.buf.as_ref().get(anchor_idx).copied() else {
            return;
        };
        if !anchor_tile.flags.contains(TileFlags::SPAN_ANCHOR) {
            return;
        }
        for row in 0..usize::from(anchor_tile.span_h) {
            let cy = usize::from(anchor.y) + row;
            if cy >= h {
                break;
            }
            for col in 0..usize::from(anchor_tile.span_w) {
                let cx = usize::from(anchor.x) + col;
                if cx >= w {
                    break;
                }
                let idx = cy * w + cx;
                lb.buf.as_mut()[idx].reset();
                lb.extras.remove(&idx);
            }
        }
    }

    /// Repairs [`TileFlags::SPAN_ANCHOR`] footprints made stale by a shrinking
    /// [`resize`](Self::resize).
    ///
    /// `resize` keeps a grid's top-left corner, and an anchor is always the top-left of its own
    /// footprint, so shrinking can never orphan a *covered* cell from its anchor. It can still
    /// leave the anchor's declared `(span_w, span_h)` running past the new edge. Half a span is
    /// not representable (the same reasoning [`blit`](Self::blit) documents for clipping one), so
    /// any anchor whose footprint no longer fits has its whole span cleared via
    /// [`reset_span_at`](Self::reset_span_at) instead of being left to claim cells that do not
    /// exist.
    ///
    /// `width_shrank`/`height_shrank` say which axis actually got smaller; `resize` only calls
    /// this when at least one is true, so a growing resize never reaches here. Each shrunk axis
    /// only scans a band up to [`u8::MAX`] cells deep from its new edge -- a span's largest
    /// representable extent (see `Tile::span_w`) -- rather than the whole grid, so an anchor far
    /// from the shrunk edge is never visited and the cost tracks the resize, not the grid's total
    /// size.
    pub(super) fn repair_spans_after_resize(&mut self, width_shrank: bool, height_shrank: bool) {
        if !self.has_spans {
            return;
        }
        let w = self.width;
        let h = self.height;
        if width_shrank {
            let x_start = w.saturating_sub(MAX_SPAN_EXTENT);
            self.repair_span_region(x_start, w, 0, h);
        }
        if height_shrank {
            let y_start = h.saturating_sub(MAX_SPAN_EXTENT);
            self.repair_span_region(0, w, y_start, h);
        }
    }

    /// Resets every [`TileFlags::SPAN_ANCHOR`] in `x_start..x_end` × `y_start..y_end`, on every
    /// allocated layer, whose stored footprint no longer fits within the grid's current bounds.
    ///
    /// The two calls in [`repair_spans_after_resize`](Self::repair_spans_after_resize) can overlap
    /// in their shared corner when both axes shrink; revisiting that corner just re-checks a few
    /// already-repaired anchors; `reset_span_at` is a no-op on a cell that is no longer an anchor.
    fn repair_span_region(&mut self, x_start: u16, x_end: u16, y_start: u16, y_end: u16) {
        let w = self.width;
        let h = self.height;
        for layer_id in 0..self.layers.len() {
            let mut anchors: Vec<Pos> = Vec::new();
            if let Some(lb) = self.layers[layer_id].as_ref() {
                for y in y_start..y_end {
                    for x in x_start..x_end {
                        // `x_end`/`y_end` are always `w`/`h` (see the two call sites in
                        // `repair_spans_after_resize`), so `idx` is always in bounds: no `.get`
                        // needed, and no untestable out-of-bounds branch to carry.
                        let idx = usize::from(y) * usize::from(w) + usize::from(x);
                        let tile = &lb.buf.as_ref()[idx];
                        if !tile.flags.contains(TileFlags::SPAN_ANCHOR) {
                            continue;
                        }
                        let (span_w, span_h) = tile.span();
                        if usize::from(x) + usize::from(span_w) > usize::from(w)
                            || usize::from(y) + usize::from(span_h) > usize::from(h)
                        {
                            anchors.push(Pos::new(x, y));
                        }
                    }
                }
            } else {
                continue;
            }
            #[allow(clippy::cast_possible_truncation)]
            let layer = layer_id as u8;
            for anchor in anchors {
                self.reset_span_at(layer, anchor);
            }
        }
    }

    /// Clears every multi-cell span that a `width`-cell write starting at `(x, y)` on `layer`
    /// would partially overwrite.
    ///
    /// The span analogue of [`clear_overlap`](Self::clear_overlap), and the reason every ordinary
    /// write path calls it: overwriting one cell of a span would otherwise leave an anchor
    /// claiming cells it no longer owns, or a covered cell pointing at an anchor that is gone.
    ///
    /// Returns immediately on a grid that has never had a span written to it, which is what keeps
    /// this off the cost of an ordinary [`put_tile`](Self::put_tile) (see
    /// [`has_spans`](Self::has_spans)).
    pub(super) fn clear_span_overlap(&mut self, layer: u8, x: u16, y: u16, width: u16) {
        if !self.has_spans {
            return;
        }
        // Collect first: resetting a span mutates cells this scan is still reading. Overlapping
        // writes touch at most a handful of spans, so the linear `contains` beats a set.
        let mut anchors: Vec<Pos> = Vec::new();
        let Some(lb) = self.layer(layer) else {
            return;
        };
        for cx in x..x.saturating_add(width) {
            let Some(tile) = lb.buf.get(to_grixy_pos(Pos::new(cx, y))) else {
                continue;
            };
            let anchor = if tile.flags.contains(TileFlags::SPAN_ANCHOR) {
                Pos::new(cx, y)
            } else if let Some((dx, dy)) = tile.span_offset() {
                match (cx.checked_sub(dx), y.checked_sub(dy)) {
                    (Some(ax), Some(ay)) => Pos::new(ax, ay),
                    _ => continue,
                }
            } else {
                continue;
            };
            if !anchors.contains(&anchor) {
                anchors.push(anchor);
            }
        }
        for anchor in anchors {
            self.reset_span_at(layer, anchor);
        }
    }

    /// Clears every multi-cell span that a `width` x `height` write starting at `(x, y)` on
    /// `layer` would partially overwrite.
    ///
    /// The region analogue of [`clear_span_overlap`](Self::clear_span_overlap), for callers like
    /// [`fill_region`](super::Grid::fill_region) that would otherwise call it once per row: a span
    /// spanning several of those rows would then be collected, and fully reset, once per row it
    /// occupies. This scans the whole region once instead, deduplicating anchors in a
    /// `BTreeSet<(u16, u16)>` (`Pos` has no `Ord`) so each span is reset exactly once regardless
    /// of how many rows or columns of the region it overlaps.
    ///
    /// Returns immediately on a grid that has never had a span written to it, same as
    /// [`clear_span_overlap`](Self::clear_span_overlap).
    pub(super) fn clear_span_overlap_rect(
        &mut self,
        layer: u8,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
    ) {
        if !self.has_spans {
            return;
        }
        // Collect first, same reasoning as `clear_span_overlap`: resetting a span mutates cells
        // this scan is still reading.
        let mut anchors: BTreeSet<(u16, u16)> = BTreeSet::new();
        let Some(lb) = self.layer(layer) else {
            return;
        };
        for cy in y..y.saturating_add(height) {
            for cx in x..x.saturating_add(width) {
                let Some(tile) = lb.buf.get(to_grixy_pos(Pos::new(cx, cy))) else {
                    continue;
                };
                let anchor = if tile.flags.contains(TileFlags::SPAN_ANCHOR) {
                    (cx, cy)
                } else if let Some((dx, dy)) = tile.span_offset() {
                    match (cx.checked_sub(dx), cy.checked_sub(dy)) {
                        (Some(ax), Some(ay)) => (ax, ay),
                        _ => continue,
                    }
                } else {
                    continue;
                };
                anchors.insert(anchor);
            }
        }
        for (ax, ay) in anchors {
            self.reset_span_at(layer, Pos::new(ax, ay));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Multi-cell spans (retroglyph#412) ────────────────────────────────
    /// The anchor owns the footprint; every other cell names the anchor and keeps its own glyph.
    #[test]
    fn write_span_marks_anchor_and_covered_cells() {
        let mut grid = Grid::new(4, 4);
        grid.write_span(0, 1, 1, &["C=", "[]"], Style::default())
            .expect("2x2 span fits in a 4x4 grid");

        let anchor = grid.tile(0, (1, 1)).unwrap();
        assert!(anchor.flags().contains(TileFlags::SPAN_ANCHOR));
        assert_eq!(anchor.span(), (2, 2));
        assert_eq!(anchor.span_offset(), None);
        assert_eq!(anchor.glyph(), 'C');

        for (x, y, glyph, offset) in [
            (2, 1, '=', (1, 0)),
            (1, 2, '[', (0, 1)),
            (2, 2, ']', (1, 1)),
        ] {
            let tile = grid.tile(0, (x, y)).unwrap();
            assert!(
                tile.flags().contains(TileFlags::SPAN_COVERED),
                "({x}, {y}) should be covered"
            );
            assert_eq!(tile.glyph(), glyph, "({x}, {y}) keeps its fallback glyph");
            assert_eq!(tile.span_offset(), Some(offset));
            // A covered cell is inside a footprint, it does not own one.
            assert_eq!(tile.span(), (1, 1));
        }
    }

    /// `clear_overlap` runs regardless of `egc` (see its own doc comment): `write_span_cells`
    /// gated it behind the feature until retroglyph#1014, so a wide pair written by `put_tile`
    /// (which is not itself `egc`-gated) kept a stale `WIDE_CHAR` flag after a span write
    /// partially overwrote it with `egc` off.
    #[test]
    fn write_span_clears_a_wide_char_it_partially_overwrites() {
        let mut grid = Grid::new(4, 1);
        grid.put_tile(0, (0, 0), Tile::new('\u{4e2d}', Style::default()));
        assert!(
            grid.tile(0, (0, 0))
                .unwrap()
                .flags()
                .contains(TileFlags::WIDE_CHAR)
        );

        grid.write_span(0, 1, 0, &["ab"], Style::default()).unwrap();

        assert!(
            !grid
                .tile(0, (0, 0))
                .unwrap()
                .flags()
                .contains(TileFlags::WIDE_CHAR)
        );
    }

    /// The whole point of `SPAN_COVERED` differing from `WIDE_CHAR_SPACER`: cell backends read
    /// these glyphs, so they must survive the write intact.
    #[test]
    fn write_span_keeps_the_fallback_glyphs_readable() {
        let mut grid = Grid::new(4, 4);
        grid.write_span(0, 0, 0, &["C=", "[]"], Style::default())
            .unwrap();
        let read = |x, y| grid.tile(0, (x, y)).unwrap().glyph();
        assert_eq!(
            [read(0, 0), read(1, 0), read(0, 1), read(1, 1)],
            ['C', '=', '[', ']']
        );
    }

    #[test]
    fn span_owner_reports_the_anchor_from_every_cell_of_the_span() {
        let mut grid = Grid::new(6, 6);
        grid.write_span(0, 2, 3, &["AB", "CD", "EF"], Style::default())
            .unwrap();

        // Every cell of the footprint, the anchor included, answers with the same position, so
        // hit-testing is one comparison.
        for (x, y) in [(2, 3), (3, 3), (2, 4), (3, 4), (2, 5), (3, 5)] {
            assert_eq!(grid.span_owner(0, x, y), Some(Pos::new(2, 3)), "({x}, {y})");
        }
        // A free cell, an out-of-bounds one, and one on an unallocated layer belong to no span.
        assert_eq!(grid.span_owner(0, 0, 0), None);
        assert_eq!(grid.span_owner(0, 99, 99), None);
        assert_eq!(grid.span_owner(3, 3, 3), None);
    }

    #[test]
    fn write_span_rejects_malformed_input_without_writing() {
        let mut grid = Grid::new(4, 4);
        assert_eq!(
            grid.write_span(0, 0, 0, &[] as &[&str], Style::default()),
            None
        );
        assert_eq!(grid.write_span(0, 0, 0, &[""], Style::default()), None);
        // Ragged rows.
        assert_eq!(
            grid.write_span(0, 0, 0, &["ab", "c"], Style::default()),
            None
        );
        // Too wide / too tall for the grid at this origin.
        assert_eq!(grid.write_span(0, 3, 0, &["ab"], Style::default()), None);
        assert_eq!(
            grid.write_span(0, 0, 3, &["a", "b"], Style::default()),
            None
        );
        // Nothing was written by any of the above.
        for y in 0..4 {
            for x in 0..4 {
                assert!(
                    grid[Pos::new(x, y)].is_empty(),
                    "({x}, {y}) should be untouched"
                );
            }
        }
    }

    #[test]
    fn write_span_takes_any_as_ref_str_row() {
        use alloc::string::String;
        use alloc::vec::Vec;

        let mut grid = Grid::new(4, 4);
        // A footprint computed at runtime: owned rows, no borrowing pass over them.
        let rows: Vec<String> = (0..2)
            .map(|row| {
                (0..2)
                    .map(|col| if (row, col) == (0, 0) { 'C' } else { ' ' })
                    .collect()
            })
            .collect();

        assert_eq!(grid.write_span(0, 0, 0, &rows, Style::default()), Some(()));
        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'C');
        assert_eq!(grid[Pos::new(0, 0)].span(), (2, 2));
    }

    #[test]
    fn write_span_uniform_writes_the_anchor_once_and_fills_the_rest() {
        let mut grid = Grid::new(4, 4);
        assert_eq!(
            grid.write_span_uniform(0, (1, 1), (2, 2), 'C', '.', Style::default()),
            Some(())
        );

        assert_eq!(grid[Pos::new(1, 1)].glyph(), 'C');
        assert_eq!(grid[Pos::new(1, 1)].span(), (2, 2));
        for (x, y) in [(2, 1), (1, 2), (2, 2)] {
            assert_eq!(grid[Pos::new(x, y)].glyph(), '.', "({x}, {y})");
            assert_eq!(grid.span_owner(0, x, y), Some(Pos::new(1, 1)));
        }
    }

    #[test]
    fn write_span_uniform_matches_the_equivalent_write_span() {
        let mut uniform = Grid::new(4, 4);
        uniform
            .write_span_uniform(0, (0, 0), (3, 2), 'C', ' ', Style::default())
            .unwrap();

        let mut rows = Grid::new(4, 4);
        rows.write_span(0, 0, 0, &["C  ", "   "], Style::default())
            .unwrap();

        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(
                    uniform[Pos::new(x, y)],
                    rows[Pos::new(x, y)],
                    "({x}, {y}) differs"
                );
            }
        }
    }

    #[test]
    fn write_span_uniform_rejects_a_degenerate_or_oversized_footprint() {
        let mut grid = Grid::new(4, 4);
        let style = Style::default();

        assert_eq!(
            grid.write_span_uniform(0, (0, 0), (0, 2), 'C', ' ', style),
            None
        );
        assert_eq!(
            grid.write_span_uniform(0, (0, 0), (2, 0), 'C', ' ', style),
            None
        );
        // A span's dimensions are one byte each.
        assert_eq!(
            grid.write_span_uniform(0, (0, 0), (256, 1), 'C', ' ', style),
            None
        );
        // Does not fit the grid at this origin.
        assert_eq!(
            grid.write_span_uniform(0, (3, 0), (2, 1), 'C', ' ', style),
            None
        );

        for y in 0..4 {
            for x in 0..4 {
                assert!(
                    grid[Pos::new(x, y)].is_empty(),
                    "({x}, {y}) should be untouched"
                );
            }
        }
    }

    #[test]
    fn writing_into_a_covered_cell_clears_the_whole_span() {
        let mut grid = Grid::new(4, 4);
        grid.write_span(0, 0, 0, &["C=", "[]"], Style::default())
            .unwrap();
        grid.put_tile(0, (1, 1), Tile::new('x', Style::default()));

        assert_eq!(grid[Pos::new(1, 1)].glyph(), 'x');
        for (x, y) in [(0, 0), (1, 0), (0, 1)] {
            let tile = grid[Pos::new(x, y)];
            assert!(tile.is_empty(), "({x}, {y}) should have been cleared");
            assert_eq!(tile.flags(), TileFlags::EMPTY);
        }
    }

    #[test]
    fn writing_over_the_anchor_clears_the_whole_span() {
        let mut grid = Grid::new(4, 4);
        grid.write_span(0, 0, 0, &["C=", "[]"], Style::default())
            .unwrap();
        grid.put_tile(0, (0, 0), Tile::new('x', Style::default()));

        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'x');
        assert_eq!(grid[Pos::new(0, 0)].span(), (1, 1));
        for (x, y) in [(1, 0), (0, 1), (1, 1)] {
            assert!(
                grid[Pos::new(x, y)].is_empty(),
                "({x}, {y}) should be cleared"
            );
        }
    }

    #[test]
    fn overlapping_spans_erase_the_old_one_entirely() {
        let mut grid = Grid::new(4, 4);
        grid.write_span(0, 0, 0, &["AB", "CD"], Style::default())
            .unwrap();
        // Overlaps the first span's bottom-right cell only; all four of its cells must go.
        grid.write_span(0, 1, 1, &["EF", "GH"], Style::default())
            .unwrap();

        assert!(grid[Pos::new(0, 0)].is_empty());
        assert!(grid[Pos::new(1, 0)].is_empty());
        assert!(grid[Pos::new(0, 1)].is_empty());
        assert_eq!(grid[Pos::new(1, 1)].glyph(), 'E');
        assert_eq!(grid.span_owner(0, 2, 2), Some(Pos::new(1, 1)));
    }

    #[test]
    fn clear_span_works_from_any_cell_of_the_span() {
        let mut grid = Grid::new(4, 4);
        for from in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            grid.write_span(0, 0, 0, &["C=", "[]"], Style::default())
                .unwrap();
            grid.clear_span(0, from.0, from.1);
            for y in 0..2 {
                for x in 0..2 {
                    assert!(
                        grid[Pos::new(x, y)].is_empty(),
                        "clearing from {from:?}: ({x}, {y})"
                    );
                }
            }
        }
        // A cell that is not part of a span is left alone.
        grid.put_tile(0, (3, 3), Tile::new('z', Style::default()));
        grid.clear_span(0, 3, 3);
        assert_eq!(grid[Pos::new(3, 3)].glyph(), 'z');
    }

    /// `clear_span_overlap_rect` scans the whole region once (retroglyph#1020), rather than
    /// calling `clear_span_overlap` once per row: a span several rows tall must still come out
    /// fully reset, not just its slice under the first row scanned.
    #[test]
    fn clear_span_overlap_rect_clears_a_span_spanning_every_row_it_touches() {
        let mut grid = Grid::new(6, 6);
        grid.write_span(0, 1, 1, &["AB", "CD", "EF", "GH"], Style::default())
            .expect("2x4 span fits in a 6x6 grid");

        // A single call covering all four rows the span occupies, same as `fill_region` now
        // makes once per call instead of once per row.
        grid.clear_span_overlap_rect(0, 0, 1, 6, 4);

        for y in 1..5 {
            for x in 1..3 {
                let tile = grid[Pos::new(x, y)];
                assert!(tile.is_empty(), "({x}, {y}) should have been reset");
            }
        }
    }

    /// `has_spans` is grid-wide, not per layer (see its own doc comment), so a span written to
    /// layer 0 is enough to take `clear_span_overlap_rect` past its fast path even when called
    /// against a layer that has never been allocated. That layer must return with no allocation
    /// and no panic, not implicitly create one just to find it empty.
    #[test]
    fn clear_span_overlap_rect_on_an_unallocated_layer_is_a_no_op() {
        let mut grid = Grid::new(4, 4);
        grid.write_span(0, 0, 0, &["C=", "[]"], Style::default())
            .unwrap();

        grid.clear_span_overlap_rect(1, 0, 0, 4, 4);

        assert!(grid.tile(1, (0, 0)).is_none());
    }

    /// A direct `clear_span_overlap_rect` call, unlike `fill_region`, is not clipped to the grid
    /// before it scans: `width`/`height` reaching past the grid's own edges must skip the
    /// out-of-bounds cells rather than panicking, while still resetting the in-bounds portion of
    /// a span the in-bounds part of the scan touches.
    #[test]
    fn clear_span_overlap_rect_skips_out_of_bounds_cells_without_panicking() {
        let mut grid = Grid::new(4, 4);
        grid.write_span(0, 2, 2, &["C=", "[]"], Style::default())
            .unwrap();

        grid.clear_span_overlap_rect(0, 2, 2, 10, 10);

        for y in 2..4 {
            for x in 2..4 {
                assert!(grid[Pos::new(x, y)].is_empty(), "({x}, {y})");
            }
        }
    }

    /// A `SPAN_COVERED` cell whose stored offset is larger than its own position never comes out
    /// of a real write (an anchor is always in-bounds and at or before every cell it covers), but
    /// a corrupted or adversarial layer should not panic subtracting past zero. Hand-crafts that
    /// cell directly (bypassing `write_span`) to exercise the `checked_sub` guard.
    #[test]
    fn clear_span_overlap_rect_skips_a_covered_cell_whose_offset_underflows() {
        let mut grid = Grid::new(4, 4);
        // A real span elsewhere sets `has_spans`, so the scan below actually runs instead of
        // short-circuiting.
        grid.write_span(0, 3, 3, &["Z"], Style::default()).unwrap();

        let mut bogus = Tile::new('x', Style::default());
        bogus.flags = TileFlags::SPAN_COVERED;
        bogus.span_w = 1;
        bogus.span_h = 0;
        grid[Pos::new(0, 0)] = bogus;

        grid.clear_span_overlap_rect(0, 0, 0, 1, 1);

        // No panic, and the bogus cell is left alone: there is no real anchor at (-1, 0) to
        // reset it against.
        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'x');
    }

    #[test]
    fn spans_are_layer_scoped() {
        let mut grid = Grid::new(4, 4);
        grid.write_span(1, 0, 0, &["C=", "[]"], Style::default())
            .unwrap();
        assert_eq!(grid.span_owner(1, 1, 1), Some(Pos::new(0, 0)));
        // Layer 0 knows nothing about layer 1's span, and writing there leaves it intact.
        assert_eq!(grid.span_owner(0, 1, 1), None);
        assert_eq!(grid.span_owner(0, 0, 0), None);
        grid.put_tile(0, (1, 1), Tile::new('x', Style::default()));
        assert_eq!(grid.span_owner(1, 1, 1), Some(Pos::new(0, 0)));
    }

    #[test]
    fn clear_region_clears_a_span_it_only_partly_covers() {
        let mut grid = Grid::new(4, 4);
        grid.write_span(0, 0, 0, &["C=", "[]"], Style::default())
            .unwrap();
        // Only the anchor cell is inside the region, but the whole span must go.
        grid.put_tile(0, (0, 0), Tile::default());
        for y in 0..2 {
            for x in 0..2 {
                assert!(grid[Pos::new(x, y)].is_empty(), "({x}, {y})");
            }
        }
    }

    #[test]
    fn resize_narrower_clears_a_span_anchor_whose_footprint_no_longer_fits() {
        let mut grid = Grid::new(4, 2);
        grid.write_span(0, 0, 0, &["ab", "cd"], Style::default())
            .unwrap();

        // Drops the span's right-hand column: a 2-wide footprint cannot survive on a 1-wide grid,
        // so the whole span must go rather than leave the anchor claiming a footprint that no
        // longer fits.
        grid.resize(1, 2);
        assert!(grid.tile(0, (0, 0)).unwrap().is_empty());
        assert_eq!(grid.tile(0, (0, 0)).unwrap().span(), (1, 1));
    }

    #[test]
    fn resize_shorter_clears_a_span_anchor_whose_footprint_no_longer_fits() {
        let mut grid = Grid::new(2, 4);
        grid.write_span(0, 0, 0, &["a", "c"], Style::default())
            .unwrap();

        // Same shape of bug on the other axis: drops the span's bottom row.
        grid.resize(2, 1);
        assert!(grid.tile(0, (0, 0)).unwrap().is_empty());
        assert_eq!(grid.tile(0, (0, 0)).unwrap().span(), (1, 1));
    }

    #[test]
    fn resize_narrower_skips_unallocated_layers_between_allocated_ones() {
        // Layer 1 stays `None`: writing to layer 2 grows the layer table past it without
        // allocating it. The repair scan must walk straight past that gap layer instead of
        // panicking or mistaking it for one with a stale anchor.
        let mut grid = Grid::new(4, 2);
        grid.write_span(0, 0, 0, &["ab", "cd"], Style::default())
            .unwrap();
        grid.put_tile(2, (0, 0), Tile::new('z', Style::default()));
        assert!(grid.tile(1, (0, 0)).is_none());

        grid.resize(1, 2);
        assert!(grid.tile(0, (0, 0)).unwrap().is_empty());
        assert_eq!(grid.tile(0, (0, 0)).unwrap().span(), (1, 1));
        // Untouched by the repair scan on an unrelated layer.
        assert_eq!(grid.tile(2, (0, 0)).unwrap().glyph(), 'z');
    }

    #[test]
    fn resize_wider_leaves_a_span_anchor_untouched() {
        // A growing resize never removes any of a footprint's cells, so the anchor must survive
        // exactly as written.
        let mut grid = Grid::new(4, 4);
        grid.write_span(0, 0, 0, &["C=", "[]"], Style::default())
            .unwrap();
        grid.resize(8, 8);
        assert_eq!(grid.tile(0, (0, 0)).unwrap().span(), (2, 2));
        assert_eq!(grid.span_owner(0, 1, 1), Some(Pos::new(0, 0)));
    }
}
