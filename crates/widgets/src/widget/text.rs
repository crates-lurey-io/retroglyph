//! [`Text`]: a single line of plain text in one [`Style`].
use retroglyph_core::{Rect, Style};
use unicode_width::UnicodeWidthStr;

use super::Widget;
use crate::Align;
use crate::Surface;
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
/// [`Style::new()`] and `align` to [`Align::Left`]; set them with
/// [`Text::style`]/[`Text::align`].
///
/// # Examples
///
/// ```
/// use retroglyph_core::{Grid, Rect};
/// use retroglyph_widgets::{Align, Surface, Text, Widget};
///
/// let area = Rect::new(0, 0, 10, 1);
/// let mut grid = Grid::new(10, 1);
/// Text::new("OK")
///     .align(Align::Right)
///     .render(area, &mut Surface::new(&mut grid, area, 0));
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Text<'a> {
    content: &'a str,
    style: Style,
    align: Align,
}

impl<'a> Text<'a> {
    /// A line of `content` in the default style, left-aligned.
    #[must_use]
    pub fn new(content: &'a str) -> Self {
        Self {
            content,
            style: Style::new(),
            align: Align::Left,
        }
    }

    /// Set the text's style.
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set how the line is aligned within `area.width()` columns.
    #[must_use]
    pub const fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }
}

impl Widget for Text<'_> {
    fn render(&self, area: Rect, surface: &mut Surface<'_>) {
        if area.width() == 0 {
            return;
        }
        let text = truncate_to_cols(self.content, area.width_usize());
        let x = area.left() + self.align.offset(area.width(), text.width() as u16);
        surface.print(x, area.top(), text, self.style);
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::{Color, Grid, Pos};

    use super::*;

    #[test]
    fn prints_the_content_in_the_given_style() {
        let area = Rect::new(0, 0, 10, 1);
        let mut grid = Grid::new(10, 1);
        Text::new("hi")
            .style(Style::new().fg(Color::WHITE))
            .render(area, &mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'h');
        assert_eq!(grid[Pos::new(1, 0)].glyph(), 'i');
        assert_eq!(grid[Pos::new(0, 0)].style().foreground(), Color::WHITE);
    }

    #[test]
    fn clips_to_area_width() {
        let area = Rect::new(0, 0, 5, 1);
        let mut grid = Grid::new(5, 1);
        Text::new("a much longer message than fits")
            .render(area, &mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(4, 0)].glyph(), 'c'); // "a muc"
    }

    #[test]
    fn right_align_places_text_against_the_right_edge() {
        let area = Rect::new(0, 0, 10, 1);
        let mut grid = Grid::new(10, 1);
        Text::new("hi")
            .align(Align::Right)
            .render(area, &mut Surface::new(&mut grid, area, 0));

        // "hi" (2 cols) in 10 cols, right-aligned: starts at column 8.
        assert_eq!(grid[Pos::new(8, 0)].glyph(), 'h');
        assert_eq!(grid[Pos::new(9, 0)].glyph(), 'i');
        assert_eq!(grid[Pos::new(7, 0)].glyph(), ' ');
    }

    #[test]
    fn center_align_centers_text() {
        let area = Rect::new(0, 0, 10, 1);
        let mut grid = Grid::new(10, 1);
        Text::new("hi")
            .align(Align::Center)
            .render(area, &mut Surface::new(&mut grid, area, 0));

        // 8 cols slack, 4 on the left: "hi" starts at column 4.
        assert_eq!(grid[Pos::new(4, 0)].glyph(), 'h');
        assert_eq!(grid[Pos::new(5, 0)].glyph(), 'i');
    }

    #[test]
    fn zero_width_is_a_no_op() {
        let area = Rect::new(0, 0, 0, 1);
        let mut grid = Grid::new(1, 1);
        Text::new("hi").render(area, &mut Surface::new(&mut grid, area, 0));
        assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
    }
}
