//! [`fill_rect`]. The box-drawing codepoints previously here moved to
//! [`retroglyph_core::symbols::border`], reachable by any crate rather than just this one.

use retroglyph_core::color::Style;
use retroglyph_core::grid::Rect;

use crate::Surface;

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
    use retroglyph_core::grid::{Grid, Pos};

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
                assert_eq!(grid[Pos::new(x, y)].glyph(), '#');
            }
        }
        // Untouched outside the rect.
        assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
    }
}
