//! Shared horizontal label-strip layout/hit-test math ([`Tabs`](super::Tabs),
//! [`MenuBar`](super::MenuBar)): both lay out `titles` left to right, `column_spacing` columns
//! apart, stopping once a title would start past the area's right edge, and hit-test against
//! that same layout. Drawing (styles, dividers, highlights) stays with each widget; only the
//! column math is shared here, so painting and hit-testing can't diverge.
use retroglyph_core::grid::{Pos, Rect};
use retroglyph_core::text::truncate_measured;

pub(super) struct Strip<'a> {
    titles: &'a [&'a str],
    column_spacing: u16,
}

impl<'a> Strip<'a> {
    pub(super) const fn new(titles: &'a [&'a str], column_spacing: u16) -> Self {
        Self {
            titles,
            column_spacing,
        }
    }

    /// Each drawn column's `(index, start_x, text_width)`, left to right, stopping once a title
    /// would start past `area`'s right edge: the same layout a widget's own `draw` paints and
    /// [`Strip::index_at`] hit-tests against, computed once here so the two can't diverge. `x`
    /// values are absolute grid coordinates (matching `area`'s own space), the same space
    /// `Response::pointer_pos` reports in, since [`Strip::index_at`] compares them directly.
    pub(super) fn columns(&self, area: Rect) -> impl Iterator<Item = (usize, u16, u16)> + '_ {
        let right = area.right();
        let spacing = self.column_spacing;
        let len = self.titles.len();
        let mut x = area.left();
        self.titles
            .iter()
            .enumerate()
            .map_while(move |(index, &title)| {
                if x >= right {
                    return None;
                }
                // x < right per the check above, so this subtraction fits a u16.
                let avail = right - x;
                let (_text, text_width) = truncate_measured(title, avail);
                let start_x = x;
                x = x.saturating_add(text_width);
                if index + 1 < len {
                    x = x.saturating_add(spacing);
                }
                Some((index, start_x, text_width))
            })
    }

    /// The index of the column whose range contains `pos`, or `None` if `pos` falls in the
    /// spacing between columns, past the last drawn column, or outside `area` entirely: a click
    /// there selects nothing rather than clamping to the nearest column.
    pub(super) fn index_at(&self, area: Rect, pos: Pos) -> Option<usize> {
        if !area.contains_pos(pos) {
            return None;
        }
        self.columns(area)
            .find(|&(_, x, width)| pos.x >= x && pos.x < x + width)
            .map(|(index, _, _)| index)
    }
}
