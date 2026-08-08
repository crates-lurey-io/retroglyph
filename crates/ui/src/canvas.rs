//! Sub-cell drawing: a buffer to plot into before there are pixels to posterize.
//!
//! [`retroglyph_core::symbols`] already provides the *quantizers*
//! ([`quantize_half_block`](retroglyph_core::symbols::quantize_half_block) and friends): given a
//! block of raw pixels, they pick the best-matching glyph plus two colors. What they don't
//! provide is anywhere to put those pixels before you have them: a caller drawing a shape,
//! sprite, or heightmap needs a buffer sized in sub-cells, not raw cells, to plot into first.
//! That's what this module adds.
//!
//! [`HalfBlockCanvas`](crate::canvas::HalfBlockCanvas), [`QuadrantCanvas`](crate::canvas::QuadrantCanvas), and
//! [`SextantCanvas`](crate::canvas::SextantCanvas) differ only in their sub-cell layout (1x2, 2x2, and 2x3
//! respectively); each plots full [`Color`](retroglyph_core::color::Color)s and reads a cell back as whichever
//! [`quantize_half_block`](retroglyph_core::symbols::quantize_half_block)/
//! [`quantize_quadrant`](retroglyph_core::symbols::quantize_quadrant)/
//! [`quantize_sextant`](retroglyph_core::symbols::quantize_sextant) picks for it.
//! [`BrailleCanvas`](crate::canvas::BrailleCanvas) is the odd one out: 2x4 dots per cell, but monochrome, since
//! every dot in a braille glyph shares one foreground color, so it plots boolean dots rather than colors and
//! reads a cell back via [`retroglyph_core::symbols::braille::glyph`] instead of a quantizer.
//!
//! A canvas can't choose a cell's glyph as it's plotted: the glyph depends on every sub-cell in
//! that cell, so nothing can be decided until [`cells`](crate::canvas::HalfBlockCanvas::cells) is called after
//! the last plot lands.

use alloc::vec;
use alloc::vec::Vec;

use retroglyph_core::color::Color;
use retroglyph_core::symbols::braille;
use retroglyph_core::symbols::{Glyph, quantize_half_block, quantize_quadrant, quantize_sextant};

/// A color plotted at a sub-cell resolution, read back one cell at a time.
///
/// [`HalfBlockCanvas`], [`QuadrantCanvas`], and [`SextantCanvas`] differ only in their sub-cell
/// layout, so they share this shape: plot into a buffer sized in sub-cells, then iterate cells
/// and let a quantizer choose each cell's glyph and two colors.
#[derive(Debug, Clone)]
struct SubCanvas {
    /// Sub-cell width, i.e. `cols * sub_w`.
    width: usize,
    /// Sub-cell height, i.e. `rows * sub_h`.
    height: usize,
    pixels: Vec<Color>,
    clear: Color,
}

impl SubCanvas {
    fn new(width: usize, height: usize, clear: Color) -> Self {
        Self {
            width,
            height,
            pixels: vec![clear; width * height],
            clear,
        }
    }

    fn clear(&mut self) {
        self.pixels.fill(self.clear);
    }

    fn set(&mut self, x: i32, y: i32, color: Color) {
        if x < 0 || y < 0 {
            return;
        }
        #[allow(clippy::cast_sign_loss)] // x, y >= 0, checked above
        let (x, y) = (x as usize, y as usize);
        if x >= self.width || y >= self.height {
            return;
        }
        self.pixels[y * self.width + x] = color;
    }

    fn get(&self, x: usize, y: usize) -> Color {
        self.pixels
            .get(y * self.width + x)
            .copied()
            .unwrap_or(self.clear)
    }

    /// Resolves a sub-cell to the `(r, g, b)` triple the quantizers expect.
    ///
    /// The fallback matters: a `Color::Default` sub-cell has no intrinsic RGB, and quantizing it
    /// as black would draw a hard black block where the caller meant "nothing here". Resolving
    /// against the canvas' own clear color instead makes an unset sub-cell vanish into the
    /// background, which is what "clear" should mean.
    fn rgb_at(&self, x: usize, y: usize) -> (u8, u8, u8) {
        let fallback = self.clear.resolve_rgb((0, 0, 0));
        self.get(x, y).resolve_rgb(fallback)
    }
}

/// A canvas with 1x2 sub-cells (upper and lower half of each cell).
///
/// The most compatible sub-cell mode: `▀` and `▄` are Unicode Block Elements, supported in
/// essentially every monospace terminal font. Doubles vertical resolution, which is exactly what
/// a character grid is short of, given cells are already about twice as tall as they are wide:
/// net effect, square pixels.
#[derive(Debug, Clone)]
pub struct HalfBlockCanvas {
    canvas: SubCanvas,
    cols: usize,
    rows: usize,
}

impl HalfBlockCanvas {
    /// A canvas covering `cols` x `rows` cells, i.e. `cols` x `rows * 2` plottable points.
    #[must_use]
    pub fn new(cols: u16, rows: u16, clear: Color) -> Self {
        let (cols, rows) = (cols as usize, rows as usize);
        Self {
            canvas: SubCanvas::new(cols, rows * 2, clear),
            cols,
            rows,
        }
    }

    /// Plottable size, in sub-cells.
    #[must_use]
    pub const fn size(&self) -> (usize, usize) {
        (self.canvas.width, self.canvas.height)
    }

    /// Resets every sub-cell to the clear color.
    pub fn clear(&mut self) {
        self.canvas.clear();
    }

    /// Plots one sub-cell. Out-of-bounds coordinates are ignored, so callers can draw shapes that
    /// overhang the canvas without clipping first.
    pub fn plot(&mut self, x: i32, y: i32, color: Color) {
        self.canvas.set(x, y, color);
    }

    /// Yields `(col, row, glyph)` for every cell.
    pub fn cells(&self) -> impl Iterator<Item = (u16, u16, Glyph)> + '_ {
        (0..self.rows).flat_map(move |row| {
            (0..self.cols).map(move |col| {
                let glyph = quantize_half_block([
                    self.canvas.rgb_at(col, row * 2),
                    self.canvas.rgb_at(col, row * 2 + 1),
                ]);
                (
                    u16::try_from(col).unwrap_or(u16::MAX),
                    u16::try_from(row).unwrap_or(u16::MAX),
                    glyph,
                )
            })
        })
    }
}

/// A canvas with 2x2 sub-cells per cell.
///
/// Doubles resolution on both axes over [`HalfBlockCanvas`]. The tradeoff: four sub-cells still
/// share only two colors, so a quadrant cell containing three different colors loses one of them,
/// where a half-block cell containing two keeps both exactly.
#[derive(Debug, Clone)]
pub struct QuadrantCanvas {
    canvas: SubCanvas,
    cols: usize,
    rows: usize,
}

impl QuadrantCanvas {
    /// A canvas covering `cols` x `rows` cells, i.e. `cols * 2` x `rows * 2` plottable points.
    #[must_use]
    pub fn new(cols: u16, rows: u16, clear: Color) -> Self {
        let (cols, rows) = (cols as usize, rows as usize);
        Self {
            canvas: SubCanvas::new(cols * 2, rows * 2, clear),
            cols,
            rows,
        }
    }

    /// Plottable size, in sub-cells.
    #[must_use]
    pub const fn size(&self) -> (usize, usize) {
        (self.canvas.width, self.canvas.height)
    }

    /// Resets every sub-cell to the clear color.
    pub fn clear(&mut self) {
        self.canvas.clear();
    }

    /// Plots one sub-cell. Out-of-bounds coordinates are ignored; see [`HalfBlockCanvas::plot`].
    pub fn plot(&mut self, x: i32, y: i32, color: Color) {
        self.canvas.set(x, y, color);
    }

    /// Yields `(col, row, glyph)` for every cell.
    pub fn cells(&self) -> impl Iterator<Item = (u16, u16, Glyph)> + '_ {
        (0..self.rows).flat_map(move |row| {
            (0..self.cols).map(move |col| {
                let (x, y) = (col * 2, row * 2);
                let glyph = quantize_quadrant([
                    self.canvas.rgb_at(x, y),
                    self.canvas.rgb_at(x + 1, y),
                    self.canvas.rgb_at(x, y + 1),
                    self.canvas.rgb_at(x + 1, y + 1),
                ]);
                (
                    u16::try_from(col).unwrap_or(u16::MAX),
                    u16::try_from(row).unwrap_or(u16::MAX),
                    glyph,
                )
            })
        })
    }
}

/// A canvas with 2x3 sub-cells per cell, using Unicode sextants.
///
/// The highest-resolution *colored* mode: six sub-cells per glyph, still two colors. Needs a font
/// with "Symbols for Legacy Computing" coverage (a 2022 Unicode addition); see
/// [`quantize_sextant`](retroglyph_core::symbols::quantize_sextant)'s own docs for the font
/// caveat and the `FontChain` setup that addresses it on the bundled pixel backends.
#[derive(Debug, Clone)]
pub struct SextantCanvas {
    canvas: SubCanvas,
    cols: usize,
    rows: usize,
}

impl SextantCanvas {
    /// A canvas covering `cols` x `rows` cells, i.e. `cols * 2` x `rows * 3` plottable points.
    #[must_use]
    pub fn new(cols: u16, rows: u16, clear: Color) -> Self {
        let (cols, rows) = (cols as usize, rows as usize);
        Self {
            canvas: SubCanvas::new(cols * 2, rows * 3, clear),
            cols,
            rows,
        }
    }

    /// Plottable size, in sub-cells.
    #[must_use]
    pub const fn size(&self) -> (usize, usize) {
        (self.canvas.width, self.canvas.height)
    }

    /// Resets every sub-cell to the clear color.
    pub fn clear(&mut self) {
        self.canvas.clear();
    }

    /// Plots one sub-cell. Out-of-bounds coordinates are ignored; see [`HalfBlockCanvas::plot`].
    pub fn plot(&mut self, x: i32, y: i32, color: Color) {
        self.canvas.set(x, y, color);
    }

    /// Yields `(col, row, glyph)` for every cell.
    pub fn cells(&self) -> impl Iterator<Item = (u16, u16, Glyph)> + '_ {
        (0..self.rows).flat_map(move |row| {
            (0..self.cols).map(move |col| {
                let (x, y) = (col * 2, row * 3);
                let glyph = quantize_sextant([
                    self.canvas.rgb_at(x, y),
                    self.canvas.rgb_at(x + 1, y),
                    self.canvas.rgb_at(x, y + 1),
                    self.canvas.rgb_at(x + 1, y + 1),
                    self.canvas.rgb_at(x, y + 2),
                    self.canvas.rgb_at(x + 1, y + 2),
                ]);
                (
                    u16::try_from(col).unwrap_or(u16::MAX),
                    u16::try_from(row).unwrap_or(u16::MAX),
                    glyph,
                )
            })
        })
    }
}

/// A monochrome canvas with 2x4 sub-cells per cell, using Unicode Braille Patterns.
///
/// The densest character-grid raster there is: 8 dots per cell, which on a 100x40 grid is a
/// 200x160 bitmap. The catch is in the name of the trade: all 8 dots share one foreground color,
/// so this draws *shapes*, not pictures. Ideal for contour lines, a pixel-accurate outline, or a
/// high-resolution minimap silhouette.
///
/// Dot addressing goes through [`retroglyph_core::symbols::braille::DOTS`] and
/// [`retroglyph_core::symbols::braille::glyph`] rather than a locally re-derived bit table: dot
/// bit positions follow Braille's historical numbering (down the left column, then the right,
/// then the two 8-dot extensions appended underneath), not raster order, and that table already
/// exists in `retroglyph-core`.
#[derive(Debug, Clone)]
pub struct BrailleCanvas {
    cols: usize,
    rows: usize,
    /// One byte of dot bits per cell.
    dots: Vec<u8>,
}

impl BrailleCanvas {
    /// A canvas covering `cols` x `rows` cells, i.e. `cols * 2` x `rows * 4` plottable points.
    #[must_use]
    pub fn new(cols: u16, rows: u16) -> Self {
        let (cols, rows) = (cols as usize, rows as usize);
        Self {
            cols,
            rows,
            dots: vec![0; cols * rows],
        }
    }

    /// Plottable size, in dots.
    #[must_use]
    pub const fn size(&self) -> (usize, usize) {
        (self.cols * 2, self.rows * 4)
    }

    /// Clears every dot.
    pub fn clear(&mut self) {
        self.dots.fill(0);
    }

    /// Sets the dot at `(x, y)`. Out-of-bounds coordinates are ignored; see
    /// [`HalfBlockCanvas::plot`].
    pub fn plot(&mut self, x: i32, y: i32) {
        if let Some((index, mask)) = self.locate(x, y) {
            self.dots[index] |= mask;
        }
    }

    /// Clears the dot at `(x, y)`.
    pub fn unplot(&mut self, x: i32, y: i32) {
        if let Some((index, mask)) = self.locate(x, y) {
            self.dots[index] &= !mask;
        }
    }

    /// Whether the dot at `(x, y)` is set.
    #[must_use]
    pub fn get(&self, x: i32, y: i32) -> bool {
        self.locate(x, y)
            .is_some_and(|(index, mask)| self.dots[index] & mask != 0)
    }

    /// Maps a dot coordinate to its cell index and dot bitmask (a `braille::DOT_*` value, already
    /// a single set bit, not a shift amount), or `None` if out of bounds.
    ///
    /// `braille::DOTS` is indexed `[row][col]` (row = `y % 4`, col = `x % 2`); see its own docs
    /// for why the mask that comes out doesn't follow raster order.
    const fn locate(&self, x: i32, y: i32) -> Option<(usize, u8)> {
        if x < 0 || y < 0 {
            return None;
        }
        #[allow(clippy::cast_sign_loss)] // x, y >= 0, checked above
        let (x, y) = (x as usize, y as usize);
        let (cell_x, cell_y) = (x / 2, y / 4);
        if cell_x >= self.cols || cell_y >= self.rows {
            return None;
        }
        Some((cell_y * self.cols + cell_x, braille::DOTS[y % 4][x % 2]))
    }

    /// Draws a line between two dot coordinates (Bresenham).
    ///
    /// The one drawing primitive worth having built in: contours, outlines, and route overlays
    /// are all lines, and hand-rolling Bresenham per caller is exactly the sort of duplication a
    /// shared crate exists to prevent.
    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let (mut x, mut y) = (x0, y0);
        let mut err = dx + dy;
        loop {
            self.plot(x, y);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// The Braille character for cell `(col, row)`, or [`braille::BLANK`] if out of bounds.
    #[must_use]
    pub fn glyph(&self, col: usize, row: usize) -> char {
        let bits = if col < self.cols && row < self.rows {
            self.dots[row * self.cols + col]
        } else {
            0
        };
        braille::glyph(bits)
    }

    /// Yields `(col, row, glyph)` for every cell.
    pub fn cells(&self) -> impl Iterator<Item = (u16, u16, char)> + '_ {
        (0..self.rows).flat_map(move |row| {
            (0..self.cols).map(move |col| {
                (
                    u16::try_from(col).unwrap_or(u16::MAX),
                    u16::try_from(row).unwrap_or(u16::MAX),
                    self.glyph(col, row),
                )
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;
    use alloc::vec::Vec;

    use super::{BrailleCanvas, Glyph, HalfBlockCanvas, QuadrantCanvas, SextantCanvas, braille};
    use retroglyph_core::color::Color;
    use retroglyph_core::symbols::{HALF_BLOCKS, QUADRANTS};

    const RED: Color = Color::rgb(255, 0, 0);
    const BLUE: Color = Color::rgb(0, 0, 255);
    const BLACK: Color = Color::rgb(0, 0, 0);

    #[test]
    fn half_block_canvas_has_double_vertical_resolution() {
        let canvas = HalfBlockCanvas::new(10, 5, BLACK);
        assert_eq!(canvas.size(), (10, 10));
        assert_eq!(canvas.cells().count(), 50);
    }

    /// Resolves a quantized [`Glyph`] back into its per-sub-cell colors.
    ///
    /// Necessary because a posterizer has two equally correct answers for every cell: `'▀'` with
    /// `(fg, bg)` renders pixel-for-pixel identically to `'▄'` with `(bg, fg)`. Asserting on the
    /// glyph alone would therefore be asserting on an implementation detail; asserting on what
    /// actually gets drawn is both stricter and stable.
    fn resolve(glyph: Glyph, bank: &[char]) -> Vec<Color> {
        let mask = bank
            .iter()
            .position(|&c| c == glyph.ch)
            .expect("glyph must come from its own bank");
        // A bank of 2^n glyphs enumerates every subset of n sub-cells, so the sub-cell count is
        // log2 of the bank length.
        let sub_cells = bank.len().trailing_zeros() as usize;
        (0..sub_cells)
            .map(|i| {
                if mask & (1 << i) != 0 {
                    glyph.fg
                } else {
                    glyph.bg
                }
            })
            .collect()
    }

    #[test]
    fn half_block_renders_the_half_that_was_plotted() {
        let mut canvas = HalfBlockCanvas::new(1, 1, BLACK);
        canvas.plot(0, 0, RED);
        let (_, _, glyph) = canvas.cells().next().expect("one cell");
        assert_eq!(resolve(glyph, &HALF_BLOCKS), vec![RED, BLACK], "top set");

        canvas.clear();
        canvas.plot(0, 1, RED);
        let (_, _, glyph) = canvas.cells().next().expect("one cell");
        assert_eq!(resolve(glyph, &HALF_BLOCKS), vec![BLACK, RED], "bottom set");
    }

    #[test]
    fn quadrant_canvas_has_double_resolution_on_both_axes() {
        let canvas = QuadrantCanvas::new(8, 4, BLACK);
        assert_eq!(canvas.size(), (16, 8));
        assert_eq!(canvas.cells().count(), 32);
    }

    #[test]
    fn quadrant_renders_the_corner_that_was_plotted() {
        for (x, y, index) in [(0, 0, 0), (1, 0, 1), (0, 1, 2), (1, 1, 3)] {
            let mut canvas = QuadrantCanvas::new(1, 1, BLACK);
            canvas.plot(x, y, RED);
            let (_, _, glyph) = canvas.cells().next().expect("one cell");
            let mut expected = vec![BLACK; 4];
            expected[index] = RED;
            assert_eq!(resolve(glyph, &QUADRANTS), expected, "corner ({x}, {y})");
        }
    }

    #[test]
    fn canvases_preserve_plotted_colors_exactly() {
        // A two-color cell must survive quantization unchanged: the whole reason to use
        // half-blocks over a shade ramp is that they are exact.
        let mut canvas = HalfBlockCanvas::new(1, 1, BLACK);
        canvas.plot(0, 0, RED);
        canvas.plot(0, 1, BLUE);
        let (_, _, glyph) = canvas.cells().next().expect("one cell");
        assert_eq!(resolve(glyph, &HALF_BLOCKS), vec![RED, BLUE]);
    }

    #[test]
    fn sextant_canvas_is_two_by_three() {
        let canvas = SextantCanvas::new(6, 3, BLACK);
        assert_eq!(canvas.size(), (12, 9));
        assert_eq!(canvas.cells().count(), 18);
    }

    #[test]
    fn canvases_ignore_out_of_bounds_plots() {
        let mut half = HalfBlockCanvas::new(2, 2, BLACK);
        let mut quad = QuadrantCanvas::new(2, 2, BLACK);
        let mut sext = SextantCanvas::new(2, 2, BLACK);
        for (x, y) in [(-1, 0), (0, -1), (999, 0), (0, 999), (-999, -999)] {
            half.plot(x, y, RED);
            quad.plot(x, y, RED);
            sext.plot(x, y, RED);
        }
        // Nothing was written, so every cell is still the clear color.
        assert!(half.cells().all(|(_, _, g)| g.ch == ' ' || g.fg == BLACK));
        assert!(quad.cells().all(|(_, _, g)| g.ch == ' ' || g.fg == BLACK));
        assert!(sext.cells().all(|(_, _, g)| g.ch == ' ' || g.fg == BLACK));
    }

    #[test]
    fn clearing_a_canvas_erases_everything() {
        let mut canvas = QuadrantCanvas::new(2, 2, BLACK);
        canvas.plot(0, 0, RED);
        canvas.clear();
        let glyphs: Vec<char> = canvas.cells().map(|(_, _, g)| g.ch).collect();
        assert!(glyphs.iter().all(|&c| c == ' '), "left over: {glyphs:?}");
    }

    // ── Braille ─────────────────────────────────────────────────────────────

    #[test]
    fn braille_canvas_is_two_by_four() {
        let canvas = BrailleCanvas::new(20, 10);
        assert_eq!(canvas.size(), (40, 40));
        assert_eq!(canvas.cells().count(), 200);
    }

    #[test]
    fn braille_plot_round_trips_through_get() {
        let mut canvas = BrailleCanvas::new(4, 2);
        for y in 0..8 {
            for x in 0..8 {
                assert!(!canvas.get(x, y));
                canvas.plot(x, y);
                assert!(canvas.get(x, y), "dot ({x}, {y}) did not stick");
                canvas.unplot(x, y);
                assert!(!canvas.get(x, y), "dot ({x}, {y}) did not clear");
            }
        }
    }

    #[test]
    fn braille_glyph_is_blank_when_empty_and_full_when_saturated() {
        let mut canvas = BrailleCanvas::new(1, 1);
        assert_eq!(canvas.glyph(0, 0), braille::BLANK);
        for y in 0..4 {
            for x in 0..2 {
                canvas.plot(x, y);
            }
        }
        assert_eq!(canvas.glyph(0, 0), '\u{28FF}', "all 8 dots set");
    }

    #[test]
    fn braille_glyphs_stay_inside_the_unicode_block() {
        let mut canvas = BrailleCanvas::new(3, 3);
        canvas.line(0, 0, 5, 11);
        for (_, _, glyph) in canvas.cells() {
            let code = glyph as u32;
            assert!(
                (0x2800..0x2900).contains(&code),
                "{glyph:?} is outside the Braille block"
            );
        }
    }

    #[test]
    fn braille_out_of_bounds_is_ignored_not_panicked() {
        let mut canvas = BrailleCanvas::new(2, 2);
        for (x, y) in [(-1, 0), (0, -1), (999, 0), (0, 999)] {
            canvas.plot(x, y);
            canvas.unplot(x, y);
            assert!(!canvas.get(x, y));
        }
        assert!(canvas.cells().all(|(_, _, g)| g == braille::BLANK));
    }

    /// The canary this module exists to fix: dot bit positions come from
    /// `retroglyph_core::symbols::braille::DOTS`, not a locally re-derived table, so this stays
    /// consistent with that module by construction rather than by convention.
    #[test]
    fn dot_bit_positions_match_symbols_braille_dots() {
        let mut canvas = BrailleCanvas::new(1, 1);
        for y in 0..4u8 {
            for x in 0..2u8 {
                canvas.plot(i32::from(x), i32::from(y));
                let bit = braille::DOTS[y as usize][x as usize];
                assert_eq!(canvas.glyph(0, 0), braille::glyph(bit), "dot ({x}, {y})");
                canvas.unplot(i32::from(x), i32::from(y));
            }
        }
    }
}
