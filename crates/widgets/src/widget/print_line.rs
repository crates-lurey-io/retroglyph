//! [`PrintLine`]: a single styled [`Line`].
use retroglyph_core::text::{Line, width as measured_width};

use super::Widget;
use crate::Align;
use crate::Surface;
use crate::text::draw_clipped;

/// A [`Line`], drawn on the first row of the area it's rendered into and
/// clipped to `area.width()` columns. Only the first row is used.
///
/// `align` defaults to [`Align::Left`] (drawn at the left edge); set it with
/// [`PrintLine::align`] to right-align or center the whole line's spans as a
/// unit within `area.width()` columns.
///
/// # Examples
///
/// ```
/// use retroglyph_core::backend::Headless;
/// use retroglyph_core::text::Line;
/// use retroglyph_core::Terminal;
/// use retroglyph_widgets::{PrintLine, Widget};
///
/// let mut term = Terminal::new(Headless::new(20, 1));
/// let line = Line::raw("hello");
/// term.draw(|surface| {
///     PrintLine::new(&line).render(surface);
/// })
/// .unwrap();
/// ```
#[derive(Clone, Copy, Debug)]
pub struct PrintLine<'a> {
    line: &'a Line,
    align: Align,
}

impl<'a> PrintLine<'a> {
    /// Print `line`, left-aligned and clipped to whatever width it's rendered
    /// at.
    #[must_use]
    pub const fn new(line: &'a Line) -> Self {
        Self {
            line,
            align: Align::Left,
        }
    }

    /// Set how the line's spans are aligned, as a unit, within `area.width()`
    /// columns.
    #[must_use]
    pub const fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }
}

impl Widget for PrintLine<'_> {
    fn render(&self, surface: &mut Surface<'_>) {
        let max_width = surface.width();
        let right = max_width;
        // Align the whole line as a unit: sum the spans' display widths
        // (clamped to the area) and offset the start column accordingly.
        // `measured_width` already saturates each span at `u16::MAX`, and `saturating_add` keeps
        // the running total from overflowing too.
        let line_width = self
            .line
            .spans
            .iter()
            .fold(0u16, |acc, s| {
                acc.saturating_add(measured_width(&s.content))
            })
            .min(max_width);
        let mut x = self.align.offset(max_width, line_width);
        for span in &self.line.spans {
            if x >= right {
                break;
            }
            let remaining = right - x;
            let text_w = draw_clipped(
                surface,
                (x, 0),
                remaining,
                &span.content,
                Align::Left,
                span.style,
            );
            x += text_w;
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::String;
    use alloc::vec;

    use retroglyph_core::text::Span;
    use retroglyph_core::{Grid, Pos, Rect};

    use super::*;

    #[test]
    fn prints_every_span() {
        let line = Line::from(vec![Span::raw("hi "), Span::raw("there")]);
        let area = Rect::new(0, 0, 20, 1);
        let mut grid = Grid::new(20, 1);
        PrintLine::new(&line).render(&mut Surface::new(&mut grid, area, 0));

        let row: String = (0..20).map(|x| grid[Pos::new(x, 0)].glyph()).collect();
        assert!(row.starts_with("hi there"));
    }

    #[test]
    fn right_align_places_the_whole_line_against_the_right_edge() {
        let line = Line::from(vec![Span::raw("hi "), Span::raw("there")]);
        let area = Rect::new(0, 0, 20, 1);
        let mut grid = Grid::new(20, 1);
        PrintLine::new(&line)
            .align(Align::Right)
            .render(&mut Surface::new(&mut grid, area, 0));

        // "hi there" is 8 cols; right-aligned in 20 it ends at column 19.
        let row: String = (0..20).map(|x| grid[Pos::new(x, 0)].glyph()).collect();
        assert!(row.ends_with("hi there"), "row was {row:?}");
    }

    #[test]
    fn clips_to_max_width() {
        let line = Line::raw("a much longer message than fits");
        let area = Rect::new(0, 0, 5, 1);
        let mut grid = Grid::new(5, 1);
        PrintLine::new(&line).render(&mut Surface::new(&mut grid, area, 0));

        // "a much longer..." clipped to 5 columns is "a muc".
        assert_eq!(grid[Pos::new(4, 0)].glyph(), 'c');
    }
}
