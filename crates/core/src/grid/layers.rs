//! `Grid`'s multi-layer API: per-layer tile access ([`Grid::put_tile`], [`Grid::fill_region`],
//! [`Grid::tint`], [`Grid::set_tint`], [`Grid::tile`], [`Grid::tile_mut`]), cross-grid copies
//! ([`Grid::blit`], [`Grid::blit_alpha`]), whole-grid iteration and clearing ([`Grid::layers`],
//! [`Grid::clear_all`]), and the single-layer compositing `flatten_into` uses for cell backends.
//!
//! The [`BlendMode`] blend math backing [`Grid::blit_alpha`] lives here too, next to its only
//! caller.

use super::{BlendMode, Grid, Pos, Rect, TileExtra, flat_index_to_xy, to_grixy_pos};
use crate::backend::DrawCell;
use crate::color::Color;
#[cfg(test)]
use crate::color::Style;
use crate::color::Tint;
use crate::tile::{Tile, TileFlags};
use alloc::vec::Vec;
use alpha_blend::BlendMode as SeparableBlendMode;
use alpha_blend::channel::Channel;
use grixy::ops::{ExactSizeGrid, GridRead, GridWrite};

impl Grid {
    /// Write a tile to `layer` at `pos`, honoring `tile`'s own precomputed
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
    /// [`DrawCell`] stream verbatim into its own copy) rather than a new
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
        if !lb.buf.contains(gpos) {
            return None;
        }
        lb.extras.remove(&idx);
        tile.flags.remove(TileFlags::HAS_EXTRA);
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
    /// cell's own extras entry, if any, is dropped rather than orphaned.
    ///
    /// # Examples
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
        let bounds = Rect::new(0, 0, self.width, self.height);
        let rect = rect.intersect(bounds);
        if rect.is_empty() {
            return;
        }

        // Clear every span (and, under `egc`, every wide-char cell) this fill would partially
        // overwrite, one row at a time rather than one cell at a time: still O(rows), and a no-op
        // on a grid that has never used spans (see `clear_span_overlap`).
        for y in rect.top()..rect.bottom() {
            self.clear_span_overlap(layer, rect.left(), y, rect.width());
            #[cfg(feature = "egc")]
            self.clear_overlap(layer, rect.left(), y, rect.width());
        }

        // `tile` is a caller-constructed `Tile` (see `put_tile`'s own doc comment for why that
        // can never carry `HAS_EXTRA`), so every cell's own extras entry is now stale. Drop them
        // per row rather than scanning the whole side table: bounded by the region, not by
        // however much of the layer happens to carry extras elsewhere.
        tile.flags.remove(TileFlags::HAS_EXTRA);
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

    /// Sets the whole side-table entry for an already-written tile at `(x, y)` on `layer`,
    /// setting [`TileFlags::HAS_EXTRA`] to match. Does nothing if out of bounds. Crate-private:
    /// the external ways in are [`write_grapheme`](Self::write_grapheme) and
    /// [`set_tint`](Self::set_tint).
    ///
    /// An empty entry is removed rather than stored, so the flag means exactly "an entry
    /// exists".
    pub(crate) fn set_extra(&mut self, layer: u8, x: u16, y: u16, extra: TileExtra) {
        let pos = to_grixy_pos(Pos::new(x, y));
        let idx = usize::from(y) * usize::from(self.width) + usize::from(x);
        let lb = self.layer_or_alloc(layer);
        if lb.buf.contains(pos) {
            if extra.is_empty() {
                lb.buf[pos].flags.remove(TileFlags::HAS_EXTRA);
                lb.extras.remove(&idx);
            } else {
                lb.buf[pos].flags.insert(TileFlags::HAS_EXTRA);
                lb.extras.insert(idx, extra);
            }
        }
    }

    /// How a pixel backend recolours the sprite drawn for the cell at `(x, y)` on `layer`.
    ///
    /// [`Tint::None`] for a cell that has never been tinted, for a cell whose glyph was
    /// overwritten since (a glyph write drops the tint with the artwork it belonged to), and for
    /// coordinates outside the grid or on an unallocated layer.
    ///
    /// A tint is grid state rather than [`Tile`] state, for the same reason a multi-codepoint
    /// grapheme is (see [`grapheme`](Self::grapheme)): it is rare per cell and `Tile` has no room
    /// left. So it is read here, not through [`Tile::style`].
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
        let idx = usize::from(y) * usize::from(self.width) + usize::from(x);
        let pos = to_grixy_pos(Pos::new(x, y));
        let lb = self.layer_or_alloc(layer);
        if !lb.buf.contains(pos) {
            return;
        }
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

    /// Read a tile on `layer` at `pos`, or `None` if the layer is
    /// unallocated or `pos` is out of bounds.
    #[must_use]
    pub fn tile(&self, layer: u8, pos: impl Into<Pos>) -> Option<&Tile> {
        let pos = to_grixy_pos(pos.into());
        self.layer(layer)?.buf.get(pos)
    }

    /// Mutably borrow a tile on `layer` at `pos`, or `None` if the layer is
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

    /// Copy tiles from `src` within `src_rect` to `self` at `(dst_x, dst_y)`
    /// on `layer`. Empty tiles (nothing written; see [`Tile::is_empty`]) are
    /// treated as transparent and skipped. An explicit space is copied and
    /// overwrites the destination.
    ///
    /// Multi-cell spans (see [`write_span`](Self::write_span)) do **not** survive a blit: copied
    /// tiles keep their glyphs but lose [`TileFlags::SPAN_ANCHOR`]/[`TileFlags::SPAN_COVERED`],
    /// so a span degrades to exactly its text fallback. `src_rect` can clip a span in half, and
    /// half a span is not a thing the grid can represent; degrading to the fallback glyphs is
    /// both representable and the same content a cell backend would have drawn anyway.
    ///
    /// The same is true of wide-character pairs: `src_rect` clipping a lead from its spacer, or
    /// the copy landing on only one half of a destination pair, both leave half a pair, which is
    /// equally unrepresentable. Either case strips [`TileFlags::WIDE_CHAR`]/
    /// [`TileFlags::WIDE_CHAR_SPACER`] from the surviving half (or clears the destination half
    /// the copy overwrites), so a blit can never leave a dangling lead or an orphaned spacer
    /// behind (retroglyph#1013).
    ///
    /// Walks `src`'s and `self`'s layer buffers directly by flat index instead of going through
    /// [`tile`](Self::tile)/[`put_tile`](Self::put_tile) per cell (see retroglyph#263):
    /// each of those recomputes a coordinate conversion and a bounds check per cell, which this
    /// does once per row instead. The destination layer is allocated once, up front, rather than
    /// as a side effect of the first written cell, but only if `src_rect` (clamped to `src`'s
    /// bounds) contains at least one non-empty tile, matching `put_tile`'s original
    /// allocate-on-first-write behavior for a `src_rect` that is entirely transparent.
    pub fn blit(&mut self, layer: u8, src: &Self, src_rect: Rect, dst_x: u16, dst_y: u16) {
        self.blit_with(
            layer,
            src,
            layer,
            src_rect,
            dst_x,
            dst_y,
            |tile, _dst_tile| *tile,
        );
    }

    /// Same as [`blit`](Self::blit) but blends foreground and background
    /// colors with the given alpha factors, using `mode` to compute the
    /// blended color. `fg_alpha` and `bg_alpha` are in 0.0-1.0 range where
    /// 0.0 = keep destination, 1.0 = replace with src; for a non-
    /// [`Linear`](BlendMode::Linear) `mode`, "replace with src" instead means
    /// "replace with `mode`'s fully blended color" (see [`BlendMode`]).
    ///
    /// Blending operates on packed RGB values; [`Color::Default`] preserves
    /// the destination. Non-RGB color variants (Ansi/Indexed) are passed
    /// through unblended, regardless of `mode`.
    ///
    /// [`BlendMode::Linear`]'s per-channel color lerp is delegated to [`gem::Mix`]. The other
    /// modes delegate to [`alpha_blend::BlendMode`] (imported in this module as
    /// `SeparableBlendMode` to avoid colliding with this crate's own [`BlendMode`]).
    ///
    /// Like [`blit`](Self::blit) (see retroglyph#262/#263), walks `src`'s and `self`'s layer
    /// buffers directly by flat index instead of per-cell [`tile`](Self::tile)/
    /// [`put_tile`](Self::put_tile), and allocates the destination layer once, up front, rather
    /// than as a side effect of the first written cell.
    #[allow(clippy::too_many_arguments, clippy::float_cmp)]
    pub fn blit_alpha(
        &mut self,
        layer: u8,
        src: &Self,
        src_rect: Rect,
        dst_x: u16,
        dst_y: u16,
        mode: BlendMode,
        fg_alpha: f32,
        bg_alpha: f32,
    ) {
        self.blit_with(
            layer,
            src,
            layer,
            src_rect,
            dst_x,
            dst_y,
            |tile, dst_tile| {
                let mut blended = *tile;
                // `fg_alpha == 1.0` only lets `Linear` skip the call: `Linear` at `t ==
                // 1.0` is `src` by definition, but a `Screen`/`Dodge`/`Burn`/`Overlay`
                // mix at full alpha still needs to run the mode's formula: it isn't
                // equivalent to the raw source color (see `blend_color`'s matching guard).
                if mode != BlendMode::Linear || fg_alpha != 1.0 {
                    blended.style.fg = blend_fg(mode, tile.style.fg, dst_tile.style.fg, fg_alpha);
                }
                if mode != BlendMode::Linear || bg_alpha != 1.0 {
                    blended.style.bg = blend_bg(mode, tile.style.bg, dst_tile.style.bg, bg_alpha);
                }
                blended
            },
        );
    }

    /// Same as [`blit`](Self::blit), except the source tiles are read from `src_layer` on `src`
    /// rather than from `dst_layer` (the layer this writes to on `self`).
    ///
    /// [`blit`](Self::blit) uses one `layer` for both sides, which is exactly right for two
    /// grids sharing the same layer scheme (e.g. [`Surface::on_layer`](crate::Surface::on_layer)
    /// copying within itself), but wrong for [`Surface::blit`](crate::Surface::blit)'s case: a
    /// `src` that is a standalone, layer-0-only `Grid` (composed content like `BoxStyle::render`'s
    /// output), stamped onto a destination surface that may currently be on any layer. Calling
    /// [`blit`](Self::blit) with the destination's layer there looks up that same layer on `src`,
    /// finds nothing (`src` only ever populated layer 0), and silently copies nothing
    /// (retroglyph#824). This method exists so a caller in that position can pin `src_layer` to
    /// `0` independently of `dst_layer`.
    pub(crate) fn blit_cross_layer(
        &mut self,
        dst_layer: u8,
        src: &Self,
        src_layer: u8,
        src_rect: Rect,
        dst_x: u16,
        dst_y: u16,
    ) {
        self.blit_with(
            dst_layer,
            src,
            src_layer,
            src_rect,
            dst_x,
            dst_y,
            |tile, _dst_tile| *tile,
        );
    }

    /// Shared copy loop behind [`blit`](Self::blit), [`blit_alpha`](Self::blit_alpha), and
    /// [`blit_cross_layer`](Self::blit_cross_layer): clamps `src_rect` to `src`'s bounds, skips
    /// the whole call if nothing in it is visible, clears any destination span or wide-character
    /// pair the copy is about to partially overwrite (retroglyph#710, retroglyph#1013), and walks
    /// matching `src`/destination cells by
    /// flat index (retroglyph#262/#263), applying `transform` to each non-empty source tile
    /// (given the source tile and, for context, the destination tile it's about to replace)
    /// before writing it and fixing up grapheme extras. `dst_x`/`dst_y` saturate on overflow
    /// (retroglyph#268) rather than wrapping; the bounds checks below always catch a saturated
    /// `u16::MAX` origin.
    ///
    /// `dst_layer` and `src_layer` are separate parameters (rather than the one `layer` [`blit`]
    /// and [`blit_alpha`] expose) so [`blit_cross_layer`](Self::blit_cross_layer) can read a
    /// different source layer than the one it writes: see that method's own doc for why (this is
    /// the retroglyph#824 fix).
    #[allow(clippy::too_many_arguments)]
    fn blit_with(
        &mut self,
        dst_layer: u8,
        src: &Self,
        src_layer: u8,
        src_rect: Rect,
        dst_x: u16,
        dst_y: u16,
        transform: impl Fn(&Tile, &Tile) -> Tile,
    ) {
        let Some(src_lb) = src.layer(src_layer) else {
            return;
        };
        let src_width = usize::from(src.width);
        let sx0 = src_rect.left().min(src.width);
        let sx1 = src_rect.right().min(src.width);
        let sy0 = src_rect.top().min(src.height);
        let sy1 = src_rect.bottom().min(src.height);
        if sx0 >= sx1 || sy0 >= sy1 {
            return;
        }

        // Matches the original's implicit allocate-on-first-write: only touch the destination
        // layer at all if there's at least one visible (non-empty) source tile to copy.
        let has_visible = (sy0..sy1).any(|sy| {
            let start = usize::from(sy) * src_width + usize::from(sx0);
            let end = usize::from(sy) * src_width + usize::from(sx1);
            src_lb.buf.as_ref()[start..end]
                .iter()
                .any(|t| !t.flags.contains(TileFlags::EMPTY))
        });
        if !has_visible {
            return;
        }

        let dst_width = usize::from(self.width);
        let dst_height = usize::from(self.height);

        // A blit writes straight into the destination buffer below, bypassing `put_tile`, so it
        // has to do `put_tile`'s `clear_span_overlap`/`clear_overlap` calls itself, or a cell that
        // used to anchor (or be covered by) a multi-cell span, or half of a wide-character pair,
        // would keep claiming cells this blit just overwrote (retroglyph#710, retroglyph#1013).
        // Only the cells actually being overwritten (in bounds, non-empty source tile) are
        // cleared: an empty source tile is transparent and leaves the destination untouched, so
        // clearing a whole row's footprint up front would wipe out spans/pairs the blit never
        // actually touches. `clear_span_overlap` is gated on `has_spans` so a grid that never uses
        // spans pays only the one `bool` check; `clear_overlap` has no such gate because `put_tile`
        // itself never gates it (`WIDE_CHAR`/`WIDE_CHAR_SPACER` are set on every feature
        // combination, not just under `egc`).
        for sy in sy0..sy1 {
            let dy = dst_y.saturating_add(sy - src_rect.top());
            if usize::from(dy) >= dst_height {
                continue;
            }
            for sx in sx0..sx1 {
                let dx = dst_x.saturating_add(sx - src_rect.left());
                if usize::from(dx) >= dst_width {
                    continue;
                }
                let src_idx = usize::from(sy) * src_width + usize::from(sx);
                if src_lb.buf.as_ref()[src_idx]
                    .flags
                    .contains(TileFlags::EMPTY)
                {
                    continue;
                }
                if self.has_spans {
                    self.clear_span_overlap(dst_layer, dx, dy, 1);
                }
                self.clear_overlap(dst_layer, dx, dy, 1);
            }
        }

        let dst_lb = self.layer_or_alloc(dst_layer);
        let mut pending_extras: Vec<(usize, TileExtra)> = Vec::new();

        for sy in sy0..sy1 {
            let dy = dst_y.saturating_add(sy - src_rect.top());
            if usize::from(dy) >= dst_height {
                continue;
            }
            for sx in sx0..sx1 {
                let dx = dst_x.saturating_add(sx - src_rect.left());
                if usize::from(dx) >= dst_width {
                    continue;
                }
                let src_idx = usize::from(sy) * src_width + usize::from(sx);
                let tile = &src_lb.buf.as_ref()[src_idx];
                if tile.flags.contains(TileFlags::EMPTY) {
                    continue;
                }
                let dst_idx = usize::from(dy) * dst_width + usize::from(dx);
                let dst_tile = dst_lb.buf.as_ref()[dst_idx];
                let mut out_tile = transform(tile, &dst_tile);
                out_tile.flags.remove(TileFlags::HAS_EXTRA);
                out_tile.clear_span();

                // Half a wide-character pair is as unrepresentable as half a span (see
                // `clear_span` above): `src_rect` or the destination clip can separate a lead
                // from its spacer, so drop the flag on whichever half survives the copy alone
                // rather than leave a dangling lead (no spacer to its right) or an orphaned
                // spacer (no lead to its left) (retroglyph#1013).
                if out_tile.flags.contains(TileFlags::WIDE_CHAR) {
                    let partner_survived = sx + 1 < sx1 && usize::from(dx) + 1 < dst_width;
                    if !partner_survived {
                        out_tile.clear_wide();
                    }
                } else if out_tile.flags.contains(TileFlags::WIDE_CHAR_SPACER) {
                    let partner_survived = sx > sx0 && dx > 0;
                    if !partner_survived {
                        out_tile.clear_wide();
                    }
                }
                dst_lb.buf.as_mut()[dst_idx] = out_tile;
                if tile.flags.contains(TileFlags::HAS_EXTRA) {
                    if let Some(extra) = src_lb.extra_entry_for(src_idx, tile) {
                        pending_extras.push((dst_idx, extra));
                    }
                } else {
                    dst_lb.extras.remove(&dst_idx);
                }
            }
        }

        for (idx, extra) in pending_extras {
            dst_lb.buf.as_mut()[idx].flags.insert(TileFlags::HAS_EXTRA);
            dst_lb.extras.insert(idx, extra);
        }
    }

    /// Yield `(layer_id, Pos, &Tile, Option<&str>)` for every allocated cell
    /// across all layers, in layer-major (0 → `max_layer`) then row-major
    /// order. The last element is the tile's grapheme text (see
    /// [`grapheme`](Self::grapheme)), `Some` only when
    /// [`TileFlags::HAS_EXTRA`] is set.
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

    /// Clear every allocated layer.
    pub fn clear_all(&mut self) {
        for layer in self.layers.iter_mut().flatten() {
            layer.buf.clear();
            layer.extras.clear();
        }
    }

    /// Composite every allocated layer into `dst`'s layer 0, one tile per cell.
    ///
    /// Used by [`crate::Terminal::present`] for backends that do not composite
    /// layers themselves (see [`crate::Output::composites_layers`]). The rule
    /// matches the software renderer's pixel semantics and the [`blit`](Self::blit)
    /// transparency convention:
    ///
    /// - Start from layer 0's tile (its `bg` fills the cell).
    /// - For each higher allocated layer, in ascending order: if the tile is
    ///   not empty (see [`Tile::is_empty`]) replace the glyph, foreground,
    ///   offsets, flags, span, and extra; if its background is not
    ///   [`Color::Default`], replace the background.
    ///
    /// The span fields travel with the flags they are keyed by (see [`Tile::span`]): a
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

/// Blend two [`Color`] values using `mode`. [`Color::Default`] preserves the
/// destination. Non-RGB source colors are returned as-is (no resolution).
///
/// [`BlendMode::Linear`] is a per-channel sRGB-domain lerp (dst -> src by `t`) delegated to
/// [`gem::Mix`], which is `no_std`-safe (round-half-away via `floor(x + 0.5)`, no `std`/`libm`
/// float intrinsics). The other modes evaluate [`SeparableBlendMode::mix`] per channel in
/// `0.0..=1.0` (converting u8 <-> f32 at the boundary; see [`blend_separable_channel`]), then lerp
/// that fully mixed color against the destination by `t`, same as `Linear`.
#[allow(clippy::float_cmp)]
fn blend_color(mode: BlendMode, src: Color, dst: Color, t: f32) -> Color {
    use gem::Mix as _;
    use gem::rgb::{HasBlue as _, HasGreen as _, HasRed as _, Rgb888};
    match (src, dst) {
        (Color::Default, _) => Color::Default,
        (
            Color::Rgb {
                r: sr,
                g: sg,
                b: sb,
            },
            Color::Rgb {
                r: dr,
                g: dg,
                b: db,
            },
        ) if mode != BlendMode::Linear || t != 1.0 => {
            // `Linear` at `t == 1.0` is `src` by definition (skip to the catch-all arm below);
            // the other modes must still run their mix formula at `t == 1.0`: see `blit_alpha`.
            let (r, g, b) = mode.separable().map_or_else(
                || {
                    // `dst.mix(src, t)`, not `src.mix(dst, t)`: at `t == 0.0` this must return
                    // `dst` ("keep destination", per `blit_alpha`'s doc comment) and only reach
                    // `src` at `t == 1.0`: the same `0.0 == dst, 1.0 == fully blended` contract
                    // every other `BlendMode` follows (see `blend_separable_channel`).
                    let out = Rgb888::from_rgb(dr, dg, db).mix(Rgb888::from_rgb(sr, sg, sb), t);
                    (out.red(), out.green(), out.blue())
                },
                |sep| {
                    (
                        blend_separable_channel(sep, sr, dr, t),
                        blend_separable_channel(sep, sg, dg, t),
                        blend_separable_channel(sep, sb, db, t),
                    )
                },
            );
            Color::Rgb { r, g, b }
        }
        (src, _) => src,
    }
}

/// Evaluates `sep`'s per-channel mixing function for one RGB channel (`src`/`dst` are u8, `sep`
/// operates in `0.0..=1.0` f32), then lerps that mixed value against `dst` by `t`: `0.0` keeps
/// `dst`, `1.0` uses the fully mixed color. Clamps before converting back to u8 via
/// `Channel::from_f32`, since `ColorDodge`/`ColorBurn`'s `min(1.0, ...)` branches can round a
/// hair outside `0.0..=1.0` at the float boundary.
fn blend_separable_channel(sep: SeparableBlendMode, src: u8, dst: u8, t: f32) -> u8 {
    let cs = Channel::to_f32(src);
    let cb = Channel::to_f32(dst);
    let mixed = sep.mix(cb, cs);
    // A plain multiply-add measurably disagrees with a fused one (`crate::math::mul_add`) by
    // ±1 LSB on some inputs.
    let blended = crate::math::mul_add(mixed - cb, t, cb);
    Channel::from_f32(blended.clamp(0.0, 1.0))
}

fn blend_fg(mode: BlendMode, src: Color, dst: Color, t: f32) -> Color {
    blend_color(mode, src, dst, t)
}

fn blend_bg(mode: BlendMode, src: Color, dst: Color, t: f32) -> Color {
    blend_color(mode, src, dst, t)
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
        assert!(g.grapheme(255, 0, 0).is_none());
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

        assert_eq!(g.grapheme(0, 1, 1), None);
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

    /// An empty (or fully out-of-bounds) `rect` allocates nothing: `fill_region` returns before
    /// touching `layer_or_alloc`.
    #[test]
    fn fill_region_on_an_empty_rect_does_not_allocate_the_layer() {
        let mut g = Grid::new(4, 4);
        g.fill_region(1, Rect::new(10, 10, 2, 2), Tile::new('#', Style::default()));
        assert_eq!(g.tile(1, (0, 0)), None);
    }

    #[cfg(feature = "egc")]
    #[test]
    fn blit_carries_a_tint_across_grids() {
        let mut src = Grid::new(4, 4);
        src.write_grapheme(0, 1, 1, "@", Style::default());
        src.set_tint(0, 1, 1, Tint::multiply(64, 128, 192));

        let mut dst = Grid::new(4, 4);
        // Pre-existing tint on the destination cell, to prove the copy replaces rather than
        // merges with whatever was there.
        dst.set_tint(0, 1, 1, Tint::mix(9, 9, 9, 9));
        dst.blit(0, &src, Rect::new(0, 0, 4, 4), 0, 0);

        assert_eq!(dst.tint(0, 1, 1), Tint::multiply(64, 128, 192));
        assert_eq!(dst.tint(0, 0, 0), Tint::None);
    }

    #[cfg(feature = "egc")]
    #[test]
    fn blit_clears_a_destination_tint_where_the_source_has_none() {
        let mut src = Grid::new(2, 2);
        src.write_grapheme(0, 0, 0, "@", Style::default());

        let mut dst = Grid::new(2, 2);
        dst.set_tint(0, 0, 0, Tint::multiply(1, 2, 3));
        dst.blit(0, &src, Rect::new(0, 0, 2, 2), 0, 0);

        assert_eq!(dst.tint(0, 0, 0), Tint::None);
    }

    #[cfg(feature = "egc")]
    #[test]
    fn a_tint_and_a_grapheme_share_one_entry_without_clobbering_each_other() {
        let mut g = Grid::new(4, 4);
        g.write_grapheme(0, 1, 1, "e\u{0301}", Style::default());
        g.set_tint(0, 1, 1, Tint::multiply(128, 128, 128));

        // Both members survive: `set_tint` preserves the grapheme already stored.
        assert_eq!(g.grapheme(0, 1, 1), Some("e\u{0301}"));
        assert_eq!(g.tint(0, 1, 1), Tint::multiply(128, 128, 128));

        // Clearing the tint leaves the grapheme, and so leaves the entry in place.
        g.set_tint(0, 1, 1, Tint::None);
        assert_eq!(g.grapheme(0, 1, 1), Some("e\u{0301}"));
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
        assert_eq!(g.grapheme(0, 1, 1), None);
        assert_eq!(g.tint(0, 1, 1), Tint::multiply(128, 128, 128));
    }

    #[cfg(feature = "egc")]
    #[test]
    fn test_grid_blit_preserves_extra() {
        let mut src = Grid::new(2, 2);
        src.write_grapheme(0, 0, 0, "e\u{0301}", Style::default());

        let mut dst = Grid::new(2, 2);
        dst.blit(0, &src, Rect::new(0, 0, 2, 2), 0, 0);
        assert_eq!(dst[Pos::new(0, 0)].glyph, 'e');
        assert_eq!(dst.grapheme(0, 0, 0), Some("e\u{0301}"));
    }

    #[test]
    fn test_grid_blit_empty_rect_is_a_no_op() {
        // A zero-area `src_rect` has no cells at all: `sx0 >= sx1` should short-circuit before
        // touching the destination.
        let src = Grid::new(2, 2);
        let mut dst = Grid::new(2, 2);
        dst.put_tile(0, (0, 0), Tile::new('x', Style::default()));
        dst.blit(0, &src, Rect::new(0, 0, 0, 0), 0, 0);
        assert_eq!(dst[Pos::new(0, 0)].glyph(), 'x');
        assert_eq!(dst.max_layer(), 0);
    }

    #[test]
    fn test_grid_blit_fully_transparent_source_does_not_allocate_dst_layer() {
        // Perf refactor (#263): the destination layer is allocated up front, but only after
        // confirming the (clamped) source region has at least one non-empty tile, matching
        // `put_tile`'s original allocate-on-first-write behavior for an all-transparent blit.
        let src = Grid::new(2, 2);
        let mut dst = Grid::new(2, 2);
        dst.blit(3, &src, Rect::new(0, 0, 2, 2), 0, 0);
        assert_eq!(dst.max_layer(), 0);
    }

    #[test]
    fn test_grid_blit_skips_out_of_bounds_source_and_dest_regions() {
        let mut src = Grid::new(4, 4);
        for y in 0..4 {
            for x in 0..4 {
                src.put_tile(0, (x, y), Tile::new('#', Style::default()));
            }
        }

        let mut dst = Grid::new(2, 2);
        // `src_rect` extends past `src`'s bounds and the destination offset pushes part of the
        // copied region past `dst`'s bounds too; both should be silently clamped, not panic.
        dst.blit(0, &src, Rect::new(2, 2, 10, 10), 1, 1);
        assert_eq!(dst[Pos::new(1, 1)].glyph(), '#');
        assert_eq!(dst[Pos::new(0, 0)].glyph(), ' ');
        assert_eq!(dst[Pos::new(0, 1)].glyph(), ' ');
        assert_eq!(dst[Pos::new(1, 0)].glyph(), ' ');
    }

    #[test]
    fn test_grid_blit_sub_cell_offset_and_transparency() {
        let mut src = Grid::new(2, 2);
        src.put_tile(0, (0, 0), Tile::new('A', Style::default()));
        // (1, 0) and (1, 1) stay at their default (empty) tile: transparent, should not
        // overwrite the destination.
        src.put_tile(0, (0, 1), Tile::new('B', Style::default()));

        let mut dst = Grid::new(3, 3);
        dst.put_tile(0, (2, 2), Tile::new('Z', Style::default()));
        dst.blit(0, &src, Rect::new(0, 0, 2, 2), 1, 1);

        assert_eq!(dst[Pos::new(1, 1)].glyph(), 'A');
        assert_eq!(dst[Pos::new(1, 2)].glyph(), 'B');
        // Untouched by the (transparent) source cells at (1, 0) and (1, 1).
        assert_eq!(dst[Pos::new(2, 1)].glyph(), ' ');
        assert_eq!(dst[Pos::new(2, 2)].glyph(), 'Z');
    }

    #[test]
    fn test_grid_blit_multi_layer_independent() {
        let mut src = Grid::new(2, 2);
        src.put_tile(0, (0, 0), Tile::new('a', Style::default()));
        src.put_tile(2, (0, 0), Tile::new('b', Style::default()));

        let mut dst = Grid::new(2, 2);
        dst.blit(0, &src, Rect::new(0, 0, 2, 2), 0, 0);
        dst.blit(2, &src, Rect::new(0, 0, 2, 2), 0, 0);

        assert_eq!(dst.tile(0, (0, 0)).map(Tile::glyph), Some('a'));
        assert_eq!(dst.tile(2, (0, 0)).map(Tile::glyph), Some('b'));
        // Layer 1 was never written by either blit call.
        assert!(dst.tile(1, (0, 0)).is_none());
    }

    #[test]
    fn test_grid_blit_dest_origin_near_u16_max_does_not_wrap() {
        // retroglyph#268: with a plain (non-saturating) `dst_x + (sx - src_rect.left())`, an
        // origin this close to `u16::MAX` overflows and wraps back into a small, in-bounds
        // value: silently corrupting an unrelated cell instead of being clamped out. Picked so
        // that `dst_x + 3` overflows `u16` and wraps to `1`, which *is* in-bounds for this small
        // `dst` grid: `65534u16.wrapping_add(3) == 1`.
        let mut src = Grid::new(4, 1);
        src.put_tile(0, (3, 0), Tile::new('Q', Style::default()));

        let mut dst = Grid::new(4, 1);
        dst.blit(0, &src, Rect::new(0, 0, 4, 1), u16::MAX - 1, 0);

        // The would-be-wrapped cell (index 1) must not have been touched.
        assert_eq!(dst[Pos::new(1, 0)].glyph(), ' ');
        // No other cell was touched either: the whole row's writes overflowed and were
        // skipped (dst_x saturates to u16::MAX for every column in this row).
        for x in 0..4 {
            assert_eq!(
                dst[Pos::new(x, 0)].glyph(),
                ' ',
                "cell ({x}, 0) unexpectedly written"
            );
        }
    }

    #[test]
    fn test_grid_blit_normal_offset_unaffected_by_overflow_fix() {
        // A typical, non-overflowing blit must still work exactly as before.
        let mut src = Grid::new(2, 2);
        src.put_tile(0, (0, 0), Tile::new('A', Style::default()));
        src.put_tile(0, (1, 1), Tile::new('B', Style::default()));

        let mut dst = Grid::new(4, 4);
        dst.blit(0, &src, Rect::new(0, 0, 2, 2), 1, 1);

        assert_eq!(dst[Pos::new(1, 1)].glyph(), 'A');
        assert_eq!(dst[Pos::new(2, 2)].glyph(), 'B');
    }

    // --- `BlendMode` / `blit_alpha` ---
    #[test]
    fn test_blend_separable_channel_screen() {
        // cb = 102 (0.4), cs = 204 (0.8): screen = cb + cs - cb*cs = 0.88.
        assert_eq!(
            blend_separable_channel(SeparableBlendMode::Screen, 204, 102, 1.0),
            224
        );
        // t = 0.5 lerps the destination halfway to that fully mixed color.
        assert_eq!(
            blend_separable_channel(SeparableBlendMode::Screen, 204, 102, 0.5),
            163
        );
    }

    #[test]
    fn test_blend_separable_channel_dodge() {
        // cb = 51 (0.2), cs = 204 (0.8): min(1, 0.2 / 0.2) saturates to 1.0.
        assert_eq!(
            blend_separable_channel(SeparableBlendMode::ColorDodge, 204, 51, 1.0),
            255
        );
        assert_eq!(
            blend_separable_channel(SeparableBlendMode::ColorDodge, 204, 51, 0.5),
            153
        );
    }

    #[test]
    fn test_blend_separable_channel_burn() {
        // cb = 204 (0.8), cs = 51 (0.2): 1 - min(1, 0.2 / 0.2) bottoms out at 0.0.
        assert_eq!(
            blend_separable_channel(SeparableBlendMode::ColorBurn, 51, 204, 1.0),
            0
        );
        assert_eq!(
            blend_separable_channel(SeparableBlendMode::ColorBurn, 51, 204, 0.5),
            102
        );
    }

    #[test]
    fn test_blend_separable_channel_overlay() {
        // cb = 51 (0.2, the <= 0.5 branch): 2 * cb * cs.
        assert_eq!(
            blend_separable_channel(SeparableBlendMode::Overlay, 204, 51, 1.0),
            82
        );
        assert_eq!(
            blend_separable_channel(SeparableBlendMode::Overlay, 204, 51, 0.5),
            66
        );
        // cb = 204 (0.8, the > 0.5 branch): 1 - 2 * (1 - cb) * (1 - cs).
        assert_eq!(
            blend_separable_channel(SeparableBlendMode::Overlay, 51, 204, 1.0),
            173
        );
    }

    /// End-to-end through `blit_alpha`, not just the per-channel helper: proves `BlendMode`
    /// actually reaches `blend_fg`/`blend_bg` and lands on the destination tile's style.
    #[test]
    fn test_grid_blit_alpha_screen_blends_fg() {
        let mut src = Grid::new(1, 1);
        src.put_tile(
            0,
            (0, 0),
            Tile::default()
                .with_glyph('X')
                .with_style(Style::new().fg(Color::Rgb {
                    r: 204,
                    g: 204,
                    b: 204,
                })),
        );

        let mut dst = Grid::new(1, 1);
        dst.put_tile(
            0,
            (0, 0),
            Tile::default()
                .with_glyph('_')
                .with_style(Style::new().fg(Color::Rgb {
                    r: 102,
                    g: 102,
                    b: 102,
                })),
        );

        dst.blit_alpha(
            0,
            &src,
            Rect::new(0, 0, 1, 1),
            0,
            0,
            BlendMode::Screen,
            1.0,
            1.0,
        );
        assert_eq!(
            dst[Pos::new(0, 0)].style.fg,
            Color::Rgb {
                r: 224,
                g: 224,
                b: 224
            }
        );
    }

    /// retroglyph#268: same wraparound guard as `blit`'s
    /// `test_grid_blit_dest_origin_near_u16_max_does_not_wrap`, but through `blit_alpha`'s
    /// separate `dst_x`/`dst_y` computation.
    #[test]
    fn test_grid_blit_alpha_dest_origin_near_u16_max_does_not_wrap() {
        let mut src = Grid::new(4, 1);
        src.put_tile(0, (3, 0), Tile::new('Q', Style::default()));

        let mut dst = Grid::new(4, 1);
        dst.blit_alpha(
            0,
            &src,
            Rect::new(0, 0, 4, 1),
            u16::MAX - 1,
            0,
            BlendMode::Linear,
            1.0,
            1.0,
        );

        for x in 0..4 {
            assert_eq!(
                dst[Pos::new(x, 0)].glyph(),
                ' ',
                "cell ({x}, 0) unexpectedly written"
            );
        }
    }

    /// `BlendMode::Linear` at `t == 0.0` keeps the destination and at `t == 1.0` uses the source,
    /// matching `blit_alpha`'s doc comment (this direction was actually inverted before this
    /// change: the underlying `gem::Mix` call had `src`/`dst` swapped, so `t == 0.0` used
    /// to return `src` and `t == 1.0` returned `dst`. No prior tests covered `blit_alpha`, so
    /// this had shipped unnoticed).
    #[test]
    fn test_grid_blit_alpha_linear_direction() {
        let mut src = Grid::new(1, 1);
        src.put_tile(
            0,
            (0, 0),
            Tile::default()
                .with_glyph('X')
                .with_style(Style::new().fg(Color::Rgb {
                    r: 255,
                    g: 255,
                    b: 255,
                })),
        );

        let dst_color = Color::Rgb { r: 0, g: 0, b: 0 };
        let at = |t: f32| {
            let mut dst = Grid::new(1, 1);
            dst.put_tile(
                0,
                (0, 0),
                Tile::default()
                    .with_glyph('_')
                    .with_style(Style::new().fg(dst_color)),
            );
            dst.blit_alpha(
                0,
                &src,
                Rect::new(0, 0, 1, 1),
                0,
                0,
                BlendMode::Linear,
                t,
                1.0,
            );
            dst[Pos::new(0, 0)].style.fg
        };

        assert_eq!(at(0.0), dst_color);
        assert_eq!(
            at(1.0),
            Color::Rgb {
                r: 255,
                g: 255,
                b: 255
            }
        );
        let Color::Rgb { r, g, b } = at(0.5) else {
            panic!("expected Color::Rgb");
        };
        assert!(r > 0 && r < 255, "expected a mid-gray, got {r}");
        assert_eq!(r, g);
        assert_eq!(g, b);
    }

    /// Every `BlendMode` preserves `Color::Default` and passes non-RGB colors through unblended,
    /// same as the pre-existing `Linear` behavior.
    #[test]
    fn test_blend_color_non_rgb_passthrough_all_modes() {
        for mode in [
            BlendMode::Linear,
            BlendMode::Screen,
            BlendMode::Dodge,
            BlendMode::Burn,
            BlendMode::Overlay,
            BlendMode::Multiply,
        ] {
            assert_eq!(
                blend_color(mode, Color::Default, Color::Rgb { r: 1, g: 2, b: 3 }, 0.5),
                Color::Default
            );
            assert_eq!(
                blend_color(mode, Color::BLACK, Color::WHITE, 0.5),
                Color::BLACK
            );
        }
    }

    #[cfg(feature = "egc")]
    #[test]
    fn test_grid_clone_preserves_extra() {
        let mut g = Grid::new(2, 2);
        g.write_grapheme(0, 0, 0, "e\u{0301}", Style::default());
        let cloned = g.clone();
        assert_eq!(cloned.grapheme(0, 0, 0), Some("e\u{0301}"));
    }

    #[cfg(feature = "egc")]
    #[test]
    fn test_grid_flatten_into_carries_extra_from_higher_layer() {
        let mut g = Grid::new(2, 2);
        g.write_grapheme(1, 0, 0, "e\u{0301}", Style::default());
        let mut flattened = Grid::new(2, 2);
        g.flatten_into(&mut flattened);
        assert_eq!(flattened[Pos::new(0, 0)].glyph, 'e');
        assert_eq!(flattened.grapheme(0, 0, 0), Some("e\u{0301}"));
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

    #[test]
    fn blit_degrades_a_span_to_its_fallback_glyphs() {
        // `src_rect` can clip a footprint in half, and half a span is not representable, so
        // `blit` drops the span role and keeps the glyphs (which are the text fallback anyway).
        let mut src = Grid::new(4, 4);
        src.write_span(0, 0, 0, &["C=", "[]"], Style::default())
            .unwrap();

        let mut dst = Grid::new(4, 4);
        dst.blit(0, &src, Rect::new(0, 0, 2, 2), 0, 0);

        assert_eq!(dst[Pos::new(0, 0)].glyph(), 'C');
        assert_eq!(dst[Pos::new(1, 1)].glyph(), ']');
        assert_eq!(dst[Pos::new(0, 0)].span(), (1, 1));
        assert_eq!(dst.span_owner(0, 1, 1), None);
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let flags = dst[Pos::new(x, y)].flags();
            assert!(!flags.contains(TileFlags::SPAN_ANCHOR), "({x}, {y})");
            assert!(!flags.contains(TileFlags::SPAN_COVERED), "({x}, {y})");
        }
    }

    #[test]
    fn blit_leaves_a_dangling_span_anchor_in_the_destination() {
        // retroglyph#710: `blit` writes straight into the destination buffer, bypassing
        // `put_tile`'s `clear_span_overlap` call, so overwriting a span's covered cell used to
        // leave the anchor still claiming a cell the blit had just replaced.
        let mut dst = Grid::new(4, 1);
        dst.write_span(0, 0, 0, &["ab"], Style::default()).unwrap();

        let mut src = Grid::new(4, 1);
        src.put_tile(0, (1, 0), Tile::new('X', Style::default()));
        dst.blit(0, &src, Rect::new(1, 0, 1, 1), 1, 0);

        assert_eq!(dst[Pos::new(1, 0)].glyph(), 'X');
        assert_eq!(dst.tile(0, Pos::new(0, 0)).map(Tile::span), Some((1, 1)));
        assert!(!dst[Pos::new(0, 0)].flags().contains(TileFlags::SPAN_ANCHOR));
        assert!(
            !dst[Pos::new(1, 0)]
                .flags()
                .contains(TileFlags::SPAN_COVERED)
        );
    }

    #[test]
    fn blit_alpha_leaves_a_dangling_span_anchor_in_the_destination() {
        // Same bug as `blit_leaves_a_dangling_span_anchor_in_the_destination`, but through
        // `blit_alpha`'s separate copy path.
        let mut dst = Grid::new(4, 1);
        dst.write_span(0, 0, 0, &["ab"], Style::default()).unwrap();

        let mut src = Grid::new(4, 1);
        src.put_tile(0, (1, 0), Tile::new('X', Style::default()));
        dst.blit_alpha(
            0,
            &src,
            Rect::new(1, 0, 1, 1),
            1,
            0,
            BlendMode::Linear,
            1.0,
            1.0,
        );

        assert_eq!(dst[Pos::new(1, 0)].glyph(), 'X');
        assert_eq!(dst.tile(0, Pos::new(0, 0)).map(Tile::span), Some((1, 1)));
    }

    #[test]
    fn blit_leaves_a_dangling_wide_char_lead_in_the_destination() {
        // retroglyph#1013: `blit` writes straight into the destination buffer, bypassing
        // `put_tile`'s `clear_overlap` call, so overwriting a wide-character pair's spacer used
        // to leave the lead cell still claiming a spacer the blit had just replaced.
        let mut dst = Grid::new(4, 1);
        dst.put_tile(0, (0, 0), Tile::new('\u{4e2d}', Style::default()));

        let mut src = Grid::new(4, 1);
        src.put_tile(0, (1, 0), Tile::new('X', Style::default()));
        dst.blit(0, &src, Rect::new(1, 0, 1, 1), 1, 0);

        assert_eq!(dst[Pos::new(1, 0)].glyph(), 'X');
        assert!(!dst[Pos::new(0, 0)].flags().contains(TileFlags::WIDE_CHAR));
    }

    #[test]
    fn blit_alpha_leaves_a_dangling_wide_char_lead_in_the_destination() {
        // Same bug as `blit_leaves_a_dangling_wide_char_lead_in_the_destination`, but through
        // `blit_alpha`'s separate copy path.
        let mut dst = Grid::new(4, 1);
        dst.put_tile(0, (0, 0), Tile::new('\u{4e2d}', Style::default()));

        let mut src = Grid::new(4, 1);
        src.put_tile(0, (1, 0), Tile::new('X', Style::default()));
        dst.blit_alpha(
            0,
            &src,
            Rect::new(1, 0, 1, 1),
            1,
            0,
            BlendMode::Linear,
            1.0,
            1.0,
        );

        assert_eq!(dst[Pos::new(1, 0)].glyph(), 'X');
        assert!(!dst[Pos::new(0, 0)].flags().contains(TileFlags::WIDE_CHAR));
    }

    #[test]
    fn blit_degrades_a_wide_char_pair_clipped_by_src_rect() {
        // `src_rect` can clip a wide-character pair in half, and half a pair is not
        // representable, so `blit` drops the `WIDE_CHAR` flag on the lead it does copy, the same
        // way it already degrades a clipped span (retroglyph#1013).
        let mut src = Grid::new(4, 1);
        src.put_tile(0, (0, 0), Tile::new('\u{4e2d}', Style::default()));

        let mut dst = Grid::new(4, 1);
        dst.blit(0, &src, Rect::new(0, 0, 1, 1), 0, 0);

        assert!(!dst[Pos::new(0, 0)].flags().contains(TileFlags::WIDE_CHAR));
    }

    #[test]
    fn blit_alpha_degrades_a_wide_char_pair_clipped_by_src_rect() {
        // Same bug as `blit_degrades_a_wide_char_pair_clipped_by_src_rect`, but through
        // `blit_alpha`'s separate copy path.
        let mut src = Grid::new(4, 1);
        src.put_tile(0, (0, 0), Tile::new('\u{4e2d}', Style::default()));

        let mut dst = Grid::new(4, 1);
        dst.blit_alpha(
            0,
            &src,
            Rect::new(0, 0, 1, 1),
            0,
            0,
            BlendMode::Linear,
            1.0,
            1.0,
        );

        assert!(!dst[Pos::new(0, 0)].flags().contains(TileFlags::WIDE_CHAR));
    }

    /// A single `blit`-vs-`copy_rect_clamped` comparison case for
    /// `blit_clamp_matches_grixys_copy_rect_clamped_on_shared_clipped_rect_cases`.
    struct BlitClampCase {
        name: &'static str,
        src_w: u16,
        src_h: u16,
        dst_w: u16,
        dst_h: u16,
        src_rect: Rect,
        dst_x: u16,
        dst_y: u16,
    }

    /// Every source cell gets a unique glyph derived from its position, so a mismatch in the
    /// clamp/translate math (an off-by-one, a row misaligned after clipping, ...) shows up as the
    /// wrong letter landing in the wrong destination cell, not just a wrong cell count.
    fn blit_clamp_case_glyph_at(x: u16, y: u16, width: u16) -> char {
        let idx = u32::from(y) * u32::from(width) + u32::from(x);
        char::from_u32(u32::from(b'A') + idx).expect("case grids stay within 'A'..='Z'")
    }

    /// Runs one [`BlitClampCase`] through both `Grid::blit` and `grixy::ops::copy_rect_clamped`
    /// on an equivalent pair of plain `grixy::buf::GridBuf`s, and asserts the copied region
    /// agrees cell-for-cell.
    fn assert_blit_clamp_case(case: &BlitClampCase) {
        use grixy::buf::GridBuf;
        use grixy::ops::GridWrite as _;
        use grixy::transform::GridConvertExt as _;

        let mut rg_src = Grid::new(case.src_w, case.src_h);
        for y in 0..case.src_h {
            for x in 0..case.src_w {
                let glyph = blit_clamp_case_glyph_at(x, y, case.src_w);
                rg_src.put_tile(0, (x, y), Tile::default().with_glyph(glyph));
            }
        }
        let mut rg_dst = Grid::new(case.dst_w, case.dst_h);
        rg_dst.blit(0, &rg_src, case.src_rect, case.dst_x, case.dst_y);
        let rg_result: Vec<char> = (0..case.dst_h)
            .flat_map(|y| (0..case.dst_w).map(move |x| (x, y)))
            .map(|(x, y)| rg_dst.tile(0, (x, y)).map_or(' ', Tile::glyph))
            .collect();

        let mut gx_src = GridBuf::<char, _, _>::new_filled(
            usize::from(case.src_w),
            usize::from(case.src_h),
            ' ',
        );
        for y in 0..case.src_h {
            for x in 0..case.src_w {
                let glyph = blit_clamp_case_glyph_at(x, y, case.src_w);
                gx_src
                    .set(grixy::core::Pos::new(usize::from(x), usize::from(y)), glyph)
                    .unwrap();
            }
        }
        let mut gx_dst = GridBuf::<char, _, _>::new_filled(
            usize::from(case.dst_w),
            usize::from(case.dst_h),
            ' ',
        );
        grixy::ops::copy_rect_clamped(
            &gx_src.copied(),
            &mut gx_dst,
            grixy::core::Rect::from_ltwh(
                usize::from(case.src_rect.left()),
                usize::from(case.src_rect.top()),
                usize::from(case.src_rect.width()),
                usize::from(case.src_rect.height()),
            ),
            grixy::core::Pos::new(usize::from(case.dst_x), usize::from(case.dst_y)),
        );
        let (gx_result, _, _) = gx_dst.into_inner();

        assert_eq!(rg_result, gx_result, "case: {}", case.name);
    }

    /// `blit_with`'s clamp math (clamp `src_rect` to `src`'s bounds, translate into destination
    /// space, clamp again to `dst`'s bounds) is a hand-written copy of the algorithm
    /// `grixy::ops::copy_rect_clamped` generalizes (retroglyph#831). This walks a shared set of
    /// clipped-rect cases through both `Grid::blit` and `copy_rect_clamped` on an equivalent pair
    /// of plain `grixy::buf::GridBuf`s, and asserts the copied region agrees cell-for-cell, so the
    /// two can't silently drift apart. `Grid` can't implement `GridRead`/`GridWrite` itself (its
    /// span/extras bookkeeping has no equivalent there), so this compares outcomes rather than
    /// sharing code.
    #[test]
    fn blit_clamp_matches_grixys_copy_rect_clamped_on_shared_clipped_rect_cases() {
        let cases = [
            BlitClampCase {
                name: "fully inside both grids",
                src_w: 4,
                src_h: 4,
                dst_w: 6,
                dst_h: 6,
                src_rect: Rect::new(0, 0, 4, 4),
                dst_x: 1,
                dst_y: 1,
            },
            BlitClampCase {
                name: "src_rect wider than src (source-side clip)",
                src_w: 3,
                src_h: 3,
                dst_w: 5,
                dst_h: 5,
                src_rect: Rect::new(0, 0, 10, 10),
                dst_x: 0,
                dst_y: 0,
            },
            BlitClampCase {
                name: "destination-side clip",
                src_w: 3,
                src_h: 3,
                dst_w: 5,
                dst_h: 5,
                src_rect: Rect::new(0, 0, 3, 3),
                dst_x: 3,
                dst_y: 3,
            },
            BlitClampCase {
                name: "both sides clip, tighter bound wins",
                src_w: 4,
                src_h: 4,
                dst_w: 6,
                dst_h: 6,
                src_rect: Rect::new(0, 0, 10, 10),
                dst_x: 3,
                dst_y: 3,
            },
            BlitClampCase {
                name: "src_rect offset, clipped on src's right/bottom",
                src_w: 4,
                src_h: 4,
                dst_w: 6,
                dst_h: 6,
                src_rect: Rect::new(2, 2, 5, 5),
                dst_x: 0,
                dst_y: 0,
            },
            BlitClampCase {
                name: "source completely out of bounds",
                src_w: 3,
                src_h: 3,
                dst_w: 5,
                dst_h: 5,
                src_rect: Rect::new(5, 5, 2, 2),
                dst_x: 0,
                dst_y: 0,
            },
            BlitClampCase {
                name: "destination completely out of bounds",
                src_w: 3,
                src_h: 3,
                dst_w: 5,
                dst_h: 5,
                src_rect: Rect::new(0, 0, 3, 3),
                dst_x: 10,
                dst_y: 10,
            },
            BlitClampCase {
                name: "zero-size src_rect",
                src_w: 3,
                src_h: 3,
                dst_w: 5,
                dst_h: 5,
                src_rect: Rect::new(0, 0, 0, 0),
                dst_x: 0,
                dst_y: 0,
            },
        ];

        for case in &cases {
            assert_blit_clamp_case(case);
        }
    }
}
