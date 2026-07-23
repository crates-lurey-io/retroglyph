//! [`Table`]: a fixed-column, scrollable table with a highlighted row.
use retroglyph_core::{Backend, Color, Rect, Style, Terminal};

use super::StatefulWidget;
use super::window::visible_window;
use crate::ListState;
use crate::Theme;
use crate::draw::fill_rect;
use crate::text::truncate as truncate_to_cols;

/// A fixed-column, scrollable table with a [`ListState`]-driven highlighted
/// row.
///
/// `headers` render on the first row of the area it's rendered into;
/// `rows` follow, one per line, clipped to that area. `widths` gives each
/// column's cell width; columns are space-separated and truncated to fit.
///
/// `state.offset()` is the index of the first row drawn below the header --
/// rendering draws whatever window `offset` names and does not clamp or
/// auto-scroll it, matching [`ListState`]'s existing "only the caller knows
/// the viewport height" design. Call
/// [`state.ensure_visible(visible_row_count)`](ListState::ensure_visible)
/// before rendering to keep `state.selected()` on-screen. If `selected()` is
/// `Some` and its row falls within the visible window, that row is drawn
/// with an inverted highlight background; if it has scrolled out of view,
/// no row is highlighted.
///
/// `header_style`, `row_style`, and `selected_style` each default to a fixed
/// palette (a light blue-gray header, a dim gray-blue for unselected rows,
/// and a bright-white-on-dark-blue highlight for the selected row); set them
/// with [`Table::header_style`], [`Table::row_style`], and
/// [`Table::selected_style`]. `column_spacing` defaults to `1` (a single
/// blank column between cells); set it with [`Table::column_spacing`].
#[derive(Clone, Copy, Debug)]
pub struct Table<'a> {
    headers: &'a [&'a str],
    widths: &'a [u16],
    rows: &'a [&'a [&'a str]],
    header_style: Style,
    row_style: Style,
    selected_style: Style,
    column_spacing: u16,
}

impl<'a> Table<'a> {
    /// A table with the given header labels, column widths, and rows, in the
    /// default style.
    #[must_use]
    pub fn new(headers: &'a [&'a str], widths: &'a [u16], rows: &'a [&'a [&'a str]]) -> Self {
        Self {
            headers,
            widths,
            rows,
            header_style: Style::new().fg(Color::Rgb {
                r: 210,
                g: 210,
                b: 230,
            }),
            row_style: Style::new().fg(Color::Rgb {
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
        }
    }

    /// Set the header row's style.
    #[must_use]
    pub const fn header_style(mut self, style: Style) -> Self {
        self.header_style = style;
        self
    }

    /// Set the style of unselected rows.
    #[must_use]
    pub const fn row_style(mut self, style: Style) -> Self {
        self.row_style = style;
        self
    }

    /// Set the style of the selected row, including its background fill.
    #[must_use]
    pub const fn selected_style(mut self, style: Style) -> Self {
        self.selected_style = style;
        self
    }

    /// Set the number of blank columns between cells.
    #[must_use]
    pub const fn column_spacing(mut self, spacing: u16) -> Self {
        self.column_spacing = spacing;
        self
    }

    /// Applies `theme`'s named roles to this table's row styles: `header_style` becomes
    /// `theme.fg` (brighter, matching the header's original brighter-than-row default) on
    /// `theme.panel_bg`, `row_style` becomes `theme.dim` (the same de-emphasized role a plain
    /// body row already reads as) on `theme.panel_bg`, and `selected_style` becomes `theme.bg`
    /// on `theme.accent` -- the same bright-on-accent highlight [`super::List::theme`] and
    /// [`super::Button::theme`] use.
    ///
    /// `header_style`/`row_style` always set an explicit background rather than leaving it at
    /// [`Style::new()`]'s default: an unset background isn't "transparent" once a real backend
    /// draws it (a bare `Color::Default` cell paints as solid black behind the glyph, not
    /// whatever was there before -- see `retroglyph-software`'s `DEFAULT_BG`), so this widget
    /// assumes it's drawn on `theme.panel_bg` -- true when composed with a themed
    /// [`super::Panel`]/[`super::Modal`], the common case -- rather than risk a black box behind
    /// every row on a light [`Theme`]. Drawing this table directly on the raw screen background
    /// instead of inside a themed panel needs a manual `.header_style(...)`/`.row_style(...)`
    /// override afterwards.
    ///
    /// Call before any manual [`Table::header_style`]/[`Table::row_style`]/
    /// [`Table::selected_style`] override you want to keep.
    #[must_use]
    pub fn theme(mut self, theme: Theme) -> Self {
        self.header_style = Style::new().fg(theme.fg).bg(theme.panel_bg);
        self.row_style = Style::new().fg(theme.dim).bg(theme.panel_bg);
        self.selected_style = Style::new().fg(theme.bg).bg(theme.accent);
        self
    }
}

impl<B: Backend> StatefulWidget<B> for Table<'_> {
    type State = ListState;

    fn render(self, area: Rect, term: &mut Terminal<B>, state: &mut Self::State) {
        if area.width() == 0 || area.height() == 0 {
            return;
        }
        draw_row(
            term,
            area,
            area.top(),
            self.headers,
            self.widths,
            RowStyle {
                style: self.header_style,
                bg: None,
                column_spacing: self.column_spacing,
            },
        );

        let visible_rows = area.height_usize().saturating_sub(1);
        let selected = state.selected();
        for (row_index, row) in visible_window(self.rows, state.offset(), visible_rows) {
            let y = area.top() + 1 + (row_index - state.offset()) as u16;
            let (style, bg) = if Some(row_index) == selected {
                (self.selected_style, Some(self.selected_style.background()))
            } else {
                (self.row_style, None)
            };
            draw_row(
                term,
                area,
                y,
                row,
                self.widths,
                RowStyle {
                    style,
                    bg,
                    column_spacing: self.column_spacing,
                },
            );
        }
        term.reset_style();
    }
}

/// The style and layout options for drawing one [`Table`] row, grouped to keep [`draw_row`]'s
/// argument count within clippy's limit.
#[derive(Clone, Copy)]
struct RowStyle {
    /// The text (and, for the selected row, background) style.
    style: Style,
    /// When set, the whole row width is filled with this background first.
    bg: Option<Color>,
    /// The number of blank columns between cells.
    column_spacing: u16,
}

/// Draw one table row of `column_spacing`-separated, per-column-clipped cells at row `y`.
fn draw_row<B: Backend>(
    term: &mut Terminal<B>,
    area: Rect,
    y: u16,
    cells: &[&str],
    widths: &[u16],
    row_style: RowStyle,
) {
    let RowStyle {
        style,
        bg,
        column_spacing,
    } = row_style;
    if let Some(bg) = bg {
        fill_rect(
            term,
            Rect::new(area.left(), y, area.width(), 1),
            ' ',
            Style::new().bg(bg),
        );
    }
    let mut x = area.left();
    for (cell, &w) in cells.iter().zip(widths) {
        if x >= area.right() {
            break;
        }
        let avail = (area.right() - x).min(w) as usize;
        let text = truncate_to_cols(cell, avail);
        term.reset_style()
            .fg(style.foreground())
            .bg(style.background());
        term.print(x, y, text);
        x = x.saturating_add(w + column_spacing);
    }
    term.reset_style();
}

#[cfg(test)]
mod tests {
    use retroglyph_core::Headless;

    use super::*;

    #[test]
    fn table_widget_highlights_the_selected_row() {
        let area = Rect::new(0, 0, 20, 3);
        let headers = ["Name"];
        let widths = [10u16];
        let rows: [&[&str]; 2] = [&["Alpha"], &["Bravo"]];
        let table = Table::new(&headers, &widths, &rows);

        let mut term = Terminal::new(Headless::new(20, 3));
        let mut state = ListState::new();
        state.select(Some(1));
        table.render(area, &mut term, &mut state);

        // Row 1 ("Bravo") is highlighted; row 0 ("Alpha") is not.
        let highlighted_bg = term.grid().get(0, 2).style().background();
        let plain_bg = term.grid().get(0, 1).style().background();
        assert_ne!(highlighted_bg, plain_bg);
    }

    #[test]
    fn table_widget_highlights_nothing_when_unselected() {
        let area = Rect::new(0, 0, 20, 3);
        let headers = ["Name"];
        let widths = [10u16];
        let rows: [&[&str]; 2] = [&["Alpha"], &["Bravo"]];
        let table = Table::new(&headers, &widths, &rows);

        let mut term = Terminal::new(Headless::new(20, 3));
        let mut state = ListState::new(); // nothing selected
        table.render(area, &mut term, &mut state);

        let row0_bg = term.grid().get(0, 1).style().background();
        let row1_bg = term.grid().get(0, 2).style().background();
        assert_eq!(row0_bg, row1_bg);
    }

    fn rows<'a>(names: &[&'a str]) -> Vec<[&'a str; 1]> {
        names.iter().map(|n| [*n]).collect()
    }

    fn row_refs<'a>(rows: &'a [[&'a str; 1]]) -> Vec<&'a [&'a str]> {
        rows.iter().map(<[&str; 1]>::as_slice).collect()
    }

    #[test]
    fn scroll_offset_renders_the_window_starting_at_offset() {
        // 2 visible rows (area height 3, minus the header row).
        let area = Rect::new(0, 0, 20, 3);
        let headers = ["Name"];
        let widths = [10u16];
        let rows = rows(&["Alpha", "Bravo", "Charlie", "Delta"]);
        let rows = row_refs(&rows);
        let table = Table::new(&headers, &widths, &rows);

        let mut term = Terminal::new(Headless::new(20, 3));
        let mut state = ListState::new();
        state.set_offset(2); // window is [Charlie, Delta]
        table.render(area, &mut term, &mut state);

        // Row 1 is "Charlie", row 2 is "Delta"; neither "Alpha" nor "Bravo"
        // (offset 0/1) are drawn anywhere.
        assert_eq!(term.grid().get(0, 1).glyph(), 'C');
        assert_eq!(term.grid().get(0, 2).glyph(), 'D');
    }

    #[test]
    fn selection_scrolled_out_of_view_highlights_nothing() {
        let area = Rect::new(0, 0, 20, 3);
        let headers = ["Name"];
        let widths = [10u16];
        let rows = rows(&["Alpha", "Bravo", "Charlie", "Delta"]);
        let rows = row_refs(&rows);
        let table = Table::new(&headers, &widths, &rows);

        let mut term = Terminal::new(Headless::new(20, 3));
        let mut state = ListState::new();
        state.select(Some(0)); // "Alpha"
        state.set_offset(2); // but the window starts at "Charlie"
        table.render(area, &mut term, &mut state);

        let row0_bg = term.grid().get(0, 1).style().background();
        let row1_bg = term.grid().get(0, 2).style().background();
        assert_eq!(row0_bg, row1_bg); // neither visible row is highlighted
    }

    #[test]
    fn default_header_style_matches_previous_hardcoded_color() {
        let area = Rect::new(0, 0, 20, 2);
        let headers = ["Name"];
        let widths = [10u16];
        let rows: Vec<&[&str]> = vec![];
        let table = Table::new(&headers, &widths, &rows);

        let mut term = Terminal::new(Headless::new(20, 2));
        let mut state = ListState::new();
        table.render(area, &mut term, &mut state);

        let expected = Color::Rgb {
            r: 210,
            g: 210,
            b: 230,
        };
        assert_eq!(term.grid().get(0, 0).style().foreground(), expected);
    }

    #[test]
    fn header_style_can_be_overridden() {
        let area = Rect::new(0, 0, 20, 2);
        let headers = ["Name"];
        let widths = [10u16];
        let rows: Vec<&[&str]> = vec![];
        let custom = Style::new().fg(Color::RED);
        let table = Table::new(&headers, &widths, &rows).header_style(custom);

        let mut term = Terminal::new(Headless::new(20, 2));
        let mut state = ListState::new();
        table.render(area, &mut term, &mut state);

        assert_eq!(term.grid().get(0, 0).style().foreground(), Color::RED);
    }

    #[test]
    fn selected_style_can_be_overridden() {
        let area = Rect::new(0, 0, 20, 3);
        let headers = ["Name"];
        let widths = [10u16];
        let rows: [&[&str]; 2] = [&["Alpha"], &["Bravo"]];
        let custom = Style::new().fg(Color::GREEN).bg(Color::BLUE);
        let table = Table::new(&headers, &widths, &rows).selected_style(custom);

        let mut term = Terminal::new(Headless::new(20, 3));
        let mut state = ListState::new();
        state.select(Some(1));
        table.render(area, &mut term, &mut state);

        assert_eq!(term.grid().get(0, 2).style().foreground(), Color::GREEN);
        assert_eq!(term.grid().get(0, 2).style().background(), Color::BLUE);
    }

    #[test]
    fn theme_maps_named_roles_onto_header_row_and_selected_styles() {
        let area = Rect::new(0, 0, 20, 3);
        let headers = ["Name"];
        let widths = [10u16];
        let rows: [&[&str]; 2] = [&["Alpha"], &["Bravo"]];
        let table = Table::new(&headers, &widths, &rows).theme(Theme::DARK);

        let mut term = Terminal::new(Headless::new(20, 3));
        let mut state = ListState::new();
        state.select(Some(1));
        table.render(area, &mut term, &mut state);

        assert_eq!(term.grid().get(0, 0).style().foreground(), Theme::DARK.fg);
        assert_eq!(
            term.grid().get(0, 0).style().background(),
            Theme::DARK.panel_bg
        );
        assert_eq!(term.grid().get(0, 1).style().foreground(), Theme::DARK.dim);
        assert_eq!(
            term.grid().get(0, 1).style().background(),
            Theme::DARK.panel_bg
        );
        assert_eq!(term.grid().get(0, 2).style().foreground(), Theme::DARK.bg);
        assert_eq!(
            term.grid().get(0, 2).style().background(),
            Theme::DARK.accent
        );
    }

    #[test]
    fn column_spacing_can_be_overridden() {
        let area = Rect::new(0, 0, 20, 1);
        let headers = ["A", "B"];
        let widths = [1u16, 1u16];
        let rows: Vec<&[&str]> = vec![];
        let table = Table::new(&headers, &widths, &rows).column_spacing(3);

        let mut term = Terminal::new(Headless::new(20, 1));
        let mut state = ListState::new();
        table.render(area, &mut term, &mut state);

        // Default spacing (1) would put "B" at column 2; spacing 3 pushes
        // it out to column 4.
        assert_eq!(term.grid().get(0, 0).glyph(), 'A');
        assert_eq!(term.grid().get(4, 0).glyph(), 'B');
    }

    #[test]
    fn ensure_visible_before_render_keeps_selection_on_screen() {
        let area = Rect::new(0, 0, 20, 3); // 2 visible rows
        let headers = ["Name"];
        let widths = [10u16];
        let rows = rows(&["Alpha", "Bravo", "Charlie", "Delta"]);
        let rows = row_refs(&rows);
        let table = Table::new(&headers, &widths, &rows);

        let mut term = Terminal::new(Headless::new(20, 3));
        let mut state = ListState::new();
        state.select(Some(3)); // "Delta", off the front of the default window
        state.ensure_visible(2);
        table.render(area, &mut term, &mut state);

        // ensure_visible moved the window to [2, 4): "Charlie" then "Delta",
        // with "Delta" (the selection) highlighted on the last visible row.
        assert_eq!(term.grid().get(0, 2).glyph(), 'D');
        let highlighted_bg = term.grid().get(0, 2).style().background();
        let plain_bg = term.grid().get(0, 1).style().background();
        assert_ne!(highlighted_bg, plain_bg);
    }
}
