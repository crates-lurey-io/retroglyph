//! Cell and surface pixel geometry shared by the graphical backends.

/// The pixel geometry of a fixed cell grid: a glyph size and an integer scale.
///
/// [`Presenter::cell_size`](crate::Presenter::cell_size)'s contract -- physical pixels,
/// `glyph x scale`, never DPI-auto-scaled -- had no code embodiment: each graphical backend
/// re-derived `glyph_w * scale` (and `cols * cell_w` for the surface) on its own, in slightly
/// different integer types, so the shared rule could drift. This is that rule as one small,
/// `const`, testable value type. A backend stores one and returns [`cell_size`](Self::cell_size)
/// from `Presenter::cell_size`.
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
}

#[cfg(test)]
mod tests {
    use super::CellGeometry;

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
}
