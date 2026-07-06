//! [`Table`], the [`StatefulWidget`] form of [`crate::draw::table`].
use retroglyph_core::{Backend, Rect, Terminal};

use super::StatefulWidget;
use crate::ListState;

/// A fixed-column, scrollable table with a [`ListState`]-driven highlighted
/// row — the [`StatefulWidget`] form of [`crate::draw::table`].
///
/// Call [`state.ensure_visible(area.height())`](ListState::ensure_visible)
/// before rendering (with the table's actual row count, header row
/// excluded) to keep the selection scrolled into view; see
/// [`crate::draw::table`] for the exact windowing contract.
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
        crate::draw::table(term, area, self.headers, self.widths, self.rows, state);
    }
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
