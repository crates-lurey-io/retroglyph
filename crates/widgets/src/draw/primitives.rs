//! [`fill_rect`], plus the box-drawing codepoints shared by
//! [`widget::BoxBorder`](crate::widget::BoxBorder) and [`style::BoxStyle`](crate::style::BoxStyle).

use retroglyph_core::Rect;
use retroglyph_core::Style;

use crate::Surface;

// ── Box-drawing codepoints (single-line) ─────────────────────────────────────

// `pub(crate)`, not private: reused by `style.rs`'s `BoxStyle` border
// rendering and `widget::BoxBorder`, but not part of the public API. The
// enclosing `primitives` module is itself `pub(crate)` for the same reason,
// which is what makes clippy's redundant-pub-crate lint fire here -- allowed
// rather than restructured, since `pub(crate)` is the accurate, intentional
// visibility.
#[allow(clippy::redundant_pub_crate)]
pub(crate) const TL: char = '┌'; // top-left corner
#[allow(clippy::redundant_pub_crate)]
pub(crate) const TR: char = '┐'; // top-right corner
#[allow(clippy::redundant_pub_crate)]
pub(crate) const BL: char = '└'; // bottom-left corner
#[allow(clippy::redundant_pub_crate)]
pub(crate) const BR: char = '┘'; // bottom-right corner
#[allow(clippy::redundant_pub_crate)]
pub(crate) const H: char = '─'; // horizontal bar
#[allow(clippy::redundant_pub_crate)]
pub(crate) const V: char = '│'; // vertical bar

/// Fill `rect` with `ch` in the given `style`.
///
/// The entire rectangle including corners is overwritten. Kept as a plain
/// function rather than a widget: there's no configuration to build up
/// beyond the two arguments already here, and it's a building block other
/// widgets (`Panel`, `Table`, `Scrollbar`) call directly.
pub fn fill_rect(surface: &mut Surface<'_>, rect: Rect, ch: char, style: Style) {
    surface.fill_rect(rect, ch, style);
}

#[cfg(test)]
mod tests {
    use retroglyph_core::Grid;

    use super::*;

    #[test]
    fn fill_rect_overwrites_the_whole_rectangle() {
        let area = Rect::new(0, 0, 6, 4);
        let rect = Rect::new(1, 1, 4, 2);
        let mut grid = Grid::new(6, 4);
        fill_rect(
            &mut Surface::new(&mut grid, area, 0),
            rect,
            '#',
            Style::new(),
        );

        for y in 1..3 {
            for x in 1..5 {
                assert_eq!(grid.get(x, y).glyph(), '#');
            }
        }
        // Untouched outside the rect.
        assert_eq!(grid.get(0, 0).glyph(), ' ');
    }
}
