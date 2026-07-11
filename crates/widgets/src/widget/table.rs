//! [`Table`]: a fixed-column, scrollable table with a highlighted row.
use retroglyph_core::{Backend, Color, Rect, Style, Terminal};

use super::StatefulWidget;
use crate::ListState;
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
#[derive(Clone, Copy, Debug)]
pub struct Table<'a> {
    headers: &'a [&'a str],
    widths: &'a [u16],
    rows: &'a [Vec<String>],
}

impl<'a> Table<'a> {
    /// A table with the given header labels, column widths, and rows.
    #[must_use]
    pub const fn new(headers: &'a [&'a str], widths: &'a [u16], rows: &'a [Vec<String>]) -> Self {
        Self {
            headers,
            widths,
            rows,
        }
    }
}

impl<B: Backend> StatefulWidget<B> for Table<'_> {
    type State = ListState;

    fn render(self, area: Rect, term: &mut Terminal<B>, state: &mut Self::State) {
        if area.width() == 0 || area.height() == 0 {
            return;
        }
        let header_style = Style::new().fg(Color::Rgb {
            r: 210,
            g: 210,
            b: 230,
        });
        draw_row(
            term,
            area,
            area.top(),
            self.headers,
            self.widths,
            header_style,
            None,
        );

        let sel_bg = Color::Rgb {
            r: 40,
            g: 60,
            b: 90,
        };
        let base_fg = Color::Rgb {
            r: 170,
            g: 175,
            b: 190,
        };
        let visible_rows = area.height_usize().saturating_sub(1);
        let selected = state.selected();
        for (row_index, row) in visible_window(self.rows, state.offset(), visible_rows) {
            let y = area.top() + 1 + (row_index - state.offset()) as u16;
            let (style, bg) = if Some(row_index) == selected {
                (
                    Style::new().fg(Color::BRIGHT_WHITE).bg(sel_bg),
                    Some(sel_bg),
                )
            } else {
                (Style::new().fg(base_fg), None)
            };
            let cells: Vec<&str> = row.iter().map(String::as_str).collect();
            draw_row(term, area, y, &cells, self.widths, style, bg);
        }
        term.reset_style();
    }
}

/// The `(original_index, item)` pairs of `items` visible in a `visible_len`-
/// item window starting at `offset` -- shared windowing math for any
/// scrollable, offset-driven listing (currently [`Table`]; a future `Log`
/// window direction reuses the same idea, see [`super::Log`]'s own doc
/// comment for why it isn't literally this same function). Out-of-range
/// `offset` simply yields nothing, the same "no upper clamp, caller's
/// responsibility" contract as [`ListState::scroll_by`].
fn visible_window<T>(
    items: &[T],
    offset: usize,
    visible_len: usize,
) -> impl Iterator<Item = (usize, &T)> {
    items.iter().enumerate().skip(offset).take(visible_len)
}

/// Draw one table row of space-separated, per-column-clipped cells at row `y`.
/// When `bg` is set, the whole row width is filled with that background first.
fn draw_row<B: Backend>(
    term: &mut Terminal<B>,
    area: Rect,
    y: u16,
    cells: &[&str],
    widths: &[u16],
    style: Style,
    bg: Option<Color>,
) {
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
        term.print(x, y, &text);
        x = x.saturating_add(w + 1); // one-column gap between columns
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
        let rows = vec![vec!["Alpha".to_string()], vec!["Bravo".to_string()]];
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
        let rows = vec![vec!["Alpha".to_string()], vec!["Bravo".to_string()]];
        let table = Table::new(&headers, &widths, &rows);

        let mut term = Terminal::new(Headless::new(20, 3));
        let mut state = ListState::new(); // nothing selected
        table.render(area, &mut term, &mut state);

        let row0_bg = term.grid().get(0, 1).style().background();
        let row1_bg = term.grid().get(0, 2).style().background();
        assert_eq!(row0_bg, row1_bg);
    }

    fn rows(names: &[&str]) -> Vec<Vec<String>> {
        names.iter().map(|n| vec![(*n).to_string()]).collect()
    }

    #[test]
    fn scroll_offset_renders_the_window_starting_at_offset() {
        // 2 visible rows (area height 3, minus the header row).
        let area = Rect::new(0, 0, 20, 3);
        let headers = ["Name"];
        let widths = [10u16];
        let rows = rows(&["Alpha", "Bravo", "Charlie", "Delta"]);
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
    fn ensure_visible_before_render_keeps_selection_on_screen() {
        let area = Rect::new(0, 0, 20, 3); // 2 visible rows
        let headers = ["Name"];
        let widths = [10u16];
        let rows = rows(&["Alpha", "Bravo", "Charlie", "Delta"]);
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
