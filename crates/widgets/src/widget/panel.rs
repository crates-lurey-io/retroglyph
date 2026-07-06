//! [`Panel`], the [`Widget`] form of [`crate::draw::panel`].
use retroglyph_core::{Backend, Rect, Style, Terminal};

use super::Widget;

/// A bordered, titled panel — the [`Widget`] form of [`crate::draw::panel`].
///
/// `Panel::new(border_style, fill_style).title("...").render(area, term)`
/// does exactly what `panel(term, area, Some("..."), border_style,
/// fill_style)` does; use whichever reads better at the call site.
#[derive(Clone, Copy, Debug)]
pub struct Panel<'a> {
    title: Option<&'a str>,
    border_style: Style,
    fill_style: Style,
}

impl<'a> Panel<'a> {
    /// A panel with no title.
    #[must_use]
    pub const fn new(border_style: Style, fill_style: Style) -> Self {
        Self {
            title: None,
            border_style,
            fill_style,
        }
    }

    /// Set the panel's title.
    #[must_use]
    pub const fn title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }
}

impl<B: Backend> Widget<B> for Panel<'_> {
    fn render(self, area: Rect, term: &mut Terminal<B>) {
        crate::draw::panel(term, area, self.title, self.border_style, self.fill_style);
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::{Color, Headless};

    use super::*;

    #[test]
    fn panel_widget_matches_free_function() {
        let area = Rect::new(0, 0, 10, 4);
        let border = Style::new().fg(Color::WHITE);
        let fill = Style::new();

        let mut via_trait = Terminal::new(Headless::new(10, 4));
        Panel::new(border, fill)
            .title("hi")
            .render(area, &mut via_trait);

        let mut via_function = Terminal::new(Headless::new(10, 4));
        crate::draw::panel(&mut via_function, area, Some("hi"), border, fill);

        for y in 0..4 {
            for x in 0..10 {
                assert_eq!(
                    via_trait.grid().get(x, y).glyph(),
                    via_function.grid().get(x, y).glyph(),
                );
            }
        }
    }
}
