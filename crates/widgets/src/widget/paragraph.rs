//! [`Paragraph`]: word-wrapped text, implementing both [`Widget`] and
//! [`Measure`] so a caller can size a pane to fit before rendering.
//!
//! Requires the `egc` feature: wrapping is delegated entirely to
//! [`retroglyph_core::layout::TextLayout`], which handles grapheme clusters
//! and hard newlines correctly. This module adds no wrapping logic of its
//! own -- see `crates/widgets/src/text.rs` for why that duplication was
//! removed.
use retroglyph_core::layout::TextLayout;
use retroglyph_core::text::{Line, Span};
use retroglyph_core::{Backend, Rect, Style, Terminal};

use super::{Measure, Widget};

/// Word-wrapped text in a single [`Style`].
///
/// `Paragraph::new(text)` wraps `text` to whatever width it is rendered at
/// (via [`Widget::render`]), or reports the height it would need at a
/// given width without rendering (via [`Measure::height_for`]) so a caller
/// can size its pane to fit instead of guessing a fixed height. `style`
/// defaults to [`Style::new()`]; set it with [`Paragraph::style`].
#[derive(Clone, Copy, Debug)]
pub struct Paragraph<'a> {
    text: &'a str,
    style: Style,
}

impl<'a> Paragraph<'a> {
    /// Text to be word-wrapped, in the default style.
    #[must_use]
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            style: Style::new(),
        }
    }

    /// Set the text's style.
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    fn line(&self) -> Line {
        Line::from(Span::styled(self.text, self.style))
    }
}

impl Measure for Paragraph<'_> {
    fn height_for(&self, width: u16) -> u16 {
        let line = self.line();
        TextLayout::new(&line)
            .rect(Rect::new(0, 0, width, u16::MAX))
            .measure()
            .height
    }
}

impl<B: Backend> Widget<B> for Paragraph<'_> {
    fn render(self, area: Rect, term: &mut Terminal<B>) {
        let line = self.line();
        TextLayout::new(&line).rect(area).render(term);
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::Headless;

    use super::*;

    #[test]
    fn height_for_matches_wrapped_line_count() {
        let p = Paragraph::new("the quick brown fox jumps");
        assert_eq!(p.height_for(10), 3); // "the quick" / "brown fox" / "jumps"
        assert_eq!(p.height_for(100), 1);
    }

    #[test]
    fn height_for_respects_hard_newlines() {
        // A naive whitespace-based wrap would flatten this to one paragraph;
        // TextLayout treats "\n" as a hard break regardless of width.
        let p = Paragraph::new("first\nsecond\nthird");
        assert_eq!(p.height_for(100), 3);
    }

    #[test]
    fn render_draws_one_line_per_wrapped_row() {
        let area = Rect::new(0, 0, 10, 5);
        let mut term = Terminal::new(Headless::new(10, 5));
        Paragraph::new("the quick brown fox jumps").render(area, &mut term);

        let row0: String = (0..10).map(|x| term.grid().get(x, 0).glyph()).collect();
        let row1: String = (0..10).map(|x| term.grid().get(x, 1).glyph()).collect();
        let row2: String = (0..10).map(|x| term.grid().get(x, 2).glyph()).collect();
        assert!(row0.starts_with("the quick"));
        assert!(row1.starts_with("brown fox"));
        assert!(row2.starts_with("jumps"));
    }

    #[test]
    fn render_stops_at_the_area_bottom() {
        // Only 1 row of height: only the first wrapped line should draw.
        let area = Rect::new(0, 0, 10, 1);
        let mut term = Terminal::new(Headless::new(10, 2));
        Paragraph::new("the quick brown fox jumps").render(area, &mut term);

        let row1: String = (0..10).map(|x| term.grid().get(x, 1).glyph()).collect();
        assert_eq!(row1.trim(), "");
    }
}
