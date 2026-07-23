//! [`Tabs`]: a horizontal strip of tab labels with a highlighted selected index.
use retroglyph_core::{Backend, Color, Rect, Style, Terminal};

use super::Widget;
use crate::Theme;
use crate::draw::fill_rect;
use crate::text::truncate as truncate_to_cols;

/// A horizontal strip of `titles` with the tab at `selected` highlighted.
///
/// Unlike [`Table`](super::Table)/[`List`](super::List), `Tabs` is a plain [`Widget`], not a
/// [`StatefulWidget`](super::StatefulWidget): there is no scroll offset for a tab strip, only a
/// selected index, so it takes `selected: Option<usize>` directly (set via [`Tabs::select`])
/// rather than a [`ListState`](crate::ListState) -- the app is free to drive that index however
/// it likes (a plain `usize` it owns, a [`FocusRing`](crate::FocusRing), whatever fits), the same
/// "app- or interaction-machinery-driven, widget just reads it" division of labor as every other
/// widget here.
///
/// Titles render left to right, `column_spacing` blank columns apart (default `1`, matching
/// [`Table::column_spacing`](super::Table::column_spacing)), with an optional single-character
/// `divider` (default `None`, i.e. no divider) centered in that spacing -- set with
/// [`Tabs::divider`]. Drawing stops once a title would start past the area's right edge; there is
/// no horizontal scrolling.
///
/// `style` and `selected_style` each default to the same fixed palette as
/// [`Table`](super::Table)'s `row_style`/`selected_style`; set them with [`Tabs::style`]/
/// [`Tabs::selected_style`].
#[derive(Clone, Copy, Debug)]
pub struct Tabs<'a> {
    titles: &'a [&'a str],
    selected: Option<usize>,
    style: Style,
    selected_style: Style,
    column_spacing: u16,
    divider: Option<char>,
}

impl<'a> Tabs<'a> {
    /// A tab strip over `titles`, with nothing selected and the default style.
    #[must_use]
    pub fn new(titles: &'a [&'a str]) -> Self {
        Self {
            titles,
            selected: None,
            style: Style::new().fg(Color::Rgb {
                r: 170,
                g: 175,
                b: 190,
            }),
            selected_style: Style::new().fg(Color::BRIGHT_WHITE).bg(Color::Rgb {
                r: 40,
                g: 60,
                b: 90,
            }),
            column_spacing: 1,
            divider: None,
        }
    }

    /// Select tab `index` (or clear the selection with `None`).
    #[must_use]
    pub const fn select(mut self, index: Option<usize>) -> Self {
        self.selected = index;
        self
    }

    /// Set the style of unselected tabs.
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set the style of the selected tab, including its background fill.
    #[must_use]
    pub const fn selected_style(mut self, style: Style) -> Self {
        self.selected_style = style;
        self
    }

    /// Set the number of blank columns between tabs.
    #[must_use]
    pub const fn column_spacing(mut self, spacing: u16) -> Self {
        self.column_spacing = spacing;
        self
    }

    /// Set a divider character drawn within the spacing between tabs. `None` (the default) draws
    /// no divider -- just `column_spacing` blank columns.
    #[must_use]
    pub const fn divider(mut self, divider: Option<char>) -> Self {
        self.divider = divider;
        self
    }

    /// Applies `theme`'s named roles to this tab strip: `style` becomes `theme.dim` (unselected
    /// tabs read as de-emphasized) on `theme.panel_bg`, and `selected_style` becomes
    /// `theme.accent` on `theme.panel_bg`.
    ///
    /// `style` sets an explicit background rather than leaving it at [`Style::new()`]'s default:
    /// an unset background isn't "transparent" once a real backend draws it (a bare
    /// `Color::Default` cell paints as solid black behind the glyph -- see
    /// `retroglyph-software`'s `DEFAULT_BG`), so this widget assumes it's drawn on
    /// `theme.panel_bg`, true when composed with a themed [`super::Panel`]/[`super::Modal`].
    /// Drawing this tab strip directly on the raw screen background instead needs a manual
    /// `.style(...)` override afterwards.
    ///
    /// Call before any manual [`Tabs::style`]/[`Tabs::selected_style`] override you want to keep.
    #[must_use]
    pub fn theme(self, theme: Theme) -> Self {
        self.theme_on(theme, theme.panel_bg)
    }

    /// Same as [`Tabs::theme`], but `style`/`selected_style` are drawn on `bg` instead of
    /// `theme.panel_bg` -- for a tab strip drawn directly on a backdrop other than a themed
    /// [`super::Panel`]/[`super::Modal`]'s fill. [`Tabs::theme`] is exactly
    /// `theme_on(theme, theme.panel_bg)`.
    #[must_use]
    pub fn theme_on(mut self, theme: Theme, bg: Color) -> Self {
        self.style = Style::new().fg(theme.dim).bg(bg);
        self.selected_style = Style::new().fg(theme.accent).bg(bg);
        self
    }
}

impl<B: Backend> Widget<B> for Tabs<'_> {
    fn render(self, area: Rect, term: &mut Terminal<B>) {
        if area.width() == 0 || area.height() == 0 {
            return;
        }

        let y = area.top();
        let mut x = area.left();
        for (index, &title) in self.titles.iter().enumerate() {
            if x >= area.right() {
                break;
            }
            let avail = (area.right() - x) as usize;
            let text = truncate_to_cols(title, avail);
            let style = if Some(index) == self.selected {
                self.selected_style
            } else {
                self.style
            };
            let text_width = text.chars().count() as u16;
            if Some(index) == self.selected && text_width > 0 {
                fill_rect(
                    term,
                    Rect::new(x, y, text_width, 1),
                    ' ',
                    Style::new().bg(style.background()),
                );
            }
            term.reset_style()
                .fg(style.foreground())
                .bg(style.background());
            term.print(x, y, text);
            x = x.saturating_add(text_width);

            if index + 1 < self.titles.len() {
                if let Some(divider) = self.divider {
                    let mid = x + self.column_spacing / 2;
                    if mid < area.right() {
                        term.reset_style();
                        term.put(mid, y, divider);
                    }
                }
                x = x.saturating_add(self.column_spacing);
            }
        }
        term.reset_style();
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::Headless;

    use super::*;

    #[test]
    fn draws_every_title_left_to_right() {
        let area = Rect::new(0, 0, 20, 1);
        let titles = ["One", "Two"];
        let mut term = Terminal::new(Headless::new(20, 1));
        Tabs::new(&titles).render(area, &mut term);

        assert_eq!(term.grid().get(0, 0).glyph(), 'O');
        // "One" (3) + column_spacing (1) = tab 2 starts at column 4.
        assert_eq!(term.grid().get(4, 0).glyph(), 'T');
    }

    #[test]
    fn highlights_the_selected_tab() {
        let area = Rect::new(0, 0, 20, 1);
        let titles = ["One", "Two"];
        let mut term = Terminal::new(Headless::new(20, 1));
        Tabs::new(&titles).select(Some(1)).render(area, &mut term);

        let selected_bg = term.grid().get(4, 0).style().background();
        let plain_bg = term.grid().get(0, 0).style().background();
        assert_ne!(selected_bg, plain_bg);
    }

    #[test]
    fn nothing_highlighted_when_unselected() {
        let area = Rect::new(0, 0, 20, 1);
        let titles = ["One", "Two"];
        let mut term = Terminal::new(Headless::new(20, 1));
        Tabs::new(&titles).render(area, &mut term);

        let bg0 = term.grid().get(0, 0).style().background();
        let bg1 = term.grid().get(4, 0).style().background();
        assert_eq!(bg0, bg1);
    }

    #[test]
    fn column_spacing_can_be_overridden() {
        let area = Rect::new(0, 0, 20, 1);
        let titles = ["A", "B"];
        let mut term = Terminal::new(Headless::new(20, 1));
        Tabs::new(&titles).column_spacing(3).render(area, &mut term);

        // Default spacing (1) would put "B" at column 2; spacing 3 pushes it to column 4.
        assert_eq!(term.grid().get(0, 0).glyph(), 'A');
        assert_eq!(term.grid().get(4, 0).glyph(), 'B');
    }

    #[test]
    fn divider_renders_between_tabs_when_set() {
        let area = Rect::new(0, 0, 20, 1);
        let titles = ["A", "B"];
        let mut term = Terminal::new(Headless::new(20, 1));
        Tabs::new(&titles)
            .column_spacing(3)
            .divider(Some('|'))
            .render(area, &mut term);

        // "A" at 0, spacing [1,3), midpoint at 1 + 3/2 = 2.
        assert_eq!(term.grid().get(2, 0).glyph(), '|');
    }

    #[test]
    fn no_divider_by_default() {
        let area = Rect::new(0, 0, 20, 1);
        let titles = ["A", "B"];
        let mut term = Terminal::new(Headless::new(20, 1));
        Tabs::new(&titles).render(area, &mut term);

        assert_eq!(term.grid().get(1, 0).glyph(), ' ');
    }

    #[test]
    fn stops_drawing_past_the_area_width_without_panicking() {
        let area = Rect::new(0, 0, 4, 1);
        let titles = ["Alpha", "Bravo", "Charlie"];
        let mut term = Terminal::new(Headless::new(4, 1));
        Tabs::new(&titles).render(area, &mut term); // must not panic

        assert_eq!(term.grid().get(0, 0).glyph(), 'A');
    }

    #[test]
    fn style_can_be_overridden() {
        let area = Rect::new(0, 0, 20, 1);
        let titles = ["One"];
        let custom = Style::new().fg(Color::RED);
        let mut term = Terminal::new(Headless::new(20, 1));
        Tabs::new(&titles).style(custom).render(area, &mut term);

        assert_eq!(term.grid().get(0, 0).style().foreground(), Color::RED);
    }

    #[test]
    fn selected_style_can_be_overridden() {
        let area = Rect::new(0, 0, 20, 1);
        let titles = ["One"];
        let custom = Style::new().fg(Color::GREEN).bg(Color::BLUE);
        let mut term = Terminal::new(Headless::new(20, 1));
        Tabs::new(&titles)
            .selected_style(custom)
            .select(Some(0))
            .render(area, &mut term);

        assert_eq!(term.grid().get(0, 0).style().foreground(), Color::GREEN);
        assert_eq!(term.grid().get(0, 0).style().background(), Color::BLUE);
    }

    #[test]
    fn zero_width_is_a_no_op() {
        let area = Rect::new(0, 0, 0, 1);
        let titles = ["One"];
        let mut term = Terminal::new(Headless::new(1, 1));
        Tabs::new(&titles).render(area, &mut term);
        assert_eq!(term.grid().get(0, 0).glyph(), ' ');
    }

    #[test]
    fn theme_maps_named_roles_onto_style_and_selected_style() {
        let area = Rect::new(0, 0, 20, 1);
        let titles = ["One", "Two"];
        let mut term = Terminal::new(Headless::new(20, 1));
        Tabs::new(&titles)
            .theme(Theme::DARK)
            .select(Some(1))
            .render(area, &mut term);

        assert_eq!(term.grid().get(0, 0).style().foreground(), Theme::DARK.dim);
        assert_eq!(
            term.grid().get(0, 0).style().background(),
            Theme::DARK.panel_bg
        );
        assert_eq!(
            term.grid().get(4, 0).style().foreground(),
            Theme::DARK.accent
        );
        assert_eq!(
            term.grid().get(4, 0).style().background(),
            Theme::DARK.panel_bg
        );
    }

    #[test]
    fn theme_on_uses_the_given_backdrop_instead_of_panel_bg() {
        let area = Rect::new(0, 0, 20, 1);
        let titles = ["One"];
        let mut term = Terminal::new(Headless::new(20, 1));
        Tabs::new(&titles)
            .theme_on(Theme::DARK, Color::Default)
            .select(Some(0))
            .render(area, &mut term);

        assert_eq!(
            term.grid().get(0, 0).style().foreground(),
            Theme::DARK.accent
        );
        assert_eq!(term.grid().get(0, 0).style().background(), Color::Default);
    }
}
