//! [`ProgressBar`]: a horizontal progress bar.
use retroglyph_core::{Backend, Color, Rect, Style, Terminal};

use super::Widget;
use crate::Theme;

/// A horizontal progress bar that fills `value / max` of the area it's
/// rendered into.
///
/// `filled_style`/`empty_style` default to [`Style::new()`]; set them with
/// [`ProgressBar::filled_style`]/[`ProgressBar::empty_style`].
/// `area.height()` is ignored; only the first row is drawn.
#[derive(Clone, Copy, Debug)]
pub struct ProgressBar {
    value: u32,
    max: u32,
    filled_style: Style,
    empty_style: Style,
}

impl ProgressBar {
    /// A bar filling `value / max`, in the default style.
    #[must_use]
    pub fn new(value: u32, max: u32) -> Self {
        Self {
            value,
            max,
            filled_style: Style::new(),
            empty_style: Style::new(),
        }
    }

    /// Set the style of the filled portion.
    #[must_use]
    pub const fn filled_style(mut self, style: Style) -> Self {
        self.filled_style = style;
        self
    }

    /// Set the style of the empty portion.
    #[must_use]
    pub const fn empty_style(mut self, style: Style) -> Self {
        self.empty_style = style;
        self
    }

    /// Applies `theme`'s named roles to this bar: `filled_style` becomes `theme.accent` (progress
    /// reads as emphasis, the same role [`super::Tabs::theme`]/[`super::Button::theme`] use for a
    /// selected/focused state) on `theme.panel_bg`, and `empty_style` becomes `theme.dim` on
    /// `theme.panel_bg`.
    ///
    /// Both set an explicit background rather than leaving it at [`Style::new()`]'s default: an
    /// unset background isn't "transparent" once a real backend draws it (a bare `Color::Default`
    /// cell paints as solid black behind the glyph -- see `retroglyph-software`'s `DEFAULT_BG`),
    /// which matters most for `empty_style`'s `'░'` glyph (it doesn't fully cover its cell the way
    /// `filled_style`'s `'█'` does, so its background actually shows). This widget assumes it's
    /// drawn on `theme.panel_bg`, true when composed with a themed [`super::Panel`]/
    /// [`super::Modal`]. Drawing this bar directly on the raw screen background instead needs a
    /// manual `.filled_style(...)`/`.empty_style(...)` override afterwards.
    ///
    /// Call before any manual [`ProgressBar::filled_style`]/[`ProgressBar::empty_style`] override
    /// you want to keep.
    #[must_use]
    pub fn theme(self, theme: Theme) -> Self {
        self.theme_on(theme, theme.panel_bg)
    }

    /// Same as [`ProgressBar::theme`], but `filled_style`/`empty_style` are drawn on `bg` instead
    /// of `theme.panel_bg` -- for a bar drawn directly on a backdrop other than a themed
    /// [`super::Panel`]/[`super::Modal`]'s fill. [`ProgressBar::theme`] is exactly
    /// `theme_on(theme, theme.panel_bg)`.
    #[must_use]
    pub fn theme_on(mut self, theme: Theme, bg: Color) -> Self {
        self.filled_style = Style::new().fg(theme.accent).bg(bg);
        self.empty_style = Style::new().fg(theme.dim).bg(bg);
        self
    }
}

impl<B: Backend> Widget<B> for ProgressBar {
    fn render(self, area: Rect, term: &mut Terminal<B>) {
        if area.width() == 0 || self.max == 0 {
            return;
        }
        let filled_cells = ((u64::from(self.value.min(self.max)) * u64::from(area.width()))
            / u64::from(self.max)) as u16;
        let y = area.top();
        for x in area.left()..area.right() {
            let is_filled = x < area.left() + filled_cells;
            let style = if is_filled {
                self.filled_style
            } else {
                self.empty_style
            };
            term.reset_style()
                .fg(style.foreground())
                .bg(style.background());
            term.put(x, y, if is_filled { '█' } else { '░' });
        }
        term.reset_style();
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::Headless;

    use super::*;

    #[test]
    fn fills_proportionally() {
        let area = Rect::new(0, 0, 10, 1);
        let mut term = Terminal::new(Headless::new(10, 1));
        ProgressBar::new(5, 10).render(area, &mut term);

        for x in 0..5 {
            assert_eq!(term.grid().get(x, 0).glyph(), '█');
        }
        for x in 5..10 {
            assert_eq!(term.grid().get(x, 0).glyph(), '░');
        }
    }

    #[test]
    fn zero_max_is_a_no_op() {
        let area = Rect::new(0, 0, 10, 1);
        let mut term = Terminal::new(Headless::new(10, 1));
        ProgressBar::new(0, 0).render(area, &mut term);
        assert_eq!(term.grid().get(0, 0).glyph(), ' ');
    }

    #[test]
    fn filled_and_empty_styles_are_configurable() {
        use retroglyph_core::Color;

        let area = Rect::new(0, 0, 4, 1);
        let mut term = Terminal::new(Headless::new(4, 1));
        ProgressBar::new(2, 4)
            .filled_style(Style::new().fg(Color::WHITE))
            .empty_style(Style::new().fg(Color::BLACK))
            .render(area, &mut term);

        assert_eq!(term.grid().get(0, 0).style().foreground(), Color::WHITE);
        assert_eq!(term.grid().get(3, 0).style().foreground(), Color::BLACK);
    }

    #[test]
    fn theme_maps_named_roles_onto_filled_and_empty_styles() {
        let area = Rect::new(0, 0, 4, 1);
        let mut term = Terminal::new(Headless::new(4, 1));
        ProgressBar::new(2, 4)
            .theme(Theme::DARK)
            .render(area, &mut term);

        assert_eq!(
            term.grid().get(0, 0).style().foreground(),
            Theme::DARK.accent
        );
        assert_eq!(
            term.grid().get(0, 0).style().background(),
            Theme::DARK.panel_bg
        );
        assert_eq!(term.grid().get(3, 0).style().foreground(), Theme::DARK.dim);
        assert_eq!(
            term.grid().get(3, 0).style().background(),
            Theme::DARK.panel_bg
        );
    }

    #[test]
    fn theme_on_uses_the_given_backdrop_instead_of_panel_bg() {
        let area = Rect::new(0, 0, 4, 1);
        let mut term = Terminal::new(Headless::new(4, 1));
        ProgressBar::new(2, 4)
            .theme_on(Theme::DARK, Color::Default)
            .render(area, &mut term);

        assert_eq!(
            term.grid().get(0, 0).style().foreground(),
            Theme::DARK.accent
        );
        assert_eq!(term.grid().get(0, 0).style().background(), Color::Default);
        assert_eq!(term.grid().get(3, 0).style().foreground(), Theme::DARK.dim);
        assert_eq!(term.grid().get(3, 0).style().background(), Color::Default);
    }
}
