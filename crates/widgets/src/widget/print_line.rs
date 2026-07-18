//! [`PrintLine`]: a single styled [`Line`].
use retroglyph_core::text::Line;
use retroglyph_core::{Backend, Rect, Terminal};
use unicode_width::UnicodeWidthStr;

use super::Widget;
use crate::Align;
use crate::text::truncate as truncate_to_cols;

/// A [`Line`], drawn on the first row of the area it's rendered into and
/// clipped to `area.width()` columns. Only the first row is used.
///
/// `align` defaults to [`Align::Left`] (drawn at the left edge); set it with
/// [`PrintLine::align`] to right-align or center the whole line's spans as a
/// unit within `area.width()` columns.
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

impl<B: Backend> Widget<B> for PrintLine<'_> {
    fn render(self, area: Rect, term: &mut Terminal<B>) {
        let max_width = area.width();
        let right = area.left() + max_width;
        // Align the whole line as a unit: sum the spans' display widths
        // (clamped to the area) and offset the start column accordingly.
        let line_width = self
            .line
            .spans
            .iter()
            .fold(0u16, |acc, s| acc.saturating_add(s.content.width() as u16))
            .min(max_width);
        let mut x = area.left() + self.align.offset(max_width, line_width);
        for span in &self.line.spans {
            if x >= right {
                break;
            }
            let remaining = (right - x) as usize;
            let text = truncate_to_cols(&span.content, remaining);
            term.reset_style()
                .fg(span.style.foreground())
                .bg(span.style.background());
            term.print(x, area.top(), &text);
            x += text.width() as u16;
        }
        term.reset_style();
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::{Headless, text::Span};

    use super::*;

    #[test]
    fn prints_every_span() {
        let line = Line::from(vec![Span::raw("hi "), Span::raw("there")]);
        let area = Rect::new(0, 0, 20, 1);
        let mut term = Terminal::new(Headless::new(20, 1));
        PrintLine::new(&line).render(area, &mut term);

        let row: String = (0..20).map(|x| term.grid().get(x, 0).glyph()).collect();
        assert!(row.starts_with("hi there"));
    }

    #[test]
    fn right_align_places_the_whole_line_against_the_right_edge() {
        let line = Line::from(vec![Span::raw("hi "), Span::raw("there")]);
        let area = Rect::new(0, 0, 20, 1);
        let mut term = Terminal::new(Headless::new(20, 1));
        PrintLine::new(&line)
            .align(Align::Right)
            .render(area, &mut term);

        // "hi there" is 8 cols; right-aligned in 20 it ends at column 19.
        let row: String = (0..20).map(|x| term.grid().get(x, 0).glyph()).collect();
        assert!(row.ends_with("hi there"), "row was {row:?}");
    }

    #[test]
    fn clips_to_max_width() {
        let line = Line::raw("a much longer message than fits");
        let area = Rect::new(0, 0, 5, 1);
        let mut term = Terminal::new(Headless::new(5, 1));
        PrintLine::new(&line).render(area, &mut term);

        // "a much longer..." clipped to 5 columns is "a muc".
        assert_eq!(term.grid().get(4, 0).glyph(), 'c');
    }
}
