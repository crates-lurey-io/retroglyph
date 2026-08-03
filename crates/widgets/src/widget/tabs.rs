//! [`Tabs`]: a horizontal strip of tab labels with a highlighted selected index.
use retroglyph_core::text::width as measured_width;
use retroglyph_core::{Color, Rect, Style};

use super::{InteractiveWidget, Widget};
use crate::Align;
use crate::Response;
use crate::Sense;
use crate::Surface;
use crate::Theme;
use crate::draw::fill_rect;
use crate::text::{draw_clipped, truncate as truncate_to_cols};

/// A horizontal strip of `titles` with the tab at `selected` highlighted.
///
/// Unlike [`Table`](super::Table)/[`List`](super::List), `Tabs` is a plain [`Widget`], not a
/// [`StatefulWidget`](super::StatefulWidget): there is no scroll offset for a tab strip, only a
/// selected index, so it takes `selected: Option<usize>` directly (set via [`Tabs::select`])
/// rather than a [`ListState`](crate::ListState): the app is free to drive that index however
/// it likes (a plain `usize` it owns, a [`FocusRing`](crate::FocusRing), whatever fits), the same
/// "app- or interaction-machinery-driven, widget just reads it" division of labor as every other
/// widget here.
///
/// Titles render left to right, `column_spacing` blank columns apart (default `1`, matching
/// [`Table::column_spacing`](super::Table::column_spacing)), with an optional single-character
/// `divider` (default `None`, i.e. no divider) centered in that spacing: set with
/// [`Tabs::divider`]. Drawing stops once a title would start past the area's right edge; there is
/// no horizontal scrolling.
///
/// `style` and `selected_style` default to [`Theme::DARK`] (as if [`Tabs::theme`] had been
/// called); set them with [`Tabs::style`]/[`Tabs::selected_style`].
///
/// As an [`InteractiveWidget`], `type State = usize` (the same index a caller already threads
/// into [`Tabs::select`] for the plain [`Widget`] path): a single id covers the whole strip, and
/// a click selects the tab whose column range contains [`Response::pointer_pos`], resolved
/// against the very same per-tab column layout the drawing routine uses, so the two can't
/// diverge.
///
/// # Examples
///
/// ```
/// use retroglyph_core::{Grid, Rect};
/// use retroglyph_widgets::{Surface, Tabs, Widget};
///
/// let titles = ["Overview", "Details", "Settings"];
/// let area = Rect::new(0, 0, 30, 1);
/// let mut grid = Grid::new(30, 1);
/// Tabs::new(&titles)
///     .select(Some(0))
///     .render(&mut Surface::new(&mut grid, area, 0));
/// ```
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
    /// A tab strip over `titles`, with nothing selected, styled from [`Theme::DARK`] (as if
    /// [`Tabs::theme`] had been called).
    #[must_use]
    pub fn new(titles: &'a [&'a str]) -> Self {
        Self {
            titles,
            selected: None,
            style: Style::new(),
            selected_style: Style::new(),
            column_spacing: 1,
            divider: None,
        }
        .theme(Theme::DARK)
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
    /// no divider: just `column_spacing` blank columns.
    #[must_use]
    pub const fn divider(mut self, divider: Option<char>) -> Self {
        self.divider = divider;
        self
    }

    /// Applies `theme`'s named roles to this tab strip: `style` becomes `theme.dim` (unselected
    /// tabs read as de-emphasized) on `theme.panel_bg`, and `selected_style` becomes `theme.bg`
    /// on `theme.accent`: the same bright-on-accent highlight [`super::List::theme`] and
    /// [`super::Table::theme`] give their own selected state.
    ///
    /// `style` sets an explicit background rather than leaving it at [`Style::new()`]'s default:
    /// an unset background isn't "transparent" once a real backend draws it (a bare
    /// `Color::Default` cell paints as solid black behind the glyph; see
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

    /// Same as [`Tabs::theme`], but `style` is drawn on `bg` instead of `theme.panel_bg`: for a
    /// tab strip drawn directly on a backdrop other than a themed [`super::Panel`]/
    /// [`super::Modal`]'s fill. `selected_style` still uses `theme.accent` as its background,
    /// unaffected by `bg`. [`Tabs::theme`] is exactly `theme_on(theme, theme.panel_bg)`.
    #[must_use]
    pub fn theme_on(mut self, theme: Theme, bg: Color) -> Self {
        self.style = Style::new().fg(theme.dim).bg(bg);
        self.selected_style = Style::new().fg(theme.bg).bg(theme.accent);
        self
    }
}

impl Tabs<'_> {
    /// Each drawn tab's `(index, start_x, text_width)`, left to right, stopping once a title
    /// would start past `area`'s right edge: the same layout [`Tabs::draw`] paints and
    /// [`Tabs::tab_at`] hit-tests against, computed once here so the two can't diverge. `x`
    /// values are absolute grid coordinates (matching `area`'s own space), the same space
    /// [`Response::pointer_pos`] reports in, since [`Tabs::tab_at`] compares them directly.
    fn tab_columns(&self, area: Rect) -> Vec<(usize, u16, u16)> {
        let mut columns = Vec::with_capacity(self.titles.len());
        let mut x = area.left();
        for (index, &title) in self.titles.iter().enumerate() {
            if x >= area.right() {
                break;
            }
            let avail = (area.right() - x) as usize;
            let text = truncate_to_cols(title, avail);
            let text_width = measured_width(text);
            columns.push((index, x, text_width));
            x = x.saturating_add(text_width);

            if index + 1 < self.titles.len() {
                x = x.saturating_add(self.column_spacing);
            }
        }
        columns
    }

    /// The shared drawing routine both [`Widget::render`] and [`InteractiveWidget::render`] use,
    /// parameterized on which tab (if any) is highlighted as selected.
    fn draw(&self, surface: &mut Surface<'_>, selected: Option<usize>) {
        let area = surface.area();
        if area.width() == 0 || area.height() == 0 {
            return;
        }

        // `tab_columns` reports absolute `x`, matching `area`'s own space (needed so
        // `Tabs::tab_at` can compare it directly against an absolute pointer position); `put`/
        // `print`/`fill_rect` below address this surface's own local coordinates instead, so
        // `area.left()` is subtracted back out at the point of drawing.
        let columns = self.tab_columns(area);
        for &(index, abs_x, text_width) in &columns {
            let x = abs_x - area.left();
            let title = self.titles[index];
            let style = if Some(index) == selected {
                self.selected_style
            } else {
                self.style
            };
            if Some(index) == selected && text_width > 0 {
                fill_rect(
                    surface,
                    Rect::new(x, 0, text_width, 1),
                    ' ',
                    Style::new().bg(style.background()),
                );
            }
            let _ = draw_clipped(surface, (x, 0), text_width, title, Align::Left, style);

            if let Some(divider) = self.divider
                && index + 1 < self.titles.len()
            {
                let mid = x + text_width + self.column_spacing / 2;
                if mid < area.width() {
                    surface.put((mid, 0), divider, Style::new());
                }
            }
        }
    }

    /// The index of the tab whose column range contains `pos`, or `None` if `pos` falls in the
    /// spacing between tabs, past the last drawn tab, or outside `area` entirely: a click there
    /// selects nothing rather than clamping to the nearest tab.
    fn tab_at(&self, area: Rect, pos: retroglyph_core::Pos) -> Option<usize> {
        if !area.contains_pos(pos) {
            return None;
        }
        self.tab_columns(area)
            .into_iter()
            .find(|&(_, x, width)| pos.x >= x && pos.x < x + width)
            .map(|(index, _, _)| index)
    }
}

impl Widget for Tabs<'_> {
    fn render(&self, surface: &mut Surface<'_>) {
        self.draw(surface, self.selected);
    }
}

impl InteractiveWidget for Tabs<'_> {
    type State = usize;

    /// A single id covers the whole strip: clicking resolves which tab via
    /// [`Response::pointer_pos`] and this strip's own column layout, rather than each tab
    /// registering its own id.
    fn sense(&self) -> Sense {
        Sense::click() | Sense::HOVER
    }

    fn render(&self, surface: &mut Surface<'_>, state: &mut Self::State, response: Response) {
        let area = surface.area();

        if response.clicked()
            && let Some(pos) = response.pointer_pos()
            && let Some(index) = self.tab_at(area, pos)
        {
            *state = index;
        }

        self.draw(surface, Some(*state));
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::{Grid, Pos};

    use super::*;

    #[test]
    fn draws_every_title_left_to_right() {
        let area = Rect::new(0, 0, 20, 1);
        let titles = ["One", "Two"];
        let mut grid = Grid::new(20, 1);
        let tabs = Tabs::new(&titles);
        Widget::render(&tabs, &mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'O');
        // "One" (3) + column_spacing (1) = tab 2 starts at column 4.
        assert_eq!(grid[Pos::new(4, 0)].glyph(), 'T');
    }

    #[test]
    fn highlights_the_selected_tab() {
        let area = Rect::new(0, 0, 20, 1);
        let titles = ["One", "Two"];
        let mut grid = Grid::new(20, 1);
        let tabs = Tabs::new(&titles).select(Some(1));
        Widget::render(&tabs, &mut Surface::new(&mut grid, area, 0));

        // Themed tabs (the `new()` default) distinguish the selected tab by both foreground and
        // background color: `theme.bg` on `theme.accent` vs `theme.dim` on `theme.panel_bg`.
        let selected_style = grid[Pos::new(4, 0)].style();
        let unselected_style = grid[Pos::new(0, 0)].style();
        assert_ne!(selected_style.foreground(), unselected_style.foreground());
        assert_ne!(selected_style.background(), unselected_style.background());
    }

    #[test]
    fn nothing_highlighted_when_unselected() {
        let area = Rect::new(0, 0, 20, 1);
        let titles = ["One", "Two"];
        let mut grid = Grid::new(20, 1);
        let tabs = Tabs::new(&titles);
        Widget::render(&tabs, &mut Surface::new(&mut grid, area, 0));

        // With nothing selected, both tabs use the same unselected `style`, so their backgrounds
        // match (unlike a selected tab, which gets `theme.accent` instead).
        let bg0 = grid[Pos::new(0, 0)].style().background();
        let bg1 = grid[Pos::new(4, 0)].style().background();
        assert_eq!(bg0, bg1);
    }

    #[test]
    fn column_spacing_can_be_overridden() {
        let area = Rect::new(0, 0, 20, 1);
        let titles = ["A", "B"];
        let mut grid = Grid::new(20, 1);
        let tabs = Tabs::new(&titles).column_spacing(3);
        Widget::render(&tabs, &mut Surface::new(&mut grid, area, 0));

        // Default spacing (1) would put "B" at column 2; spacing 3 pushes it to column 4.
        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'A');
        assert_eq!(grid[Pos::new(4, 0)].glyph(), 'B');
    }

    #[test]
    fn divider_renders_between_tabs_when_set() {
        let area = Rect::new(0, 0, 20, 1);
        let titles = ["A", "B"];
        let mut grid = Grid::new(20, 1);
        let tabs = Tabs::new(&titles).column_spacing(3).divider(Some('|'));
        Widget::render(&tabs, &mut Surface::new(&mut grid, area, 0));

        // "A" at 0, spacing [1,3), midpoint at 1 + 3/2 = 2.
        assert_eq!(grid[Pos::new(2, 0)].glyph(), '|');
    }

    #[test]
    fn no_divider_by_default() {
        let area = Rect::new(0, 0, 20, 1);
        let titles = ["A", "B"];
        let mut grid = Grid::new(20, 1);
        let tabs = Tabs::new(&titles);
        Widget::render(&tabs, &mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(1, 0)].glyph(), ' ');
    }

    #[test]
    fn stops_drawing_past_the_area_width_without_panicking() {
        let area = Rect::new(0, 0, 4, 1);
        let titles = ["Alpha", "Bravo", "Charlie"];
        let mut grid = Grid::new(4, 1);
        let tabs = Tabs::new(&titles);
        Widget::render(&tabs, &mut Surface::new(&mut grid, area, 0)); // must not panic

        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'A');
    }

    #[test]
    fn style_can_be_overridden() {
        use retroglyph_core::Color;

        let area = Rect::new(0, 0, 20, 1);
        let titles = ["One"];
        let custom = Style::new().fg(Color::RED);
        let mut grid = Grid::new(20, 1);
        let tabs = Tabs::new(&titles).style(custom);
        Widget::render(&tabs, &mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 0)].style().foreground(), Color::RED);
    }

    #[test]
    fn selected_style_can_be_overridden() {
        use retroglyph_core::Color;

        let area = Rect::new(0, 0, 20, 1);
        let titles = ["One"];
        let custom = Style::new().fg(Color::GREEN).bg(Color::BLUE);
        let mut grid = Grid::new(20, 1);
        let tabs = Tabs::new(&titles).selected_style(custom).select(Some(0));
        Widget::render(&tabs, &mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 0)].style().foreground(), Color::GREEN);
        assert_eq!(grid[Pos::new(0, 0)].style().background(), Color::BLUE);
    }

    #[test]
    fn zero_width_is_a_no_op() {
        let area = Rect::new(0, 0, 0, 1);
        let titles = ["One"];
        let mut grid = Grid::new(1, 1);
        let tabs = Tabs::new(&titles);
        Widget::render(&tabs, &mut Surface::new(&mut grid, area, 0));
        assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
    }

    #[test]
    fn theme_maps_named_roles_onto_style_and_selected_style() {
        let area = Rect::new(0, 0, 20, 1);
        let titles = ["One", "Two"];
        let mut grid = Grid::new(20, 1);
        let tabs = Tabs::new(&titles).theme(Theme::DARK).select(Some(1));
        Widget::render(&tabs, &mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 0)].style().foreground(), Theme::DARK.dim);
        assert_eq!(
            grid[Pos::new(0, 0)].style().background(),
            Theme::DARK.panel_bg
        );
        assert_eq!(grid[Pos::new(4, 0)].style().foreground(), Theme::DARK.bg);
        assert_eq!(
            grid[Pos::new(4, 0)].style().background(),
            Theme::DARK.accent
        );
    }

    #[test]
    fn theme_on_uses_the_given_backdrop_instead_of_panel_bg() {
        use retroglyph_core::Color;

        let area = Rect::new(0, 0, 20, 1);
        let titles = ["One"];
        let mut grid = Grid::new(20, 1);
        let tabs = Tabs::new(&titles)
            .theme_on(Theme::DARK, Color::Default)
            .select(Some(0));
        Widget::render(&tabs, &mut Surface::new(&mut grid, area, 0));

        // `selected_style` uses `theme.accent` as its background regardless of `bg`: only the
        // unselected `style` picks up the custom backdrop.
        assert_eq!(grid[Pos::new(0, 0)].style().foreground(), Theme::DARK.bg);
        assert_eq!(
            grid[Pos::new(0, 0)].style().background(),
            Theme::DARK.accent
        );
    }

    #[test]
    fn click_selects_the_tab_under_the_pointer() {
        let area = Rect::new(0, 0, 20, 1);
        let titles = ["One", "Two"];
        let tabs = Tabs::new(&titles);
        let mut state = 0usize;

        let response = Response {
            hovered: true,
            clicked: true,
            pointer_pos: Some(Pos::new(4, 0)), // over "Two"
            ..Response::default()
        };
        let mut grid = Grid::new(20, 1);
        InteractiveWidget::render(
            &tabs,
            &mut Surface::new(&mut grid, area, 0),
            &mut state,
            response,
        );
        assert_eq!(state, 1);
    }

    #[test]
    fn click_past_the_last_tab_selects_nothing() {
        let area = Rect::new(0, 0, 20, 1);
        let titles = ["One", "Two"];
        let tabs = Tabs::new(&titles);
        let mut state = 0usize;

        let response = Response {
            hovered: true,
            clicked: true,
            pointer_pos: Some(Pos::new(10, 0)), // past "Two", still inside the wide area
            ..Response::default()
        };
        let mut grid = Grid::new(20, 1);
        InteractiveWidget::render(
            &tabs,
            &mut Surface::new(&mut grid, area, 0),
            &mut state,
            response,
        );
        assert_eq!(state, 0); // unchanged: not clamped to the last tab
    }
}
