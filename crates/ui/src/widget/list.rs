//! [`List`]: a scrollable, single-column list with a [`ListState`]-driven highlighted item.
use retroglyph_core::color::{Color, Style};
use retroglyph_core::grid::Rect;

use super::window::visible_window;
use super::{HighlightSpacing, InteractiveWidget, ListDirection, Measure, StatefulWidget};
use crate::Surface;
use crate::align::Align;
use crate::draw::fill_rect;
use crate::interact::Response;
use crate::interact::Sense;
use crate::state::ListState;
use crate::text::draw_clipped;
use crate::theme::Theme;

/// A scrollable, single-column list of plain-text items with a [`ListState`]-driven highlighted
/// item: `Table`'s single-column sibling, sharing its windowing and selection story.
///
/// One `item` renders per line, top-aligned in the area it's rendered into and clipped to
/// `area.width()`. `state.offset()` is the index of the first item drawn: rendering draws
/// whatever window `offset` names and does not clamp or auto-scroll it, matching
/// [`Table`](super::Table)'s and [`ListState`]'s existing "only the caller knows the viewport
/// height" design. Call [`state.ensure_visible(visible_item_count)`](ListState::ensure_visible)
/// before rendering to keep `state.selected()` on-screen. If `selected()` is `Some` and its item
/// falls within the visible window, that item is drawn with an inverted highlight background; if
/// it has scrolled out of view, nothing is highlighted.
///
/// `item_style` and `selected_style` default to [`Theme::DARK`], as if [`List::theme`] had been
/// called; set a different [`Theme`] with [`List::theme`]/[`List::theme_on`] or override a single
/// field with [`List::item_style`]/[`List::selected_style`].
///
/// As an [`InteractiveWidget`], a single id covers the whole list: a click selects the row under
/// [`Response::pointer_pos`], resolved from this list's own row geometry (`state.offset()` plus
/// the pointer's row within `surface.area()`) rather than from a separate id per row, and the
/// wheel scrolls it via [`Response::scroll_delta`]/[`ListState::scroll_by`].
///
/// Selection is style-only by default (a swap to `selected_style`), the same as before
/// [`List::highlight_symbol`] existed. Set it to prefix the selected item with a marker like
/// `"> "`, the one selection signal that survives `set_plain_mode`'s style stripping, or that
/// reads clearly on a 16-color backend where `selected_style`'s background isn't distinct enough.
/// [`List::highlight_spacing`] controls whether that marker column reserves width even when
/// nothing is selected, and [`List::direction`] draws the same windowed items from the bottom of
/// the area upward instead of the top downward, for a chat/log-style list that grows upward.
///
/// # Examples
///
/// ```
/// use retroglyph_core::grid::{Grid, Rect};
/// use retroglyph_ui::state::ListState;
/// use retroglyph_ui::widget::{List, StatefulWidget};
/// use retroglyph_ui::Surface;
///
/// let items = ["Alpha", "Bravo", "Charlie"];
/// let mut state = ListState::new();
/// state.select(Some(1));
///
/// let area = Rect::new(0, 0, 20, 3);
/// let mut grid = Grid::new(20, 3);
/// List::new(&items).render(&mut Surface::new(&mut grid, area, 0), &mut state);
/// ```
#[derive(Clone, Copy, Debug)]
pub struct List<'a> {
    items: &'a [&'a str],
    item_style: Style,
    selected_style: Style,
    highlight_symbol: &'a str,
    highlight_spacing: HighlightSpacing,
    direction: ListDirection,
}

impl<'a> List<'a> {
    /// A list of `items`, styled from [`Theme::DARK`] (as if [`List::theme`] had been called); set
    /// [`List::theme`]/[`List::theme_on`] for a different [`Theme`] or
    /// [`List::item_style`]/[`List::selected_style`] for a one-off override.
    #[must_use]
    pub fn new(items: &'a [&'a str]) -> Self {
        Self {
            items,
            item_style: Style::new(),
            selected_style: Style::new(),
            highlight_symbol: "",
            highlight_spacing: HighlightSpacing::WhenSelected,
            direction: ListDirection::TopToBottom,
        }
        .theme(Theme::DARK)
    }

    /// Set the style of unselected items.
    #[must_use]
    pub const fn item_style(mut self, style: Style) -> Self {
        self.item_style = style;
        self
    }

    /// Set the style of the selected item, including its background fill.
    #[must_use]
    pub const fn selected_style(mut self, style: Style) -> Self {
        self.selected_style = style;
        self
    }

    /// Prefix the selected item with `symbol`, e.g. `"> "`. Empty by default (no marker column,
    /// the same style-only highlight as before this method existed).
    ///
    /// While the marker column is reserved (see [`List::highlight_spacing`]), unselected items'
    /// text still starts one column past it, so every item's text lines up regardless of which
    /// row is selected; that reserved column itself is left untouched (not filled) on unselected
    /// rows, the same "caller's backdrop shows through" behavior [`List::item_style`] already has
    /// for any unset background.
    #[must_use]
    pub const fn highlight_symbol(mut self, symbol: &'a str) -> Self {
        self.highlight_symbol = symbol;
        self
    }

    /// Whether the marker column [`List::highlight_symbol`] draws into reserves width even when
    /// nothing is selected. Defaults to [`HighlightSpacing::WhenSelected`].
    #[must_use]
    pub const fn highlight_spacing(mut self, spacing: HighlightSpacing) -> Self {
        self.highlight_spacing = spacing;
        self
    }

    /// Which end of the area this list's items render from. Defaults to
    /// [`ListDirection::TopToBottom`].
    #[must_use]
    pub const fn direction(mut self, direction: ListDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Applies `theme`'s named roles to this list: `item_style` becomes `theme.fg` on
    /// `theme.panel_bg`, and `selected_style` becomes `theme.bg` on `theme.accent`.
    ///
    /// `item_style` sets an explicit background rather than leaving it at [`Style::new()`]'s
    /// default: an unset background isn't "transparent" once a real backend draws it (a bare
    /// `Color::Default` cell paints as solid black behind the glyph; see
    /// `retroglyph-software`'s `DEFAULT_BG`), so this widget assumes it's drawn on
    /// `theme.panel_bg`, true when composed with a themed [`super::Panel`]/[`super::Modal`].
    /// Drawing this list directly on the raw screen background instead needs a manual
    /// `.item_style(...)` override afterwards.
    ///
    /// Call before any manual [`List::item_style`]/[`List::selected_style`] override you want to
    /// keep.
    #[must_use]
    pub fn theme(self, theme: Theme) -> Self {
        self.theme_on(theme, theme.panel_bg)
    }

    /// Same as [`List::theme`], but `item_style` is drawn on `bg` instead of `theme.panel_bg` --
    /// for a list drawn directly on a backdrop other than a themed [`super::Panel`]/
    /// [`super::Modal`]'s fill. [`List::theme`] is exactly `theme_on(theme, theme.panel_bg)`.
    #[must_use]
    pub fn theme_on(mut self, theme: Theme, bg: Color) -> Self {
        self.item_style = Style::new().fg(theme.fg).bg(bg);
        self.selected_style = Style::new().fg(theme.bg).bg(theme.accent);
        self
    }
}

impl List<'_> {
    /// The marker column's width in this render: [`List::highlight_symbol`]'s display width if
    /// [`List::highlight_spacing`] says to reserve it right now, `0` otherwise (including when
    /// `highlight_symbol` is empty: there's nothing to reserve room for).
    fn marker_width(&self, has_selection: bool) -> u16 {
        let symbol_width = retroglyph_core::text::width(self.highlight_symbol);
        if symbol_width == 0 {
            return 0;
        }
        match self.highlight_spacing {
            HighlightSpacing::Always => symbol_width,
            HighlightSpacing::WhenSelected => {
                if has_selection {
                    symbol_width
                } else {
                    0
                }
            }
            HighlightSpacing::Never => 0,
        }
    }

    /// The shared drawing routine both [`StatefulWidget::render`] and
    /// [`InteractiveWidget::render`] use.
    fn draw(&self, surface: &mut Surface<'_>, state: &ListState) {
        let (width, height) = (surface.width(), surface.height());
        if width == 0 || height == 0 {
            return;
        }

        let visible_items = usize::from(height);
        let selected = state.selected();
        let marker_width = self.marker_width(selected.is_some());
        let text_width = width.saturating_sub(marker_width);
        for (item_index, &item) in visible_window(self.items, state.offset(), visible_items) {
            // `item_index - state.offset()` is a row within the visible window, so it never
            // exceeds `visible_items` (this surface's own `u16` height).
            #[allow(clippy::cast_possible_truncation)]
            let row_in_window = (item_index - state.offset()) as u16;
            let y = match self.direction {
                ListDirection::TopToBottom => row_in_window,
                // `row_in_window < visible_items <= height`, so this never underflows.
                ListDirection::BottomToTop => height - 1 - row_in_window,
            };
            let is_selected = Some(item_index) == selected;
            let style = if is_selected {
                fill_rect(
                    surface,
                    Rect::new(0, y, width, 1),
                    ' ',
                    Style::new().bg(self.selected_style.background()),
                );
                self.selected_style
            } else {
                self.item_style
            };
            if marker_width > 0 {
                let marker = if is_selected {
                    self.highlight_symbol
                } else {
                    ""
                };
                let _ = draw_clipped(
                    surface,
                    (0, y),
                    marker_width.min(width),
                    marker,
                    Align::Left,
                    style,
                );
            }
            let _ = draw_clipped(
                surface,
                (marker_width, y),
                text_width,
                item,
                Align::Left,
                style,
            );
        }
    }

    /// The item index at `pos`, given `state`'s current scroll offset and [`List::direction`], or
    /// `None` if `pos` falls past the last item (not clamped to the last item).
    fn index_at(
        &self,
        area: Rect,
        state: &ListState,
        pos: retroglyph_core::grid::Pos,
    ) -> Option<usize> {
        let row = pos.y.checked_sub(area.top())?;
        let row = match self.direction {
            ListDirection::TopToBottom => row,
            ListDirection::BottomToTop => area.height().checked_sub(1)?.checked_sub(row)?,
        };
        let index = state.offset() + usize::from(row);
        (index < self.items.len()).then_some(index)
    }
}

impl StatefulWidget for List<'_> {
    type State = ListState;

    fn render(&self, surface: &mut Surface<'_>, state: &mut Self::State) {
        self.draw(surface, state);
    }
}

impl Measure for List<'_> {
    /// One row per item; `width` is ignored, since items are truncated rather than wrapped.
    fn height_for(&self, _width: u16) -> u16 {
        #[allow(clippy::cast_possible_truncation)]
        let height = self.items.len().min(usize::from(u16::MAX)) as u16;
        height
    }
}

impl<Id> InteractiveWidget<Id> for List<'_> {
    type State = ListState;

    /// A single id covers the whole list: clicking resolves which row via
    /// [`Response::pointer_pos`] and this list's own row geometry, rather than each row
    /// registering its own id.
    fn sense(&self) -> Sense {
        Sense::click() | Sense::SCROLL | Sense::HOVER
    }

    fn render(&self, surface: &mut Surface<'_>, state: &mut Self::State, response: Response<Id>) {
        let area = surface.area();

        // A click past the last row selects nothing: it's neither clamped to the last item nor
        // left as whatever was selected before.
        if response.clicked()
            && let Some(pos) = response.pointer_pos()
            && let Some(index) = self.index_at(area, state, pos)
        {
            state.select(Some(index));
        }
        let scroll_delta = response.scroll_delta();
        if scroll_delta != 0 {
            state.scroll_by(scroll_delta);
        }

        self.draw(surface, state);
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use retroglyph_core::grid::{Grid, Pos};

    use super::*;

    #[test]
    fn list_widget_highlights_the_selected_item() {
        let area = Rect::new(0, 0, 20, 2);
        let items = ["Alpha", "Bravo"];
        let list = List::new(&items);

        let mut grid = Grid::new(20, 2);
        let mut state = ListState::new();
        state.select(Some(1));
        StatefulWidget::render(&list, &mut Surface::new(&mut grid, area, 0), &mut state);

        let highlighted_bg = grid[Pos::new(0, 1)].style().background();
        let plain_bg = grid[Pos::new(0, 0)].style().background();
        assert_ne!(highlighted_bg, plain_bg);
    }

    #[test]
    fn list_widget_highlights_nothing_when_unselected() {
        let area = Rect::new(0, 0, 20, 2);
        let items = ["Alpha", "Bravo"];
        let list = List::new(&items);

        let mut grid = Grid::new(20, 2);
        let mut state = ListState::new();
        StatefulWidget::render(&list, &mut Surface::new(&mut grid, area, 0), &mut state);

        let row0_bg = grid[Pos::new(0, 0)].style().background();
        let row1_bg = grid[Pos::new(0, 1)].style().background();
        assert_eq!(row0_bg, row1_bg);
    }

    fn items<'a>(names: &[&'a str]) -> Vec<&'a str> {
        names.to_vec()
    }

    #[test]
    fn scroll_offset_renders_the_window_starting_at_offset() {
        let area = Rect::new(0, 0, 20, 2);
        let names = items(&["Alpha", "Bravo", "Charlie", "Delta"]);
        let list = List::new(&names);

        let mut grid = Grid::new(20, 2);
        let mut state = ListState::new();
        state.set_offset(2); // window is [Charlie, Delta]
        StatefulWidget::render(&list, &mut Surface::new(&mut grid, area, 0), &mut state);

        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'C');
        assert_eq!(grid[Pos::new(0, 1)].glyph(), 'D');
    }

    #[test]
    fn selection_scrolled_out_of_view_highlights_nothing() {
        let area = Rect::new(0, 0, 20, 2);
        let names = items(&["Alpha", "Bravo", "Charlie", "Delta"]);
        let list = List::new(&names);

        let mut grid = Grid::new(20, 2);
        let mut state = ListState::new();
        state.select(Some(0)); // "Alpha"
        state.set_offset(2); // but the window starts at "Charlie"
        StatefulWidget::render(&list, &mut Surface::new(&mut grid, area, 0), &mut state);

        let row0_bg = grid[Pos::new(0, 0)].style().background();
        let row1_bg = grid[Pos::new(0, 1)].style().background();
        assert_eq!(row0_bg, row1_bg); // neither visible row is highlighted
    }

    #[test]
    fn item_style_can_be_overridden() {
        let area = Rect::new(0, 0, 20, 1);
        let items = ["Alpha"];
        let custom = Style::new().fg(Color::RED);
        let list = List::new(&items).item_style(custom);

        let mut grid = Grid::new(20, 1);
        let mut state = ListState::new();
        StatefulWidget::render(&list, &mut Surface::new(&mut grid, area, 0), &mut state);

        assert_eq!(grid[Pos::new(0, 0)].style().foreground(), Color::RED);
    }

    #[test]
    fn selected_style_can_be_overridden() {
        let area = Rect::new(0, 0, 20, 1);
        let items = ["Alpha"];
        let custom = Style::new().fg(Color::GREEN).bg(Color::BLUE);
        let list = List::new(&items).selected_style(custom);

        let mut grid = Grid::new(20, 1);
        let mut state = ListState::new();
        state.select(Some(0));
        StatefulWidget::render(&list, &mut Surface::new(&mut grid, area, 0), &mut state);

        assert_eq!(grid[Pos::new(0, 0)].style().foreground(), Color::GREEN);
        assert_eq!(grid[Pos::new(0, 0)].style().background(), Color::BLUE);
    }

    #[test]
    fn clips_long_items_to_area_width() {
        let area = Rect::new(0, 0, 5, 1);
        let items = ["a much longer item than fits"];
        let list = List::new(&items);

        let mut grid = Grid::new(5, 1);
        let mut state = ListState::new();
        StatefulWidget::render(&list, &mut Surface::new(&mut grid, area, 0), &mut state);

        assert_eq!(grid[Pos::new(4, 0)].glyph(), 'c'); // "a muc"
    }

    #[test]
    fn ensure_visible_before_render_keeps_selection_on_screen() {
        let area = Rect::new(0, 0, 20, 2); // 2 visible items
        let names = items(&["Alpha", "Bravo", "Charlie", "Delta"]);
        let list = List::new(&names);

        let mut grid = Grid::new(20, 2);
        let mut state = ListState::new();
        state.select(Some(3)); // "Delta", off the front of the default window
        state.ensure_visible(2);
        StatefulWidget::render(&list, &mut Surface::new(&mut grid, area, 0), &mut state);

        assert_eq!(grid[Pos::new(0, 1)].glyph(), 'D');
        let highlighted_bg = grid[Pos::new(0, 1)].style().background();
        let plain_bg = grid[Pos::new(0, 0)].style().background();
        assert_ne!(highlighted_bg, plain_bg);
    }

    #[test]
    fn height_for_is_the_item_count() {
        let items = ["Alpha", "Bravo", "Charlie"];
        assert_eq!(List::new(&items).height_for(80), 3);
    }

    #[test]
    fn zero_height_is_a_no_op() {
        let area = Rect::new(0, 0, 20, 0);
        let items = ["Alpha"];
        let list = List::new(&items);

        let mut grid = Grid::new(20, 1);
        let mut state = ListState::new();
        StatefulWidget::render(&list, &mut Surface::new(&mut grid, area, 0), &mut state);

        assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
    }

    #[test]
    fn theme_maps_named_roles_onto_item_and_selected_styles() {
        let area = Rect::new(0, 0, 20, 2);
        let items = ["Alpha", "Bravo"];
        let list = List::new(&items).theme(Theme::DARK);

        let mut grid = Grid::new(20, 2);
        let mut state = ListState::new();
        state.select(Some(1));
        StatefulWidget::render(&list, &mut Surface::new(&mut grid, area, 0), &mut state);

        assert_eq!(grid[Pos::new(0, 0)].style().foreground(), Theme::DARK.fg);
        assert_eq!(
            grid[Pos::new(0, 0)].style().background(),
            Theme::DARK.panel_bg
        );
        assert_eq!(grid[Pos::new(0, 1)].style().foreground(), Theme::DARK.bg);
        assert_eq!(
            grid[Pos::new(0, 1)].style().background(),
            Theme::DARK.accent
        );
    }

    #[test]
    fn theme_on_uses_the_given_backdrop_instead_of_panel_bg() {
        let area = Rect::new(0, 0, 20, 1);
        let items = ["Alpha"];
        let list = List::new(&items).theme_on(Theme::DARK, Color::Default);

        let mut grid = Grid::new(20, 1);
        let mut state = ListState::new();
        StatefulWidget::render(&list, &mut Surface::new(&mut grid, area, 0), &mut state);

        assert_eq!(grid[Pos::new(0, 0)].style().foreground(), Theme::DARK.fg);
        assert_eq!(grid[Pos::new(0, 0)].style().background(), Color::Default);
    }

    #[test]
    fn click_selects_the_row_under_the_pointer() {
        let area = Rect::new(0, 0, 20, 3);
        let names = items(&["Alpha", "Bravo", "Charlie"]);
        let list = List::new(&names);
        let mut state = ListState::new();

        let response: Response<()> = Response {
            hovered: true,
            clicked: true,
            pointer_pos: Some(Pos::new(2, 1)), // row 1 -> "Bravo"
            ..Response::default()
        };
        let mut grid = Grid::new(20, 3);
        InteractiveWidget::render(
            &list,
            &mut Surface::new(&mut grid, area, 0),
            &mut state,
            response,
        );
        assert_eq!(state.selected(), Some(1));
    }

    #[test]
    fn click_selects_the_row_under_the_pointer_with_a_scroll_offset() {
        let area = Rect::new(0, 0, 20, 2); // 2 visible rows
        let names = items(&["Alpha", "Bravo", "Charlie", "Delta"]);
        let list = List::new(&names);
        let mut state = ListState::new();
        state.set_offset(2); // window is [Charlie, Delta]

        let response: Response<()> = Response {
            hovered: true,
            clicked: true,
            pointer_pos: Some(Pos::new(2, 1)), // row 1 of the window -> "Delta" (index 3)
            ..Response::default()
        };
        let mut grid = Grid::new(20, 2);
        InteractiveWidget::render(
            &list,
            &mut Surface::new(&mut grid, area, 0),
            &mut state,
            response,
        );
        assert_eq!(state.selected(), Some(3));
    }

    #[test]
    fn wheel_scroll_moves_the_offset() {
        let area = Rect::new(0, 0, 20, 2);
        let names = items(&["Alpha", "Bravo", "Charlie", "Delta"]);
        let list = List::new(&names);
        let mut state = ListState::new();

        let response: Response<()> = Response {
            scroll_delta: 2,
            ..Response::default()
        };
        let mut grid = Grid::new(20, 2);
        InteractiveWidget::render(
            &list,
            &mut Surface::new(&mut grid, area, 0),
            &mut state,
            response,
        );
        assert_eq!(state.offset(), 2);
    }

    #[test]
    fn highlight_symbol_prefixes_only_the_selected_item() {
        let area = Rect::new(0, 0, 20, 2);
        let items = ["Alpha", "Bravo"];
        let list = List::new(&items).highlight_symbol("> ");

        let mut grid = Grid::new(20, 2);
        let mut state = ListState::new();
        state.select(Some(1));
        StatefulWidget::render(&list, &mut Surface::new(&mut grid, area, 0), &mut state);

        assert_eq!(grid[Pos::new(0, 1)].glyph(), '>');
        assert_eq!(grid[Pos::new(1, 1)].glyph(), ' ');
        assert_eq!(grid[Pos::new(2, 1)].glyph(), 'B');
        // Unselected row's marker column is blank, and its text still starts past it.
        assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
        assert_eq!(grid[Pos::new(2, 0)].glyph(), 'A');
    }

    #[test]
    fn list_highlight_symbol_does_not_bleed_onto_the_next_row() {
        let area = Rect::new(0, 0, 2, 2);
        let names = items(&["A", "B"]);
        let list = List::new(&names).highlight_symbol(">>>"); // marker is wider than the list

        let mut grid = Grid::new(2, 2);
        let mut state = ListState::new();
        state.select(Some(0));
        StatefulWidget::render(&list, &mut Surface::new(&mut grid, area, 0), &mut state);

        // Row 1 belongs to "B", so it must not show any spillover from row 0's marker (the
        // marker is wider than the whole list, so there's no room left for item text either).
        assert_eq!(grid[Pos::new(0, 1)].glyph(), ' ');
        assert_eq!(grid[Pos::new(1, 1)].glyph(), ' ');
    }

    #[test]
    fn no_highlight_symbol_is_the_default_style_only_behavior() {
        let area = Rect::new(0, 0, 20, 1);
        let items = ["Alpha"];
        let list = List::new(&items);

        let mut grid = Grid::new(20, 1);
        let mut state = ListState::new();
        state.select(Some(0));
        StatefulWidget::render(&list, &mut Surface::new(&mut grid, area, 0), &mut state);

        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'A');
    }

    #[test]
    fn highlight_spacing_when_selected_reserves_no_column_until_something_is_selected() {
        let area = Rect::new(0, 0, 20, 1);
        let items = ["Alpha"];
        let list = List::new(&items).highlight_symbol("> ");

        let mut grid = Grid::new(20, 1);
        let mut state = ListState::new();
        StatefulWidget::render(&list, &mut Surface::new(&mut grid, area, 0), &mut state);

        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'A');
    }

    #[test]
    fn highlight_spacing_always_reserves_the_column_even_when_unselected() {
        let area = Rect::new(0, 0, 20, 1);
        let items = ["Alpha"];
        let list = List::new(&items)
            .highlight_symbol("> ")
            .highlight_spacing(HighlightSpacing::Always);

        let mut grid = Grid::new(20, 1);
        let mut state = ListState::new();
        StatefulWidget::render(&list, &mut Surface::new(&mut grid, area, 0), &mut state);

        assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
        assert_eq!(grid[Pos::new(2, 0)].glyph(), 'A');
    }

    #[test]
    fn highlight_spacing_never_suppresses_the_symbol_even_when_selected() {
        let area = Rect::new(0, 0, 20, 1);
        let items = ["Alpha"];
        let list = List::new(&items)
            .highlight_symbol("> ")
            .highlight_spacing(HighlightSpacing::Never);

        let mut grid = Grid::new(20, 1);
        let mut state = ListState::new();
        state.select(Some(0));
        StatefulWidget::render(&list, &mut Surface::new(&mut grid, area, 0), &mut state);

        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'A');
    }

    #[test]
    fn bottom_to_top_direction_draws_the_first_visible_item_at_the_bottom_row() {
        let area = Rect::new(0, 0, 20, 3);
        let names = items(&["Alpha", "Bravo", "Charlie"]);
        let list = List::new(&names).direction(ListDirection::BottomToTop);

        let mut grid = Grid::new(20, 3);
        let mut state = ListState::new();
        StatefulWidget::render(&list, &mut Surface::new(&mut grid, area, 0), &mut state);

        assert_eq!(grid[Pos::new(0, 2)].glyph(), 'A');
        assert_eq!(grid[Pos::new(0, 1)].glyph(), 'B');
        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'C');
    }

    #[test]
    fn bottom_to_top_direction_highlights_the_selected_row_in_its_flipped_position() {
        let area = Rect::new(0, 0, 20, 3);
        let names = items(&["Alpha", "Bravo", "Charlie"]);
        let list = List::new(&names).direction(ListDirection::BottomToTop);

        let mut grid = Grid::new(20, 3);
        let mut state = ListState::new();
        state.select(Some(0)); // "Alpha" draws at the bottom row under this direction
        StatefulWidget::render(&list, &mut Surface::new(&mut grid, area, 0), &mut state);

        let highlighted_bg = grid[Pos::new(0, 2)].style().background();
        let plain_bg = grid[Pos::new(0, 1)].style().background();
        assert_ne!(highlighted_bg, plain_bg);
    }

    #[test]
    fn bottom_to_top_direction_click_selects_the_flipped_row() {
        let area = Rect::new(0, 0, 20, 3);
        let names = items(&["Alpha", "Bravo", "Charlie"]);
        let list = List::new(&names).direction(ListDirection::BottomToTop);
        let mut state = ListState::new();

        let response: Response<()> = Response {
            hovered: true,
            clicked: true,
            pointer_pos: Some(Pos::new(2, 2)), // bottom row -> "Alpha" under this direction
            ..Response::default()
        };
        let mut grid = Grid::new(20, 3);
        InteractiveWidget::render(
            &list,
            &mut Surface::new(&mut grid, area, 0),
            &mut state,
            response,
        );
        assert_eq!(state.selected(), Some(0));
    }
}
