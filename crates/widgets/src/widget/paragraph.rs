//! [`Paragraph`]: word-wrapped text, implementing both [`Widget`] and
//! [`Measure`] so a caller can size a pane to fit before rendering.
use retroglyph_core::{Backend, Rect, Style, Terminal};

use super::{Measure, Widget};
use crate::text::{truncate, wrap};

/// Word-wrapped text in a single [`Style`].
///
/// `Paragraph::new(text, style)` wraps `text` to whatever width it is
/// rendered at (via [`Widget::render`]), or reports the height it would
/// need at a given width without rendering (via [`Measure::height_for`]) so
/// a caller can size its pane to fit instead of guessing a fixed height.
#[derive(Clone, Copy, Debug)]
pub struct Paragraph<'a> {
    text: &'a str,
    style: Style,
}

impl<'a> Paragraph<'a> {
    /// Text to be word-wrapped, drawn in `style`.
    #[must_use]
    pub const fn new(text: &'a str, style: Style) -> Self {
        Self { text, style }
    }
}

impl Measure for Paragraph<'_> {
    fn height_for(&self, width: u16) -> u16 {
        let lines = wrap(self.text, usize::from(width));
        u16::try_from(lines.len()).unwrap_or(u16::MAX)
    }
}

impl<B: Backend> Widget<B> for Paragraph<'_> {
    fn render(self, area: Rect, term: &mut Terminal<B>) {
        let width = usize::from(area.width());
        let lines = wrap(self.text, width);
        term.reset_style()
            .fg(self.style.foreground())
            .bg(self.style.background())
            .modifier(self.style.modifiers());
        for (i, line) in lines.iter().enumerate() {
            let Ok(dy) = u16::try_from(i) else { break };
            let Some(y) = area.top().checked_add(dy) else {
                break;
            };
            if y >= area.bottom() {
                break;
            }
            // A single word wider than `width` (see `wrap`'s doc) can still
            // overflow the pane; clip defensively so it never draws past it.
            let clipped = truncate(line, width);
            term.print(area.left(), y, &clipped);
        }
        term.reset_style();
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::Headless;

    use super::*;

    #[test]
    fn height_for_matches_wrapped_line_count() {
        let p = Paragraph::new("the quick brown fox jumps", Style::new());
        assert_eq!(p.height_for(10), 3); // "the quick" / "brown fox" / "jumps"
        assert_eq!(p.height_for(100), 1);
    }

    #[test]
    fn render_draws_one_line_per_wrapped_row() {
        let area = Rect::new(0, 0, 10, 5);
        let mut term = Terminal::new(Headless::new(10, 5));
        Paragraph::new("the quick brown fox jumps", Style::new()).render(area, &mut term);

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
        Paragraph::new("the quick brown fox jumps", Style::new()).render(area, &mut term);

        let row1: String = (0..10).map(|x| term.grid().get(x, 1).glyph()).collect();
        assert_eq!(row1.trim(), "");
    }
}
