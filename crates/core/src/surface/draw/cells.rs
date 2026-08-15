//! Single-cell and whole-region writes: [`put`](Surface::put) and its rect/grid-scale twins.

use crate::color::{Style, Tint};
use crate::grid::{Grid, HasSize, Pos, Rect};
use crate::tile::Tile;
use unicode_width::UnicodeWidthChar;

use super::Surface;

impl Surface<'_> {
    /// The whole-rect counterpart to [`shift`](Self::shift): translates a local `rect` (same
    /// convention as `fill_rect`/`clear_region`, and as `shift`'s own `x`/`y`) into the absolute
    /// grid rect it covers under this surface's own clip, or `None` if none of it lands.
    ///
    /// Only handles `origin_offset == (0, 0)` (true of every surface except one produced by
    /// [`clip_translate`](Self::clip_translate)): with any other offset `shift` still refuses a
    /// negative post-offset coordinate per cell, which a single rect-wide translation can't
    /// reproduce without re-deriving per-cell bounds, so callers fall back to `shift` itself
    /// instead.
    fn local_rect_to_absolute(&self, rect: Rect) -> Option<Rect> {
        if self.origin_offset != (0, 0) {
            return None;
        }
        let local = rect.intersect(self.area.to_rect());
        if local.is_empty() {
            return None;
        }
        let abs = Rect::new(
            self.area.left() + local.left(),
            self.area.top() + local.top(),
            local.width(),
            local.height(),
        )
        .intersect(self.clip);
        (!abs.is_empty()).then_some(abs)
    }

    /// Clips `rect` (in the same coordinate space as `fill_rect`/`clear_region`'s own `rect`
    /// argument) to what can possibly land on this surface: `(0, 0)..(area.width, area.height)`
    /// shifted by `origin_offset`, mirroring the subtraction [`shift`](Self::shift) applies per
    /// cell.
    ///
    /// Both methods' per-cell fallback loop runs this first so the loop is bounded to at most
    /// `area.width * area.height` cells regardless of how much larger `rect` is, rather than
    /// iterating `rect`'s full width * height (up to ~4.3 billion cells for a `u16`-sized rect)
    /// and relying on a per-cell check to skip what doesn't land.
    ///
    /// The intersection itself is [`Rect::intersect`], not hand-rolled per-field arithmetic,
    /// widened to `i64` because `origin_offset` can push the shifted area below `0` or above
    /// `u16::MAX`, neither of which `Rect<u16>` can represent; the result is narrowed back to
    /// `u16` once [`intersect`](ixy::Rect::intersect) has already bounded it within `rect`'s own
    /// (already-`u16`) extent.
    fn clip_local_rect(&self, rect: Rect) -> Rect {
        let bounds = ixy::Rect::<i64>::new(
            i64::from(self.origin_offset.0),
            i64::from(self.origin_offset.1),
            i64::from(self.area.width()),
            i64::from(self.area.height()),
        );
        let rect = ixy::Rect::<i64>::new(
            i64::from(rect.left()),
            i64::from(rect.top()),
            i64::from(rect.width()),
            i64::from(rect.height()),
        )
        .intersect(bounds);
        // `intersect` only ever narrows `rect`'s own fields, which started out as `u16`, so
        // these conversions never fail.
        let left = u16::try_from(rect.left()).unwrap_or(u16::MAX);
        let top = u16::try_from(rect.top()).unwrap_or(u16::MAX);
        let width = u16::try_from(rect.width()).unwrap_or(u16::MAX);
        let height = u16::try_from(rect.height()).unwrap_or(u16::MAX);
        Rect::new(left, top, width, height)
    }

    /// Place `ch` at `pos` in `style`. A no-op if `pos` is outside this surface's clip.
    ///
    /// If a pixel backend resolves `ch` to a sprite, that sprite is composited from its own
    /// pixels: [`style.fg`](Style::fg) does not tint it, and `style.bg` shows through only where
    /// the sprite is transparent. See [`put_span`](Self::put_span).
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::color::Style;
    /// use retroglyph_core::grid::{Grid, Pos, Rect};
    /// use retroglyph_core::surface::Surface;
    ///
    /// let mut grid = Grid::new(4, 4);
    /// let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 4, 4), 0);
    ///
    /// surface.put((1, 1), 'X', Style::default());
    /// // Outside the surface's clip: silently dropped, not a panic.
    /// surface.put((10, 10), 'X', Style::default());
    ///
    /// assert_eq!(grid[Pos::new(1, 1)].glyph(), 'X');
    /// ```
    pub fn put(&mut self, pos: impl Into<Pos>, ch: char, style: Style) {
        let pos = pos.into();
        let Some((x, y)) = self.shift(pos.x, pos.y) else {
            return;
        };
        self.put_char_at(x, y, ch, style);
    }

    /// [`put`](Self::put), in coordinates relative to this surface's own area origin, where a
    /// negative coordinate is expressible and simply falls outside (a no-op, matching `put`'s
    /// out-of-bounds behavior). A coordinate that stays non-negative but exceeds `u16::MAX` after
    /// this surface's translate offset is subtracted is dropped the same way: it addresses a cell
    /// this surface's `u16` grid space cannot name.
    ///
    /// Scrolling/camera code (e.g. a viewport over a wider world) computes positions in a
    /// coordinate space that can go negative relative to the viewport, which [`Pos`] (backed by
    /// `u16`) cannot even express. `put_signed` takes that arithmetic directly, so a caller no
    /// longer clip-tests by hand before calling `put`.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::color::Style;
    /// use retroglyph_core::grid::{Grid, Pos, Rect};
    /// use retroglyph_core::surface::Surface;
    ///
    /// let mut grid = Grid::new(4, 4);
    /// let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 4, 4), 0);
    ///
    /// // Negative in either axis: outside this surface's area, silently dropped.
    /// surface.put_signed((-1, 1), 'X', Style::default());
    /// // Non-negative and within bounds: lands like `put`.
    /// surface.put_signed((1, 1), 'X', Style::default());
    ///
    /// assert_eq!(grid[Pos::new(1, 1)].glyph(), 'X');
    /// assert_eq!(grid[Pos::new(0, 1)].glyph(), ' ');
    /// ```
    pub fn put_signed(&mut self, pos: (i32, i32), ch: char, style: Style) {
        let (x, y) = pos;
        let Some((x, y)) = self.shift_signed(x, y) else {
            return;
        };
        self.put_char_at(x, y, ch, style);
    }

    /// Fill `rect` (clipped to this surface's own clip) with `ch` in `style`.
    ///
    /// `rect` is local to this surface's own [`area`](Self::area): `(0, 0)` is `area`'s own
    /// top-left, not the grid's, the same convention [`clear_region`](Self::clear_region) and
    /// [`print_aligned`](Self::print_aligned) use for their own `rect` (not absolute grid
    /// coordinates, the convention [`clip`](Self::clip)/[`scope`](Self::scope) use).
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::color::Style;
    /// use retroglyph_core::grid::{Grid, Pos, Rect};
    /// use retroglyph_core::surface::Surface;
    ///
    /// let mut grid = Grid::new(4, 4);
    /// let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 4, 4), 0);
    ///
    /// // `rect` extends well past the grid on both axes; only the cells inside the
    /// // surface's own clip are touched, the rest is silently clipped.
    /// surface.fill_rect(Rect::new(2, 2, 10, 10), '#', Style::default());
    ///
    /// assert_eq!(grid[Pos::new(3, 3)].glyph(), '#');
    /// assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
    /// ```
    pub fn fill_rect(&mut self, rect: Rect, ch: char, style: Style) {
        // The batch path below writes a plain `Tile::new(ch, style)` per cell, which matches
        // `put`'s own per-cell write only when there's no tint to apply and `ch` is a
        // single-column glyph: `fill_rect` itself refuses (no-op) any `tile.width() != 1` (see
        // its own doc comment), so this check just avoids paying for a delegation that would
        // silently do nothing. Anything else (tinted surface, zero/double-width glyph) falls back
        // to the per-cell loop, unchanged from before this method had a fast path.
        let single_width = UnicodeWidthChar::width(ch) == Some(1);

        if self.tint == Tint::None
            && single_width
            && let Some(abs) = self.local_rect_to_absolute(rect)
        {
            self.grid.fill_rect(self.layer, abs, Tile::new(ch, style));
            return;
        }

        let rect = self.clip_local_rect(rect);
        for pos in rect {
            self.put(pos, ch, style);
        }
    }

    /// Stamps `grid`'s layer 0 onto this surface's own layer, with its top-left cell at `(x, y)`
    /// (local to this surface's area, matching [`put`](Self::put)'s convention), clipped to this
    /// surface's clip.
    ///
    /// Always reads `grid`'s layer 0, regardless of which layer this surface itself is currently
    /// writing to: `grid` is typically a standalone buffer composed elsewhere (e.g.
    /// `BoxStyle::render`'s output, or `retroglyph-ui`' `join_h`/`join_v`), and per their own
    /// docs those only ever populate layer 0. Reading this surface's own layer off `grid` instead
    /// (what [`Grid::blit`]'s single `layer` parameter would do if called directly) finds nothing
    /// there whenever this surface isn't on layer 0, and the copy silently does nothing.
    ///
    /// Unlike a single-cell [`put`](Self::put), a write that starts outside this surface's clip is
    /// not necessarily dropped whole: the part of `grid` that does land inside the clip is copied,
    /// matching [`fill_rect`](Self::fill_rect)'s per-cell clipping rather than
    /// [`put_span`](Self::put_span)'s all-or-nothing footprint check, since `grid` is arbitrary
    /// composed content rather than one indivisible sprite.
    ///
    /// Unlike [`put`](Self::put) and the rest of this surface's single-sprite writes, this does
    /// not apply [`with_tint`](Self::with_tint)'s tint: a tint lands on one sprite's anchor cell,
    /// and `grid` is arbitrary composed content with no single anchor to land it on, the same
    /// reason `Grid::blit_cross_layer` (this method's own cross-layer copy, internal to `Grid`)
    /// carries no tint either. A tinted surface's `blit` copies `grid` through unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::color::Style;
    /// use retroglyph_core::grid::{Grid, Rect};
    /// use retroglyph_core::surface::{Layer, Surface};
    /// use retroglyph_core::tile::Tile;
    ///
    /// let mut src = Grid::new(2, 2);
    /// src.put_tile(0, (0, 0), Tile::new('x', Style::default()));
    ///
    /// let mut dst = Grid::new(4, 4);
    /// let mut surface = Surface::new(&mut dst, Rect::new(0, 0, 4, 4), Layer::World.as_u8());
    ///
    /// // `surface` is on the overlay tier; `src` only ever has layer 0, but `blit` reads that
    /// // layer regardless, so the copy still lands (unlike `Grid::blit(surface.layer(), ...)`).
    /// surface.on_tier(Layer::Overlay).blit(&src, 1, 1);
    ///
    /// assert_eq!(dst.tile(Layer::Overlay.as_u8(), (1, 1)).map(Tile::glyph), Some('x'));
    /// ```
    pub fn blit(&mut self, grid: &Grid, x: u16, y: u16) {
        let w = grid.width();
        let h = grid.height();
        if w == 0 || h == 0 {
            return;
        }

        // Local `(x, y)` shifted by this surface's translate offset, same subtraction `shift`
        // does for a single cell, but a footprint that starts left of/above the origin crops its
        // near edge instead of being dropped whole (there is no single `(x, y)` for `shift` to
        // reject: only part of the footprint may be off-screen).
        let sx = i64::from(x) - i64::from(self.origin_offset.0);
        let sy = i64::from(y) - i64::from(self.origin_offset.1);
        let crop_left = u16::try_from(sx.min(0).unsigned_abs()).unwrap_or(u16::MAX);
        let crop_top = u16::try_from(sy.min(0).unsigned_abs()).unwrap_or(u16::MAX);
        if crop_left >= w || crop_top >= h {
            return;
        }
        let Ok(local_x) = u16::try_from(sx.max(0)) else {
            return;
        };
        let Ok(local_y) = u16::try_from(sy.max(0)) else {
            return;
        };

        let abs_x = self.area.left().saturating_add(local_x);
        let abs_y = self.area.top().saturating_add(local_y);
        let visible_w = (w - crop_left).min(u16::MAX - abs_x);
        let visible_h = (h - crop_top).min(u16::MAX - abs_y);

        let dst_rect = Rect::new(abs_x, abs_y, visible_w, visible_h).intersect(self.clip);
        if dst_rect.is_empty() {
            return;
        }

        let src_rect = Rect::new(
            crop_left + (dst_rect.left() - abs_x),
            crop_top + (dst_rect.top() - abs_y),
            dst_rect.width(),
            dst_rect.height(),
        );
        self.grid.blit_cross_layer(
            self.layer,
            grid,
            0,
            src_rect,
            dst_rect.left(),
            dst_rect.top(),
        );
    }

    /// Clears this surface's own area, intersected with its clip (on its own layer), back to
    /// [`Tile::default`].
    pub fn clear(&mut self) {
        let region = self.area.intersect(self.clip);
        self.grid.fill_rect(self.layer, region, Tile::default());
    }

    /// Clears `rect` (clipped to this surface's own clip, on its own layer) back to
    /// [`Tile::default`].
    ///
    /// `rect` is local to this surface's own [`area`](Self::area), the same convention
    /// [`fill_rect`](Self::fill_rect) and [`print_aligned`](Self::print_aligned) use for their
    /// own `rect` (not absolute grid coordinates, the convention
    /// [`clip`](Self::clip)/[`scope`](Self::scope) use).
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::color::Style;
    /// use retroglyph_core::grid::{Grid, Pos, Rect};
    /// use retroglyph_core::surface::Surface;
    ///
    /// let mut grid = Grid::new(4, 4);
    /// let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 4, 4), 0);
    /// surface.fill_rect(Rect::new(0, 0, 4, 4), '#', Style::default());
    ///
    /// // `rect` extends past the surface's own clip; only the overlap is cleared.
    /// surface.clear_region(Rect::new(2, 2, 10, 10));
    ///
    /// assert_eq!(grid[Pos::new(2, 2)].glyph(), ' ');
    /// assert_eq!(grid[Pos::new(1, 1)].glyph(), '#');
    /// ```
    pub fn clear_region(&mut self, rect: Rect) {
        if let Some(abs) = self.local_rect_to_absolute(rect) {
            self.grid.fill_rect(self.layer, abs, Tile::default());
            return;
        }

        let rect = self.clip_local_rect(rect);
        for pos in rect {
            if let Some((x, y)) = self.shift(pos.x, pos.y) {
                self.grid.put_tile(self.layer, (x, y), Tile::default());
            }
        }
    }
}
