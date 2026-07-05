//! [`Table`], the [`StatefulWidget`] form of [`crate::draw::table`].
use retroglyph_core::{Backend, Rect, Terminal};

use super::StatefulWidget;
use crate::ListState;

/// A fixed-column table with a [`ListState`]-driven highlighted row — the
/// [`StatefulWidget`] form of [`crate::draw::table`].
///
/// `state.offset()` is currently ignored: [`crate::draw::table`] has no
/// scrolling support of its own, so rows past the bottom of `area` simply
/// aren't drawn, selected or not. Only `state.selected()` is read. Follow-up:
/// teach `draw::table` to scroll before this can honor `offset`.
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
        // `draw::table` takes a bare row index with no "nothing selected"
        // case; mapping a missing ListState selection to an index past the
        // last row means nothing ever matches, so no row is highlighted.
        let selected = state.selected().unwrap_or(usize::MAX);
        crate::draw::table(term, area, self.headers, self.widths, self.rows, selected);
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
}
