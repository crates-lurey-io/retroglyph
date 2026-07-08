//! [`PrintLine`]: a single styled [`Line`].
use retroglyph_core::text::Line;
use retroglyph_core::{Backend, Rect, Terminal};

use super::Widget;
use crate::text::truncate as truncate_to_cols;

/// A [`Line`], drawn at the top-left of the area it's rendered into and
/// clipped to `area.width()` columns. Only the first row is used.
#[derive(Clone, Copy, Debug)]
pub struct PrintLine<'a> {
    line: &'a Line,
}

impl<'a> PrintLine<'a> {
    /// Print `line`, clipped to whatever width it's rendered at.
    #[must_use]
    pub const fn new(line: &'a Line) -> Self {
        Self { line }
    }
}

impl<B: Backend> Widget<B> for PrintLine<'_> {
    fn render(self, area: Rect, term: &mut Terminal<B>) {
        let max_width = area.width();
        let mut x = area.left();
        for span in &self.line.spans {
            if x >= area.left() + max_width {
                break;
            }
            let remaining = (area.left() + max_width - x) as usize;
            let text = truncate_to_cols(&span.content, remaining);
            term.reset_style()
                .fg(span.style.foreground())
                .bg(span.style.background())
                .modifier(span.style.modifiers());
            term.print(x, area.top(), &text);
            x += text.len() as u16;
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
    fn clips_to_max_width() {
        let line = Line::raw("a much longer message than fits");
        let area = Rect::new(0, 0, 5, 1);
        let mut term = Terminal::new(Headless::new(5, 1));
        PrintLine::new(&line).render(area, &mut term);

        // "a much longer..." clipped to 5 columns is "a muc".
        assert_eq!(term.grid().get(4, 0).glyph(), 'c');
    }
}
