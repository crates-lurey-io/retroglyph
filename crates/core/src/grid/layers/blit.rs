//! Cross-grid copies: [`Grid::blit`], [`Grid::blit_alpha`], and [`Grid::blit_cross_layer`], built
//! on the shared [`Grid::blit_with`] copy loop.
//!
//! The [`BlendMode`] blend math backing [`Grid::blit_alpha`] lives here too, next to its only
//! caller.

#[cfg(test)]
use super::super::Pos;
use super::super::{BlendMode, Grid, Rect, TileExtra};
use crate::color::Color;
#[cfg(test)]
use crate::color::{Style, Tint};
use crate::tile::{Tile, TileFlags};
use alloc::vec::Vec;
use alpha_blend::BlendMode as SeparableBlendMode;
use alpha_blend::channel::Channel;

impl Grid {
    /// Copies tiles from `src` within `src_rect` to `self` at `(dst_x, dst_y)`
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
}

/// Blends two [`Color`] values using `mode`. [`Color::Default`] preserves the
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
    fn test_grid_blit_preserves_extra() {
        let mut src = Grid::new(2, 2);
        src.write_grapheme(0, 0, 0, "e\u{0301}", Style::default());

        let mut dst = Grid::new(2, 2);
        dst.blit(0, &src, Rect::new(0, 0, 2, 2), 0, 0);
        assert_eq!(dst[Pos::new(0, 0)].glyph, 'e');
        assert_eq!(crate::grid::grapheme_at(&dst, 0, 0, 0), Some("e\u{0301}"));
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

    #[test]
    fn blit_copies_a_whole_wide_char_pair_intact() {
        // The lead-clip and spacer-clip tests above both exercise the `!partner_survived` half of
        // `blit_with`'s wide-pair check; this covers the other half, where `src_rect` includes
        // both halves and neither flag should be stripped.
        let mut src = Grid::new(4, 1);
        src.put_tile(0, (0, 0), Tile::new('\u{4e2d}', Style::default()));

        let mut dst = Grid::new(4, 1);
        dst.blit(0, &src, Rect::new(0, 0, 2, 1), 0, 0);

        assert!(dst[Pos::new(0, 0)].flags().contains(TileFlags::WIDE_CHAR));
        assert!(
            dst[Pos::new(1, 0)]
                .flags()
                .contains(TileFlags::WIDE_CHAR_SPACER)
        );
    }

    #[test]
    fn blit_degrades_a_bare_wide_char_spacer_clipped_by_src_rect() {
        // The spacer twin of `blit_degrades_a_wide_char_pair_clipped_by_src_rect`: `src_rect` can
        // just as easily clip out the lead and leave the spacer, which is equally unrepresentable
        // on its own, so `blit` drops `WIDE_CHAR_SPACER` on the spacer it does copy.
        let mut src = Grid::new(4, 1);
        src.put_tile(0, (0, 0), Tile::new('\u{4e2d}', Style::default()));

        let mut dst = Grid::new(4, 1);
        dst.blit(0, &src, Rect::new(1, 0, 1, 1), 1, 0);

        assert!(
            !dst[Pos::new(1, 0)]
                .flags()
                .contains(TileFlags::WIDE_CHAR_SPACER)
        );
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
