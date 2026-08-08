//! Cell and surface pixel geometry shared by the graphical backends.

use retroglyph_core::grid::Pos;

/// The pixel geometry of a fixed cell grid: a glyph size and an integer scale.
///
/// The single code embodiment of [`Presenter::cell_size`](crate::presenter::Presenter::cell_size)'s
/// contract: physical pixels, `glyph x scale`, never DPI-auto-scaled.
///
/// Every graphical backend stores one of these and returns [`cell_size`](Self::cell_size) from
/// `Presenter::cell_size`, rather than re-deriving `glyph_w * scale` (and `cols * cell_w` for the
/// surface) per backend in its own integer types, which lets the shared rule drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellGeometry {
    /// Glyph width in unscaled font pixels.
    pub glyph_w: u8,
    /// Glyph height in unscaled font pixels.
    pub glyph_h: u8,
    /// Integer pixel scale: each glyph pixel becomes a `scale x scale` block of physical pixels.
    pub scale: u16,
}

impl CellGeometry {
    /// A geometry for `glyph_w x glyph_h` glyphs drawn at integer `scale`.
    #[must_use]
    pub const fn new(glyph_w: u8, glyph_h: u8, scale: u16) -> Self {
        Self {
            glyph_w,
            glyph_h,
            scale,
        }
    }

    /// Cell size in physical pixels: `(glyph_w * scale, glyph_h * scale)`.
    ///
    /// The single embodiment of `Presenter::cell_size`'s "physical pixels, glyph x scale" contract.
    #[must_use]
    pub const fn cell_size(&self) -> (u32, u32) {
        // `as` (not `u32::from`) because this is a `const fn` and `From` isn't const-callable; both
        // casts are lossless widenings (u8/u16 -> u32).
        (
            self.glyph_w as u32 * self.scale as u32,
            self.glyph_h as u32 * self.scale as u32,
        )
    }

    /// Surface size in physical pixels for a `cols x rows` grid: `(cols * cell_w, rows * cell_h)`.
    #[must_use]
    pub const fn surface_size(&self, cols: u16, rows: u16) -> (u32, u32) {
        let (cell_w, cell_h) = self.cell_size();
        (cols as u32 * cell_w, rows as u32 * cell_h)
    }

    /// Converts physical pixel coordinates to a grid cell [`Pos`], using this geometry's
    /// [`cell_size`](Self::cell_size).
    ///
    /// Clamps to `u16::MAX` so out-of-bounds cursor positions (negative or extremely large)
    /// don't panic: the caller is responsible for bounds-checking against the terminal size.
    #[must_use]
    pub fn pixel_to_cell(&self, x: f64, y: f64) -> Pos {
        let (cell_w, cell_h) = self.cell_size();
        Pos {
            x: pixel_to_cell_axis(x, cell_w),
            y: pixel_to_cell_axis(y, cell_h),
        }
    }
}

/// Divides one clamped, non-negative pixel axis by a cell dimension, saturating to `u16::MAX`.
///
/// Shared by [`CellGeometry::pixel_to_cell`] and
/// [`translate_pixel_to_cell`](crate::winit::translate::translate_pixel_to_cell) so the
/// clamp/divide/saturate rule lives in exactly one place.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(crate) fn pixel_to_cell_axis(px: f64, cell: u32) -> u16 {
    // .max(0.0) guards against negatives before the f64→u32 cast.
    // .min(u16::MAX as u32) guarantees the u32→u16 cast never truncates.
    let index =
        u32::checked_div(px.max(0.0) as u32, cell).map_or(0, |v| v.min(u32::from(u16::MAX)));
    u16::try_from(index).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::CellGeometry;
    use retroglyph_core::grid::Pos;

    #[test]
    fn cell_size_is_glyph_times_scale() {
        assert_eq!(CellGeometry::new(8, 16, 1).cell_size(), (8, 16));
        assert_eq!(CellGeometry::new(8, 16, 2).cell_size(), (16, 32));
        assert_eq!(CellGeometry::new(6, 12, 3).cell_size(), (18, 36));
    }

    #[test]
    fn surface_size_is_grid_times_cell() {
        // 80x25 grid of 8x16 cells at scale 1, then scale 2.
        assert_eq!(CellGeometry::new(8, 16, 1).surface_size(80, 25), (640, 400));
        assert_eq!(
            CellGeometry::new(8, 16, 2).surface_size(80, 25),
            (1280, 800)
        );
    }

    #[test]
    fn zero_grid_is_zero_surface() {
        assert_eq!(CellGeometry::new(8, 16, 2).surface_size(0, 0), (0, 0));
    }

    // ── pixel_to_cell ─────────────────────────────────────────────────────────

    #[test]
    fn pixel_to_cell_basic() {
        // 8×16 cells: pixel (20, 48) → col 2, row 3
        let geometry = CellGeometry::new(8, 16, 1);
        assert_eq!(geometry.pixel_to_cell(20.0, 48.0), Pos { x: 2, y: 3 });
    }

    #[test]
    fn pixel_to_cell_origin() {
        let geometry = CellGeometry::new(8, 16, 1);
        assert_eq!(geometry.pixel_to_cell(0.0, 0.0), Pos { x: 0, y: 0 });
    }

    #[test]
    fn pixel_to_cell_negative_coords_clamp_to_zero() {
        // Cursor briefly outside the window can produce negative physical coords.
        let geometry = CellGeometry::new(8, 16, 1);
        assert_eq!(geometry.pixel_to_cell(-5.0, -10.0), Pos { x: 0, y: 0 });
    }

    #[test]
    fn pixel_to_cell_zero_cell_size_returns_origin() {
        // Degenerate case: glyph size 0 (backend not yet initialised with a valid font).
        let geometry = CellGeometry::new(0, 0, 1);
        assert_eq!(geometry.pixel_to_cell(100.0, 200.0), Pos { x: 0, y: 0 });
    }

    #[test]
    fn pixel_to_cell_accounts_for_scale() {
        // 8×16 glyphs at scale 2 → 16×32 cells: pixel (20, 48) → col 1, row 1.
        let geometry = CellGeometry::new(8, 16, 2);
        assert_eq!(geometry.pixel_to_cell(20.0, 48.0), Pos { x: 1, y: 1 });
    }

    #[test]
    fn pixel_to_cell_clamps_to_u16_max() {
        // A huge pixel coordinate must not overflow u16.
        let geometry = CellGeometry::new(1, 1, 1);
        assert_eq!(
            geometry.pixel_to_cell(f64::from(u32::MAX), f64::from(u32::MAX)),
            Pos {
                x: u16::MAX,
                y: u16::MAX
            }
        );
    }
}
