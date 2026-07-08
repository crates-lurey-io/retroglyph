//! [`Panel`]: a bordered, titled panel.
use retroglyph_core::{Backend, Rect, Style, Terminal};

use super::{BoxBorder, Widget};
use crate::draw::fill_rect;
use crate::text::truncate as truncate_to_cols;

/// A bordered panel: a filled background with a box border and an optional
/// title centred in the top edge.
///
/// `border_style` (the box outline and title) and `fill_style` (the
/// interior background) both default to [`Style::new()`]; there is no
/// title by default. Set whichever of these a caller needs via
/// [`Panel::border_style`]/[`Panel::fill_style`]/[`Panel::title`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Panel<'a> {
    title: Option<&'a str>,
    border_style: Style,
    fill_style: Style,
}

impl<'a> Panel<'a> {
    /// A plain, untitled panel in the default style.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the panel's title.
    #[must_use]
    pub const fn title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }

    /// Set the box outline and title's style.
    #[must_use]
    pub const fn border_style(mut self, style: Style) -> Self {
        self.border_style = style;
        self
    }

    /// Set the interior background's style.
    #[must_use]
    pub const fn fill_style(mut self, style: Style) -> Self {
        self.fill_style = style;
        self
    }
}

impl<B: Backend> Widget<B> for Panel<'_> {
    fn render(self, area: Rect, term: &mut Terminal<B>) {
        if area.width() < 2 || area.height() < 2 {
            return;
        }

        // Fill interior (inside the border).
        let inner = Rect::new(
            area.left() + 1,
            area.top() + 1,
            area.width().saturating_sub(2),
            area.height().saturating_sub(2),
        );
        fill_rect(term, inner, ' ', self.fill_style);

        BoxBorder::new().style(self.border_style).render(area, term);

        // Render the title into the top border if one was provided.
        if let Some(t) = self.title {
            let max_title_w = area.width().saturating_sub(4) as usize; // 2 border + 2 spaces
            if max_title_w == 0 {
                return;
            }
            // Truncate to fit.
            let t = truncate_to_cols(t, max_title_w);
            let title_x = area.left() + (area.width() - t.len() as u16 - 2) / 2;
            let title_y = area.top();
            term.reset_style()
                .fg(self.border_style.foreground())
                .bg(self.border_style.background());
            term.put(title_x, title_y, ' ');
            term.print(title_x + 1, title_y, &t);
            term.put(title_x + 1 + t.len() as u16, title_y, ' ');
            term.reset_style();
        }
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::{Color, Headless};

    use super::*;

    #[test]
    fn draws_border_fill_and_title() {
        let area = Rect::new(0, 0, 10, 4);
        let border = Style::new().fg(Color::WHITE);
        let fill = Style::new();

        let mut term = Terminal::new(Headless::new(10, 4));
        Panel::new()
            .border_style(border)
            .fill_style(fill)
            .title("hi")
            .render(area, &mut term);

        assert_eq!(term.grid().get(0, 0).glyph(), '┌');
        assert_eq!(term.grid().get(1, 1).glyph(), ' '); // interior filled
        // Title centred in the top border somewhere.
        let top_row: String = (0..10).map(|x| term.grid().get(x, 0).glyph()).collect();
        assert!(top_row.contains("hi"));
    }

    #[test]
    fn long_title_is_truncated_to_fit() {
        let area = Rect::new(0, 0, 8, 3); // max_title_w = 8 - 4 = 4
        let mut term = Terminal::new(Headless::new(8, 3));
        Panel::new()
            .title("a very long title")
            .render(area, &mut term);

        let top_row: String = (0..8).map(|x| term.grid().get(x, 0).glyph()).collect();
        assert!(!top_row.contains("a very long title"));
    }

    #[test]
    fn too_small_is_a_no_op() {
        let area = Rect::new(0, 0, 1, 1);
        let mut term = Terminal::new(Headless::new(1, 1));
        Panel::new().render(area, &mut term);
        assert_eq!(term.grid().get(0, 0).glyph(), ' ');
    }
}
