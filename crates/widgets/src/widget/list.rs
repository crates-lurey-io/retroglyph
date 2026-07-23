//! [`List`]: a scrollable, single-column list with a [`ListState`]-driven highlighted item.
use retroglyph_core::{Backend, Color, Rect, Style, Terminal};

use super::StatefulWidget;
use super::window::visible_window;
use crate::ListState;
use crate::Theme;
use crate::draw::fill_rect;
use crate::text::truncate as truncate_to_cols;

/// A scrollable, single-column list of plain-text items with a [`ListState`]-driven highlighted
/// item -- `Table`'s single-column sibling, sharing its windowing and selection story.
///
/// One `item` renders per line, top-aligned in the area it's rendered into and clipped to
/// `area.width()`. `state.offset()` is the index of the first item drawn -- rendering draws
/// whatever window `offset` names and does not clamp or auto-scroll it, matching
/// [`Table`](super::Table)'s and [`ListState`]'s existing "only the caller knows the viewport
/// height" design. Call [`state.ensure_visible(visible_item_count)`](ListState::ensure_visible)
/// before rendering to keep `state.selected()` on-screen. If `selected()` is `Some` and its item
/// falls within the visible window, that item is drawn with an inverted highlight background; if
/// it has scrolled out of view, nothing is highlighted.
///
/// `item_style` and `selected_style` each default to the same fixed palette as
/// [`Table`](super::Table)'s `row_style`/`selected_style` (a dim gray-blue for unselected items,
/// a bright-white-on-dark-blue highlight for the selected one); set them with
/// [`List::item_style`]/[`List::selected_style`].
#[derive(Clone, Copy, Debug)]
pub struct List<'a> {
    items: &'a [&'a str],
    item_style: Style,
    selected_style: Style,
}

impl<'a> List<'a> {
    /// A list of `items` in the default style.
    #[must_use]
    pub fn new(items: &'a [&'a str]) -> Self {
        Self {
            items,
            item_style: Style::new().fg(Color::Rgb {
                r: 170,
                g: 175,
                b: 190,
            }),
            selected_style: Style::new().fg(Color::BRIGHT_WHITE).bg(Color::Rgb {
                r: 40,
                g: 60,
                b: 90,
            }),
        }
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

    /// Applies `theme`'s named roles to this list: `item_style` becomes `theme.fg` on
    /// `theme.panel_bg`, and `selected_style` becomes `theme.bg` on `theme.accent`.
    ///
    /// `item_style` sets an explicit background rather than leaving it at [`Style::new()`]'s
    /// default: an unset background isn't "transparent" once a real backend draws it (a bare
    /// `Color::Default` cell paints as solid black behind the glyph -- see
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

impl<B: Backend> StatefulWidget<B> for List<'_> {
    type State = ListState;

    fn render(self, area: Rect, term: &mut Terminal<B>, state: &mut Self::State) {
        if area.width() == 0 || area.height() == 0 {
            return;
        }

        let visible_items = area.height_usize();
        let selected = state.selected();
        for (item_index, &item) in visible_window(self.items, state.offset(), visible_items) {
            let y = area.top() + (item_index - state.offset()) as u16;
            let style = if Some(item_index) == selected {
                fill_rect(
                    term,
                    Rect::new(area.left(), y, area.width(), 1),
                    ' ',
                    Style::new().bg(self.selected_style.background()),
                );
                self.selected_style
            } else {
                self.item_style
            };
            let text = truncate_to_cols(item, area.width_usize());
            term.reset_style()
                .fg(style.foreground())
                .bg(style.background());
            term.print(area.left(), y, text);
        }
        term.reset_style();
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::Headless;

    use super::*;

    #[test]
    fn list_widget_highlights_the_selected_item() {
        let area = Rect::new(0, 0, 20, 2);
        let items = ["Alpha", "Bravo"];
        let list = List::new(&items);

        let mut term = Terminal::new(Headless::new(20, 2));
        let mut state = ListState::new();
        state.select(Some(1));
        list.render(area, &mut term, &mut state);

        let highlighted_bg = term.grid().get(0, 1).style().background();
        let plain_bg = term.grid().get(0, 0).style().background();
        assert_ne!(highlighted_bg, plain_bg);
    }

    #[test]
    fn list_widget_highlights_nothing_when_unselected() {
        let area = Rect::new(0, 0, 20, 2);
        let items = ["Alpha", "Bravo"];
        let list = List::new(&items);

        let mut term = Terminal::new(Headless::new(20, 2));
        let mut state = ListState::new();
        list.render(area, &mut term, &mut state);

        let row0_bg = term.grid().get(0, 0).style().background();
        let row1_bg = term.grid().get(0, 1).style().background();
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

        let mut term = Terminal::new(Headless::new(20, 2));
        let mut state = ListState::new();
        state.set_offset(2); // window is [Charlie, Delta]
        list.render(area, &mut term, &mut state);

        assert_eq!(term.grid().get(0, 0).glyph(), 'C');
        assert_eq!(term.grid().get(0, 1).glyph(), 'D');
    }

    #[test]
    fn selection_scrolled_out_of_view_highlights_nothing() {
        let area = Rect::new(0, 0, 20, 2);
        let names = items(&["Alpha", "Bravo", "Charlie", "Delta"]);
        let list = List::new(&names);

        let mut term = Terminal::new(Headless::new(20, 2));
        let mut state = ListState::new();
        state.select(Some(0)); // "Alpha"
        state.set_offset(2); // but the window starts at "Charlie"
        list.render(area, &mut term, &mut state);

        let row0_bg = term.grid().get(0, 0).style().background();
        let row1_bg = term.grid().get(0, 1).style().background();
        assert_eq!(row0_bg, row1_bg); // neither visible row is highlighted
    }

    #[test]
    fn item_style_can_be_overridden() {
        let area = Rect::new(0, 0, 20, 1);
        let items = ["Alpha"];
        let custom = Style::new().fg(Color::RED);
        let list = List::new(&items).item_style(custom);

        let mut term = Terminal::new(Headless::new(20, 1));
        let mut state = ListState::new();
        list.render(area, &mut term, &mut state);

        assert_eq!(term.grid().get(0, 0).style().foreground(), Color::RED);
    }

    #[test]
    fn selected_style_can_be_overridden() {
        let area = Rect::new(0, 0, 20, 1);
        let items = ["Alpha"];
        let custom = Style::new().fg(Color::GREEN).bg(Color::BLUE);
        let list = List::new(&items).selected_style(custom);

        let mut term = Terminal::new(Headless::new(20, 1));
        let mut state = ListState::new();
        state.select(Some(0));
        list.render(area, &mut term, &mut state);

        assert_eq!(term.grid().get(0, 0).style().foreground(), Color::GREEN);
        assert_eq!(term.grid().get(0, 0).style().background(), Color::BLUE);
    }

    #[test]
    fn clips_long_items_to_area_width() {
        let area = Rect::new(0, 0, 5, 1);
        let items = ["a much longer item than fits"];
        let list = List::new(&items);

        let mut term = Terminal::new(Headless::new(5, 1));
        let mut state = ListState::new();
        list.render(area, &mut term, &mut state);

        assert_eq!(term.grid().get(4, 0).glyph(), 'c'); // "a muc"
    }

    #[test]
    fn ensure_visible_before_render_keeps_selection_on_screen() {
        let area = Rect::new(0, 0, 20, 2); // 2 visible items
        let names = items(&["Alpha", "Bravo", "Charlie", "Delta"]);
        let list = List::new(&names);

        let mut term = Terminal::new(Headless::new(20, 2));
        let mut state = ListState::new();
        state.select(Some(3)); // "Delta", off the front of the default window
        state.ensure_visible(2);
        list.render(area, &mut term, &mut state);

        assert_eq!(term.grid().get(0, 1).glyph(), 'D');
        let highlighted_bg = term.grid().get(0, 1).style().background();
        let plain_bg = term.grid().get(0, 0).style().background();
        assert_ne!(highlighted_bg, plain_bg);
    }

    #[test]
    fn zero_height_is_a_no_op() {
        let area = Rect::new(0, 0, 20, 0);
        let items = ["Alpha"];
        let list = List::new(&items);

        let mut term = Terminal::new(Headless::new(20, 1));
        let mut state = ListState::new();
        list.render(area, &mut term, &mut state);

        assert_eq!(term.grid().get(0, 0).glyph(), ' ');
    }

    #[test]
    fn theme_maps_named_roles_onto_item_and_selected_styles() {
        let area = Rect::new(0, 0, 20, 2);
        let items = ["Alpha", "Bravo"];
        let list = List::new(&items).theme(Theme::DARK);

        let mut term = Terminal::new(Headless::new(20, 2));
        let mut state = ListState::new();
        state.select(Some(1));
        list.render(area, &mut term, &mut state);

        assert_eq!(term.grid().get(0, 0).style().foreground(), Theme::DARK.fg);
        assert_eq!(
            term.grid().get(0, 0).style().background(),
            Theme::DARK.panel_bg
        );
        assert_eq!(term.grid().get(0, 1).style().foreground(), Theme::DARK.bg);
        assert_eq!(
            term.grid().get(0, 1).style().background(),
            Theme::DARK.accent
        );
    }

    #[test]
    fn theme_on_uses_the_given_backdrop_instead_of_panel_bg() {
        let area = Rect::new(0, 0, 20, 1);
        let items = ["Alpha"];
        let list = List::new(&items).theme_on(Theme::DARK, Color::Default);

        let mut term = Terminal::new(Headless::new(20, 1));
        let mut state = ListState::new();
        list.render(area, &mut term, &mut state);

        assert_eq!(term.grid().get(0, 0).style().foreground(), Theme::DARK.fg);
        assert_eq!(term.grid().get(0, 0).style().background(), Color::Default);
    }
}
