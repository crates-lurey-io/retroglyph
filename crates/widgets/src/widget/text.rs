//! [`Text`]: a single line of plain text in one [`Style`].
use retroglyph_core::{Backend, Rect, Style, Terminal};

use super::Widget;
use crate::text::truncate as truncate_to_cols;

/// A single line of text in one [`Style`], clipped (not wrapped) to
/// `area.width()` columns. Only the first row of `area` is used.
///
/// The plain-content cousin of [`PrintLine`](super::PrintLine) (which
/// prints a multi-span [`Line`](retroglyph_core::text::Line), for mixed
/// styling within one line) and [`Paragraph`](super::Paragraph) (which
/// word-wraps across multiple lines, and needs the `egc` feature): reach
/// for `Text` for a single already-one-line label or readout in a single
/// style, with no wrapping and no per-span styling. `style` defaults to
/// [`Style::new()`]; set it with [`Text::style`].
#[derive(Clone, Copy, Debug)]
pub struct Text<'a> {
    content: &'a str,
    style: Style,
}

impl<'a> Text<'a> {
    /// A line of `content` in the default style.
    #[must_use]
    pub fn new(content: &'a str) -> Self {
        Self {
            content,
            style: Style::new(),
        }
    }

    /// Set the text's style.
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

impl<B: Backend> Widget<B> for Text<'_> {
    fn render(self, area: Rect, term: &mut Terminal<B>) {
        if area.width() == 0 {
            return;
        }
        let text = truncate_to_cols(self.content, area.width_usize());
        term.reset_style()
            .fg(self.style.foreground())
            .bg(self.style.background());
        term.print(area.left(), area.top(), &text);
        term.reset_style();
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::{Color, Headless};

    use super::*;

    #[test]
    fn prints_the_content_in_the_given_style() {
        let area = Rect::new(0, 0, 10, 1);
        let mut term = Terminal::new(Headless::new(10, 1));
        Text::new("hi")
            .style(Style::new().fg(Color::WHITE))
            .render(area, &mut term);

        assert_eq!(term.grid().get(0, 0).glyph(), 'h');
        assert_eq!(term.grid().get(1, 0).glyph(), 'i');
        assert_eq!(term.grid().get(0, 0).style().foreground(), Color::WHITE);
    }

    #[test]
    fn clips_to_area_width() {
        let area = Rect::new(0, 0, 5, 1);
        let mut term = Terminal::new(Headless::new(5, 1));
        Text::new("a much longer message than fits").render(area, &mut term);

        assert_eq!(term.grid().get(4, 0).glyph(), 'c'); // "a muc"
    }

    #[test]
    fn zero_width_is_a_no_op() {
        let area = Rect::new(0, 0, 0, 1);
        let mut term = Terminal::new(Headless::new(1, 1));
        Text::new("hi").render(area, &mut term);
        assert_eq!(term.grid().get(0, 0).glyph(), ' ');
    }
}
