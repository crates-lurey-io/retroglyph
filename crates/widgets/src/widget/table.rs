//! [`Table`]: a fixed-column, scrollable table with a highlighted row.
use retroglyph_core::{Color, Rect, Style};

use super::StatefulWidget;
use super::window::visible_window;
use crate::ListState;
use crate::Surface;
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
/// `header_style`, `row_style`, and `selected_style` default to [`Theme::DARK`] (as if
/// [`Table::theme`] had been called); set them with [`Table::header_style`],
/// [`Table::row_style`], and [`Table::selected_style`]. `column_spacing` defaults to `1` (a
/// single blank column between cells); set it with [`Table::column_spacing`].
///
/// # Examples
///
/// ```
/// use retroglyph_core::{Grid, Rect};
/// use retroglyph_widgets::{ListState, StatefulWidget, Surface, Table};
///
/// let headers = ["Name", "Score"];
/// let widths = [10u16, 6];
/// let rows: [&[&str]; 2] = [&["Alpha", "10"], &["Bravo", "20"]];
///
/// let mut state = ListState::new();
/// state.select(Some(1));
///
/// let area = Rect::new(0, 0, 20, 3);
/// let mut grid = Grid::new(20, 3);
/// Table::new(&headers, &widths, &rows).render(&mut Surface::new(&mut grid, area, 0), &mut state);
/// ```
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
    /// A table with the given header labels, column widths, and rows, styled from
    /// [`Theme::DARK`] (as if [`Table::theme`] had been called).
    #[must_use]
    pub fn new(headers: &'a [&'a str], widths: &'a [u16], rows: &'a [&'a [&'a str]]) -> Self {
        Self {
            headers,
            widths,
            rows,
            header_style: Style::new(),
            row_style: Style::new(),
            selected_style: Style::new(),
            column_spacing: 1,
        }
        .theme(Theme::DARK)
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
    /// on `theme.accent`: the same bright-on-accent highlight [`super::List::theme`] and
    /// [`super::Button::theme`] use.
    ///
    /// `header_style`/`row_style` always set an explicit background for the same reason, and with
    /// the same caveat, as [`super::Gauge::theme`]; see its doc comment for the full explanation.
    /// Drawing this table directly on the raw screen background instead of inside a themed panel
    /// needs a manual `.header_style(...)`/`.row_style(...)` override afterwards.
    ///
    /// Call before any manual [`Table::header_style`]/[`Table::row_style`]/
    /// [`Table::selected_style`] override you want to keep.
    #[must_use]
    pub fn theme(self, theme: Theme) -> Self {
        self.theme_on(theme, theme.panel_bg)
    }

    /// Same as [`Table::theme`], but `header_style`/`row_style` are drawn on `bg` instead of
    /// `theme.panel_bg`: for a table drawn directly on a backdrop other than a themed
    /// [`super::Panel`]/[`super::Modal`]'s fill, e.g. the raw screen background or a different
    /// panel's fill color. [`Table::theme`] is exactly `theme_on(theme, theme.panel_bg)`.
    #[must_use]
    pub fn theme_on(mut self, theme: Theme, bg: Color) -> Self {
        self.header_style = Style::new().fg(theme.fg).bg(bg);
        self.row_style = Style::new().fg(theme.dim).bg(bg);
        self.selected_style = Style::new().fg(theme.bg).bg(theme.accent);
        self
    }
}

impl StatefulWidget for Table<'_> {
    type State = ListState;

    fn render(&self, surface: &mut Surface<'_>, state: &mut Self::State) {
        let (width, height) = (surface.width(), surface.height());
        if width == 0 || height == 0 {
            return;
        }
        draw_row(
            surface,
            width,
            0,
            self.headers,
            self.widths,
            RowStyle {
                style: self.header_style,
                bg: None,
                column_spacing: self.column_spacing,
            },
        );

        let visible_rows = usize::from(height).saturating_sub(1);
        let selected = state.selected();
        for (row_index, row) in visible_window(self.rows, state.offset(), visible_rows) {
            // `row_index - state.offset()` is a row within the visible window, so it never
            // exceeds `visible_rows` (this surface's own `u16` height).
            #[allow(clippy::cast_possible_truncation)]
            let row_offset = (row_index - state.offset()) as u16;
            let y = 1 + row_offset;
            let (style, bg) = if Some(row_index) == selected {
                (self.selected_style, Some(self.selected_style.background()))
            } else {
                (self.row_style, None)
            };
            draw_row(
                surface,
                width,
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

/// Draw one table row of `column_spacing`-separated, per-column-clipped cells at row `y`, in
/// this surface's own local coordinates (`width` columns starting at `0`).
fn draw_row(
    surface: &mut Surface<'_>,
    width: u16,
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
        fill_rect(surface, Rect::new(0, y, width, 1), ' ', Style::new().bg(bg));
    }
    let mut x = 0u16;
    for (cell, &w) in cells.iter().zip(widths) {
        if x >= width {
            break;
        }
        let avail = (width - x).min(w) as usize;
        let text = truncate_to_cols(cell, avail);
        surface.print((x, y), text, style);
        x = x.saturating_add(w.saturating_add(column_spacing));
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::{Grid, Pos};

    use super::*;

    #[test]
    fn table_widget_highlights_the_selected_row() {
        let area = Rect::new(0, 0, 20, 3);
        let headers = ["Name"];
        let widths = [10u16];
        let rows: [&[&str]; 2] = [&["Alpha"], &["Bravo"]];
        let table = Table::new(&headers, &widths, &rows);

        let mut grid = Grid::new(20, 3);
        let mut state = ListState::new();
        state.select(Some(1));
        table.render(&mut Surface::new(&mut grid, area, 0), &mut state);

        // Row 1 ("Bravo") is highlighted; row 0 ("Alpha") is not.
        let highlighted_bg = grid[Pos::new(0, 2)].style().background();
        let plain_bg = grid[Pos::new(0, 1)].style().background();
        assert_ne!(highlighted_bg, plain_bg);
    }

    #[test]
    fn table_widget_highlights_nothing_when_unselected() {
        let area = Rect::new(0, 0, 20, 3);
        let headers = ["Name"];
        let widths = [10u16];
        let rows: [&[&str]; 2] = [&["Alpha"], &["Bravo"]];
        let table = Table::new(&headers, &widths, &rows);

        let mut grid = Grid::new(20, 3);
        let mut state = ListState::new(); // nothing selected
        table.render(&mut Surface::new(&mut grid, area, 0), &mut state);

        let row0_bg = grid[Pos::new(0, 1)].style().background();
        let row1_bg = grid[Pos::new(0, 2)].style().background();
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

        let mut grid = Grid::new(20, 3);
        let mut state = ListState::new();
        state.set_offset(2); // window is [Charlie, Delta]
        table.render(&mut Surface::new(&mut grid, area, 0), &mut state);

        // Row 1 is "Charlie", row 2 is "Delta"; neither "Alpha" nor "Bravo"
        // (offset 0/1) are drawn anywhere.
        assert_eq!(grid[Pos::new(0, 1)].glyph(), 'C');
        assert_eq!(grid[Pos::new(0, 2)].glyph(), 'D');
    }

    #[test]
    fn selection_scrolled_out_of_view_highlights_nothing() {
        let area = Rect::new(0, 0, 20, 3);
        let headers = ["Name"];
        let widths = [10u16];
        let rows = rows(&["Alpha", "Bravo", "Charlie", "Delta"]);
        let rows = row_refs(&rows);
        let table = Table::new(&headers, &widths, &rows);

        let mut grid = Grid::new(20, 3);
        let mut state = ListState::new();
        state.select(Some(0)); // "Alpha"
        state.set_offset(2); // but the window starts at "Charlie"
        table.render(&mut Surface::new(&mut grid, area, 0), &mut state);

        let row0_bg = grid[Pos::new(0, 1)].style().background();
        let row1_bg = grid[Pos::new(0, 2)].style().background();
        assert_eq!(row0_bg, row1_bg); // neither visible row is highlighted
    }

    #[test]
    fn default_header_style_matches_theme_dark() {
        let area = Rect::new(0, 0, 20, 2);
        let headers = ["Name"];
        let widths = [10u16];
        let rows: Vec<&[&str]> = vec![];
        let table = Table::new(&headers, &widths, &rows);

        let mut grid = Grid::new(20, 2);
        let mut state = ListState::new();
        table.render(&mut Surface::new(&mut grid, area, 0), &mut state);

        assert_eq!(grid[Pos::new(0, 0)].style().foreground(), Theme::DARK.fg);
        assert_eq!(
            grid[Pos::new(0, 0)].style().background(),
            Theme::DARK.panel_bg
        );
    }

    #[test]
    fn header_style_can_be_overridden() {
        let area = Rect::new(0, 0, 20, 2);
        let headers = ["Name"];
        let widths = [10u16];
        let rows: Vec<&[&str]> = vec![];
        let custom = Style::new().fg(Color::RED);
        let table = Table::new(&headers, &widths, &rows).header_style(custom);

        let mut grid = Grid::new(20, 2);
        let mut state = ListState::new();
        table.render(&mut Surface::new(&mut grid, area, 0), &mut state);

        assert_eq!(grid[Pos::new(0, 0)].style().foreground(), Color::RED);
    }

    #[test]
    fn selected_style_can_be_overridden() {
        let area = Rect::new(0, 0, 20, 3);
        let headers = ["Name"];
        let widths = [10u16];
        let rows: [&[&str]; 2] = [&["Alpha"], &["Bravo"]];
        let custom = Style::new().fg(Color::GREEN).bg(Color::BLUE);
        let table = Table::new(&headers, &widths, &rows).selected_style(custom);

        let mut grid = Grid::new(20, 3);
        let mut state = ListState::new();
        state.select(Some(1));
        table.render(&mut Surface::new(&mut grid, area, 0), &mut state);

        assert_eq!(grid[Pos::new(0, 2)].style().foreground(), Color::GREEN);
        assert_eq!(grid[Pos::new(0, 2)].style().background(), Color::BLUE);
    }

    #[test]
    fn theme_maps_named_roles_onto_header_row_and_selected_styles() {
        let area = Rect::new(0, 0, 20, 3);
        let headers = ["Name"];
        let widths = [10u16];
        let rows: [&[&str]; 2] = [&["Alpha"], &["Bravo"]];
        let table = Table::new(&headers, &widths, &rows).theme(Theme::DARK);

        let mut grid = Grid::new(20, 3);
        let mut state = ListState::new();
        state.select(Some(1));
        table.render(&mut Surface::new(&mut grid, area, 0), &mut state);

        assert_eq!(grid[Pos::new(0, 0)].style().foreground(), Theme::DARK.fg);
        assert_eq!(
            grid[Pos::new(0, 0)].style().background(),
            Theme::DARK.panel_bg
        );
        assert_eq!(grid[Pos::new(0, 1)].style().foreground(), Theme::DARK.dim);
        assert_eq!(
            grid[Pos::new(0, 1)].style().background(),
            Theme::DARK.panel_bg
        );
        assert_eq!(grid[Pos::new(0, 2)].style().foreground(), Theme::DARK.bg);
        assert_eq!(
            grid[Pos::new(0, 2)].style().background(),
            Theme::DARK.accent
        );
    }

    #[test]
    fn theme_on_uses_the_given_backdrop_instead_of_panel_bg() {
        let area = Rect::new(0, 0, 20, 2);
        let headers = ["Name"];
        let widths = [10u16];
        let rows: [&[&str]; 1] = [&["Alpha"]];
        let table = Table::new(&headers, &widths, &rows).theme_on(Theme::DARK, Color::Default);

        let mut grid = Grid::new(20, 2);
        let mut state = ListState::new();
        table.render(&mut Surface::new(&mut grid, area, 0), &mut state);

        assert_eq!(grid[Pos::new(0, 0)].style().foreground(), Theme::DARK.fg);
        assert_eq!(grid[Pos::new(0, 0)].style().background(), Color::Default);
        assert_eq!(grid[Pos::new(0, 1)].style().foreground(), Theme::DARK.dim);
        assert_eq!(grid[Pos::new(0, 1)].style().background(), Color::Default);
    }

    #[test]
    fn column_spacing_can_be_overridden() {
        let area = Rect::new(0, 0, 20, 1);
        let headers = ["A", "B"];
        let widths = [1u16, 1u16];
        let rows: Vec<&[&str]> = vec![];
        let table = Table::new(&headers, &widths, &rows).column_spacing(3);

        let mut grid = Grid::new(20, 1);
        let mut state = ListState::new();
        table.render(&mut Surface::new(&mut grid, area, 0), &mut state);

        // Default spacing (1) would put "B" at column 2; spacing 3 pushes
        // it out to column 4.
        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'A');
        assert_eq!(grid[Pos::new(4, 0)].glyph(), 'B');
    }

    #[test]
    fn draw_row_column_width_plus_spacing_saturates_instead_of_overflowing() {
        // A column width near `u16::MAX` combined with a nonzero `column_spacing` must not
        // overflow the intermediate `w + column_spacing` addition (see issue #315); the whole
        // expression should saturate to `u16::MAX` instead of panicking (debug) or wrapping
        // (release).
        let area = Rect::new(0, 0, 20, 1);
        let cells: [&str; 2] = ["A", "B"];
        let widths = [u16::MAX - 1, 1];
        let row_style = RowStyle {
            style: Style::new(),
            bg: None,
            column_spacing: 3,
        };

        let mut grid = Grid::new(20, 1);
        draw_row(
            &mut Surface::new(&mut grid, area, 0),
            area.width(),
            0,
            &cells,
            &widths,
            row_style,
        );

        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'A');
    }

    #[test]
    fn ensure_visible_before_render_keeps_selection_on_screen() {
        let area = Rect::new(0, 0, 20, 3); // 2 visible rows
        let headers = ["Name"];
        let widths = [10u16];
        let rows = rows(&["Alpha", "Bravo", "Charlie", "Delta"]);
        let rows = row_refs(&rows);
        let table = Table::new(&headers, &widths, &rows);

        let mut grid = Grid::new(20, 3);
        let mut state = ListState::new();
        state.select(Some(3)); // "Delta", off the front of the default window
        state.ensure_visible(2);
        table.render(&mut Surface::new(&mut grid, area, 0), &mut state);

        // ensure_visible moved the window to [2, 4): "Charlie" then "Delta",
        // with "Delta" (the selection) highlighted on the last visible row.
        assert_eq!(grid[Pos::new(0, 2)].glyph(), 'D');
        let highlighted_bg = grid[Pos::new(0, 2)].style().background();
        let plain_bg = grid[Pos::new(0, 1)].style().background();
        assert_ne!(highlighted_bg, plain_bg);
    }
}
