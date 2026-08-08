//! Private geometry and single-cell write helpers shared by the [`draw`](super) submodules.

#[cfg(feature = "egc")]
use crate::color::Style;
use crate::color::Tint;

use super::Surface;

mod cells;
mod spans;
mod text;

impl Surface<'_> {
    /// Shifts `(x, y)` by this surface's translate offset (see [`translate`](Self::translate)),
    /// returning the coordinate to actually write at if the shift still lands inside this
    /// surface's own clip, or `None` otherwise.
    ///
    /// The subtracted result is a coordinate local to this surface's area (`(0, 0)` is the
    /// area's own top-left, matching [`put_signed`](Self::put_signed)'s convention), not an
    /// absolute grid coordinate: a local check against `(0, 0)..(width, height)` here, followed
    /// by re-adding [`area`](Self::area)'s own top-left, so a clipped area that does not itself
    /// start at grid `(0, 0)` (e.g. a scrolling-camera widget's `clip_translate`-based
    /// `surface` method) still resolves to the right absolute cell. The result is then checked
    /// against [`clip_rect`](Self::clip_rect), not `area`, since the clip, never the area, is
    /// what decides whether a write lands.
    pub(super) fn shift(&self, x: u16, y: u16) -> Option<(u16, u16)> {
        let sx = i32::from(x).checked_sub(self.origin_offset.0)?;
        let sy = i32::from(y).checked_sub(self.origin_offset.1)?;
        if sx < 0 || sy < 0 {
            return None;
        }
        let sx = u16::try_from(sx).ok()?;
        let sy = u16::try_from(sy).ok()?;
        if sx >= self.area.width() || sy >= self.area.height() {
            return None;
        }
        let gx = self.area.left() + sx;
        let gy = self.area.top() + sy;
        self.clip.contains(gx, gy).then_some((gx, gy))
    }

    /// The exclusive right column, in this surface's own (possibly translated) coordinate space,
    /// past which `print`/`print_line` stop a row: they wrap onto the next row, or skip the
    /// remaining spans, once the cursor reaches it.
    ///
    /// `shift` subtracts `origin_offset` from every incoming coordinate before bounds-checking it,
    /// so the cursor the text writers advance lives in that shifted space. The threshold has to
    /// live there too, or a translated surface (any `Camera::surface`, or a plain `translate`)
    /// wraps or skips early by exactly the offset. The result is `i64` because folding the offset
    /// back into a `u16` clip edge can land outside the `u16` range in either direction.
    pub(super) fn wrap_right(&self) -> i64 {
        i64::from(self.clip.right()) - i64::from(self.area.left()) + i64::from(self.origin_offset.0)
    }

    /// Applies this surface's tint to the cell just written at `(x, y)`.
    ///
    /// Called after a write rather than as part of one, because a glyph write drops whatever
    /// tint the cell held (see [`Grid::set_tint`]); doing it in the other order would erase the
    /// tint being applied. Untinted surfaces skip the call entirely, so the ordinary text path
    /// never touches the side table.
    pub(super) fn apply_tint(&mut self, x: u16, y: u16) {
        if self.tint != Tint::None {
            self.grid.set_tint(self.layer, x, y, self.tint);
        }
    }

    /// Writes `grapheme` (already a single extended grapheme cluster) at `(x, y)`. A no-op if
    /// out of this surface's clip.
    ///
    /// A 2-column grapheme also needs its spacer cell (`x + 1`) inside the clip: `shift` only
    /// checks the primary cell, and `Grid::write_grapheme` only refuses the spacer at the
    /// *grid*'s own edge, not the clip's, so without this the spacer would land one column past
    /// the clip. Refusing the whole write here (rather than writing a primary cell with no
    /// spacer) matches [`span_fits`](Self::span_fits)'s reasoning: a footprint half outside the
    /// clip would reserve a cell the caller does not own.
    #[cfg(feature = "egc")]
    pub(super) fn put_grapheme(&mut self, x: u16, y: u16, grapheme: &str, style: Style) {
        let Some((x, y)) = self.shift(x, y) else {
            return;
        };
        self.write_grapheme_at(x, y, grapheme, style);
    }

    /// Writes `grapheme` at the already-*absolute* grid coordinate `(x, y)` (post-[`shift`],
    /// or an equivalent translation a caller had to do by hand): the width-2 spacer-in-clip
    /// check, [`Grid::write_grapheme`]'s wide-char bookkeeping, and this surface's tint.
    ///
    /// Shared by [`put_grapheme`](Self::put_grapheme) and [`put_signed`](Self::put_signed),
    /// which cannot just call `put_grapheme` with its own local coordinates: `put_signed`
    /// already subtracts `origin_offset` itself (see its doc), so routing through `shift` again
    /// would subtract it twice.
    ///
    /// Returns whether the write actually landed, so a caller that also needs to touch the
    /// written tile afterward (e.g. [`put_offset`](Self::put_offset) setting a pixel offset)
    /// can tell a refused write apart from a successful one instead of blindly poking whatever
    /// tile is already at `(x, y)`. [`Grid::write_grapheme`] can refuse on its own (e.g. `(x, y)`
    /// outside the grid, reachable when this surface's clip/area is wider than the grid itself),
    /// distinct from the clip check above, so `apply_tint` is gated on its own `bool` too rather
    /// than assumed to always land once `wide_spacer_fits` passes.
    #[cfg(feature = "egc")]
    pub(super) fn write_grapheme_at(
        &mut self,
        x: u16,
        y: u16,
        grapheme: &str,
        style: Style,
    ) -> bool {
        use unicode_width::UnicodeWidthStr;

        if !self.wide_spacer_fits(x, y, grapheme.width()) {
            return false;
        }
        let wrote = self.grid.write_grapheme(self.layer, x, y, grapheme, style).is_some();
        if wrote {
            self.apply_tint(x, y);
        }
        wrote
    }

    /// `true` unless `width` is 2 and the spacer cell it would need at `x + 1` falls outside
    /// this surface's clip.
    ///
    /// [`Grid::put_tile`]/[`Grid::write_grapheme`] only refuse a wide write at the *grid*'s own
    /// right edge, not the clip's, so every wide write site (both the `egc` grapheme path via
    /// [`write_grapheme_at`](Self::write_grapheme_at) and the plain-`char` path in
    /// [`put`](Self::put)/[`put_signed`](Self::put_signed)/[`put_offset`](Self::put_offset))
    /// calls this first: without it, a clip narrower than the surface's own area would let a
    /// wide glyph's spacer land one column past the clip, silently overwriting whatever is
    /// there.
    pub(super) fn wide_spacer_fits(&self, x: u16, y: u16, width: usize) -> bool {
        width != 2 || self.clip.contains(x.saturating_add(1), y)
    }
}
