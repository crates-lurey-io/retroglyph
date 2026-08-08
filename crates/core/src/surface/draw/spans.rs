//! Multi-cell sprite spans: [`put_span`](Surface::put_span) and its uniform/pixel-offset twins.

use crate::color::Style;
use crate::grid::{HasSize, Offset, Pos, Size};
#[cfg(not(feature = "egc"))]
use crate::tile::Tile;
#[cfg(not(feature = "egc"))]
use unicode_width::UnicodeWidthChar;

use super::Surface;

impl Surface<'_> {
    /// Writes a multi-cell span at `pos` on this surface's layer in `style`: one piece of
    /// artwork occupying a block of cells rather than one, the [`Surface`] twin of
    /// [`Grid::write_span`](crate::grid::Grid::write_span).
    ///
    /// `rows` holds one string per row of the footprint. Its first character is the **anchor**
    /// glyph, which a pixel backend looks up in its sprite cache; the rest are the span's **text
    /// fallback**, printed by cell backends and skipped by pixel backends. Any `AsRef<str>` row
    /// works, so a literal footprint (`&["[==]", "|__|"]`) and a computed one (`&Vec<String>`)
    /// both pass without a borrowing pass over the rows; for the uniform case, see
    /// [`put_span_uniform`](Self::put_span_uniform).
    ///
    /// See [`Grid::write_span`](crate::grid::Grid::write_span) for the full write semantics, and
    /// [`Grid::span_owner`](crate::grid::Grid::span_owner) to hit-test the whole footprint.
    ///
    /// # `style` applies to the text fallback, not to the sprite
    ///
    /// A sprite is composited from its own pixels. [`style.fg`](Style::fg) does not tint it;
    /// `style.bg` is still painted behind it, so it shows through wherever the sprite is
    /// transparent. Recoloring a shared sprite per cell is therefore not possible: draw a
    /// variant of the artwork instead, which is the usual tileset idiom.
    ///
    /// `style` is not dead on such a cell, because the same span drawn by a *cell* backend
    /// renders the text fallback in it. The consequence is that `fg` reads very differently
    /// depending on the backend, and that a glyph missing from the sprite cache silently falls
    /// back to a font glyph that *is* `fg`-colored, which looks a lot like a tint working.
    ///
    /// # Returns
    ///
    /// `Some(())` once the whole span is written, or `None` having written nothing at all when
    /// `rows` is empty or ragged, either axis exceeds 255 cells, or the footprint does not fit
    /// entirely within this surface's own clip (not just the grid) at `pos`. The surface has
    /// strictly more ways to refuse a span than
    /// [`Grid::write_span`](crate::grid::Grid::write_span) does, so a sprite that did not draw is
    /// answered here rather than in the backend.
    pub fn put_span<S: AsRef<str>>(
        &mut self,
        pos: impl Into<Pos>,
        rows: &[S],
        style: Style,
    ) -> Option<()> {
        let pos = pos.into();
        let (x, y) = self.shift(pos.x, pos.y)?;
        let cols = rows.first()?.as_ref().chars().count();
        let w = u16::try_from(cols).ok()?;
        let h = u16::try_from(rows.len()).ok()?;
        if !self.span_fits(Pos::new(x, y), w, h) {
            return None;
        }
        self.grid.write_span(self.layer, x, y, rows, style)?;
        // The anchor only: a pixel backend draws the whole footprint from that one cell, so the
        // covered cells have no sprite of their own to recolour.
        self.apply_tint(x, y);
        Some(())
    }

    /// Writes a `size` multi-cell span at `pos` on this surface's layer in `style`: `anchor` in
    /// the anchor cell, `fill` in every other cell of the footprint, the [`Surface`] twin of
    /// [`Grid::write_span_uniform`](crate::grid::Grid::write_span_uniform).
    ///
    /// The uniform case of [`put_span`](Self::put_span), and what a sheet-driven renderer usually
    /// wants: one sprite, chosen at runtime, with the cells it covers blanked so nothing shows
    /// through its transparent pixels. `fill` is the text fallback a *cell* backend prints for
    /// those covered cells, so `' '` blanks them and a visible character keeps the footprint
    /// legible in a terminal.
    ///
    /// `style` reads exactly as it does for [`put_span`](Self::put_span): it applies to the text
    /// fallback, never to the sprite.
    ///
    /// # Returns
    ///
    /// `Some(())` once the whole span is written, or `None` having written nothing at all when
    /// either axis of `size` is `0` or exceeds 255 cells, or the footprint does not fit entirely
    /// within this surface's own clip at `pos`.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() {
    /// # fn run() -> Option<()> {
    /// use retroglyph_core::color::Style;
    /// use retroglyph_core::grid::{Grid, Rect};
    /// use retroglyph_core::surface::Surface;
    ///
    /// let mut grid = Grid::new(8, 4);
    /// let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 8, 4), 0);
    ///
    /// // A 16x16 sprite over a 2x1 block of 8x16 cells, anchored at a runtime glyph.
    /// let anchor = '\u{E000}';
    /// surface.put_span_uniform((1, 1), (2, 1), anchor, ' ', Style::default())?;
    /// # Some(())
    /// # }
    /// # run().unwrap();
    /// # }
    /// ```
    pub fn put_span_uniform(
        &mut self,
        pos: impl Into<Pos>,
        size: impl Into<Size>,
        anchor: char,
        fill: char,
        style: Style,
    ) -> Option<()> {
        let pos = pos.into();
        let (x, y) = self.shift(pos.x, pos.y)?;
        let pos = Pos::new(x, y);
        let size = size.into();
        if !self.span_fits(pos, size.width(), size.height()) {
            return None;
        }
        self.grid
            .write_span_uniform(self.layer, pos, size, anchor, fill, style)?;
        self.apply_tint(pos.x, pos.y);
        Some(())
    }

    /// `true` if a `w` x `h` footprint at `pos` lies entirely within this surface's clip.
    ///
    /// A span is all-or-nothing rather than clipped like the per-cell writes, because a
    /// footprint half outside the clip would reserve cells the caller does not own.
    fn span_fits(&self, pos: Pos, w: u16, h: u16) -> bool {
        pos.x >= self.clip.left()
            && pos.y >= self.clip.top()
            && pos.x.saturating_add(w) <= self.clip.right()
            && pos.y.saturating_add(h) <= self.clip.bottom()
    }

    /// Place `ch` at `pos` with a sub-cell pixel `offset`, in `style`.
    ///
    /// Sub-cell offsets are visual only: they do not affect grid logic or hit-testing.
    /// Backends that cannot represent pixel offsets (e.g. `CrosstermBackend`) ignore them. A
    /// no-op if `pos` is outside this surface's clip.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::color::Style;
    /// use retroglyph_core::grid::{Grid, Offset, Pos, Rect};
    /// use retroglyph_core::surface::Surface;
    ///
    /// let mut grid = Grid::new(4, 4);
    /// let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 4, 4), 0);
    ///
    /// // A large offset still lands the glyph in cell (1, 1): the offset is a pixel nudge
    /// // for a pixel backend, never a coordinate shift.
    /// surface.put_offset((1, 1), Offset::new(12, -12), 'X', Style::default());
    /// // Outside the surface's clip: silently dropped, matching `put`.
    /// surface.put_offset((10, 10), Offset::default(), 'X', Style::default());
    ///
    /// assert_eq!(grid[Pos::new(1, 1)].glyph(), 'X');
    /// ```
    pub fn put_offset(
        &mut self,
        pos: impl Into<Pos>,
        offset: impl Into<Offset>,
        ch: char,
        style: Style,
    ) {
        let pos = pos.into();
        let Some((x, y)) = self.shift(pos.x, pos.y) else {
            return;
        };
        let offset = offset.into();
        #[cfg(feature = "egc")]
        let wrote = {
            let mut buf = [0u8; 4];
            let s = ch.encode_utf8(&mut buf);
            self.put_grapheme_at(x, y, s, style)
        };
        #[cfg(not(feature = "egc"))]
        let wrote = {
            if self.wide_spacer_fits(x, y, ch.width().unwrap_or(1)) {
                let tile = Tile::new(ch, style);
                let wrote = self.grid.put_tile(self.layer, (x, y), tile).is_some();
                if wrote {
                    self.apply_tint(x, y);
                }
                wrote
            } else {
                false
            }
        };
        // A refused write (e.g. a wide glyph whose spacer falls outside the clip, or
        // `put_tile` declining an out-of-grid/unallocatable-layer write) leaves `(x, y)`
        // holding whatever tile a *different* draw call put there. Setting the offset on it
        // would move a cell this call never touched, so bail out before `tile_mut` below.
        if !wrote {
            return;
        }
        // The offset is a pixel nudge on the tile the write above just landed, not part of
        // `write_grapheme`'s contract (it has no offset parameter): set it directly via
        // `tile_mut` rather than widening `Grid`'s public write API for a `Surface`-only concern.
        if let Some(tile) = self.grid.tile_mut(self.layer, (x, y)) {
            tile.dx = offset.dx;
            tile.dy = offset.dy;
        }
    }
}
