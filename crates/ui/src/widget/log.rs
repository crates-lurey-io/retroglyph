//! [`Log`]: a scrolled-back tail of message lines.
#[cfg(feature = "egc")]
use alloc::vec::Vec;

use retroglyph_core::grid::Rect;
#[cfg(feature = "egc")]
use retroglyph_core::layout::wrap;
use retroglyph_core::text::Line;

use super::{Measure, PrintLine, Widget};
use crate::Surface;

/// The tail of `messages` that fits in the area it's rendered into, oldest
/// at top, newest at the bottom, each line clipped to `area.width()` via
/// [`PrintLine`].
///
/// `offset` (set via [`Log::offset`], default `0`) scrolls back through
/// history: `0` shows the most recent messages, and each increment moves
/// the window one message further into the past. Like
/// [`Table`](super::Table)'s `state.offset()`, this does not clamp `offset`:
/// scrolling back past the start of `messages` shows fewer (or zero)
/// lines rather than wrapping or panicking, and it's the caller's
/// responsibility to stop incrementing `offset` past `messages.len()` if
/// that's undesired. This is a different windowing direction than
/// `Table`'s (anchored to the start and counting forward), so it isn't
/// expressed as the same shared helper.
///
/// `messages` is a plain slice the caller owns and appends to (the same
/// division of labor as [`ListState`](crate::state::ListState) for selection):
/// this widget only reads it. Rows beyond the available messages are left
/// untouched: compose with [`fill_rect`](crate::draw::fill_rect) first
/// for a solid background if one is wanted.
///
/// By default each message is truncated to one row via [`PrintLine`], which clips anything
/// past `area.width()`. [`Log::wrap`] (requires the `egc` feature) switches to word-wrapping
/// each message across as many rows as it needs, via [`retroglyph_core::layout::wrap`], while
/// keeping `offset` counting messages rather than rows: the window still fills from the newest
/// message backward, but a message is only included if all of its wrapped rows fit in what's
/// left of the surface, since there's no supported way to render only the bottom rows of an
/// overflowing wrapped message. A message that doesn't fully fit is left out entirely, the same
/// "rows beyond the available messages are left untouched" behavior as running out of messages.
///
/// # Examples
///
/// ```
/// use retroglyph_core::grid::Rect;
/// use retroglyph_core::text::Line;
/// use retroglyph_core::grid::Grid;
/// use retroglyph_ui::widget::{Log, Widget};
/// use retroglyph_ui::Surface;
///
/// let messages = [Line::raw("connected"), Line::raw("joined #general")];
/// let area = Rect::new(0, 0, 20, 2);
/// let mut grid = Grid::new(20, 2);
/// Log::new(&messages).render(&mut Surface::new(&mut grid, area, 0));
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Log<'a> {
    messages: &'a [Line],
    offset: usize,
    #[cfg(feature = "egc")]
    wrap: bool,
}

impl<'a> Log<'a> {
    /// A log tail over `messages`, starting at the most recent (`offset` 0).
    #[must_use]
    pub const fn new(messages: &'a [Line]) -> Self {
        Self {
            messages,
            offset: 0,
            #[cfg(feature = "egc")]
            wrap: false,
        }
    }

    /// Scroll back `offset` messages from the most recent.
    #[must_use]
    pub const fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// Word-wraps each message across as many rows as it needs instead of clipping it to one
    /// row, via [`retroglyph_core::layout::wrap`]. See the struct docs for how this interacts
    /// with `offset`. Requires the `egc` feature.
    #[cfg(feature = "egc")]
    #[must_use]
    pub const fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }
}

impl Measure for Log<'_> {
    /// One row per message when not wrapped (`width` ignored); with [`Log::wrap`] set, the sum
    /// of each message's wrapped row count at `width`. This is the height needed to show the
    /// full backlog, not just the current `offset` window.
    fn height_for(&self, width: u16) -> u16 {
        #[cfg(feature = "egc")]
        if self.wrap {
            let total: usize = self.messages.iter().map(|m| wrap(m, width).len()).sum();
            #[allow(clippy::cast_possible_truncation)]
            let height = total.min(usize::from(u16::MAX)) as u16;
            return height;
        }
        #[cfg(not(feature = "egc"))]
        let _ = width;
        #[allow(clippy::cast_possible_truncation)]
        let height = self.messages.len().min(usize::from(u16::MAX)) as u16;
        height
    }
}

impl Widget for Log<'_> {
    fn render(&self, surface: &mut Surface<'_>) {
        let (width, height) = (surface.width(), surface.height());
        let visible_height = usize::from(height);
        if width == 0 || visible_height == 0 {
            return;
        }

        // Index of the newest message in the visible window; `None` once
        // `offset` has scrolled back past the start of `messages`.
        let Some(bottom) = self
            .messages
            .len()
            .checked_sub(self.offset.saturating_add(1))
        else {
            return;
        };

        #[cfg(feature = "egc")]
        if self.wrap {
            self.render_wrapped(surface, bottom, visible_height, width);
            return;
        }

        let top = bottom.saturating_sub(visible_height - 1);

        // `scope`, unlike `put`, addresses the same grid-space `surface.area()` does, so each
        // row's rect is built from `area`'s own top-left.
        let area = surface.area();
        for (row, message) in self.messages[top..=bottom].iter().enumerate() {
            // `row` indexes a slice of at most `visible_height` messages, itself bounded by this
            // surface's own `u16` height, so narrowing it back is always exact.
            #[allow(clippy::cast_possible_truncation)]
            let y = area.top() + row as u16;
            let row_area = Rect::new(area.left(), y, width, 1);
            PrintLine::new(message).render(&mut surface.scope(row_area));
        }
    }
}

#[cfg(feature = "egc")]
impl Log<'_> {
    /// The [`Log::wrap`] render path: walks backward from `bottom` (the newest visible message)
    /// wrapping each message at `width` and accumulating its row count, until the row budget
    /// (`visible_height`) would be exceeded or `messages` is exhausted. A message whose wrapped
    /// rows would overflow the remaining budget is left out entirely rather than rendering only
    /// part of it (see the struct docs for why), so fewer than `visible_height` rows can end up
    /// drawn even with more history available.
    fn render_wrapped(
        &self,
        surface: &mut Surface<'_>,
        bottom: usize,
        visible_height: usize,
        width: u16,
    ) {
        let mut included: Vec<Vec<Line>> = Vec::new();
        let mut used = 0usize;
        for message in self.messages[..=bottom].iter().rev() {
            let rows = wrap(message, width);
            if used + rows.len() > visible_height {
                break;
            }
            used += rows.len();
            included.push(rows);
        }

        let area = surface.area();
        let mut y = area.top();
        for rows in included.into_iter().rev() {
            for row in rows {
                let row_area = Rect::new(area.left(), y, width, 1);
                PrintLine::new(&row).render(&mut surface.scope(row_area));
                y += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use retroglyph_core::grid::{Grid, Pos};

    use super::*;

    fn lines(texts: &[&str]) -> Vec<Line> {
        texts.iter().map(|t| Line::raw(*t)).collect()
    }

    #[test]
    fn shows_the_most_recent_messages_oldest_at_top() {
        // 2 visible rows; 4 messages, so only the last two should show.
        let area = Rect::new(0, 0, 20, 2);
        let messages = lines(&["alpha", "bravo", "charlie", "delta"]);

        let mut grid = Grid::new(20, 2);
        Log::new(&messages).render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'c'); // "charlie"
        assert_eq!(grid[Pos::new(0, 1)].glyph(), 'd'); // "delta"
    }

    #[test]
    fn height_for_is_the_message_count() {
        let messages = lines(&["alpha", "bravo", "charlie", "delta"]);
        assert_eq!(Log::new(&messages).height_for(80), 4);
    }

    #[test]
    fn offset_scrolls_back_through_history() {
        let area = Rect::new(0, 0, 20, 2);
        let messages = lines(&["alpha", "bravo", "charlie", "delta"]);

        let mut grid = Grid::new(20, 2);
        Log::new(&messages)
            .offset(1)
            .render(&mut Surface::new(&mut grid, area, 0)); // one message back from the tail

        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'b'); // "bravo"
        assert_eq!(grid[Pos::new(0, 1)].glyph(), 'c'); // "charlie"
    }

    #[test]
    fn offset_past_the_start_shows_fewer_lines_without_panicking() {
        let area = Rect::new(0, 0, 20, 2);
        let messages = lines(&["alpha", "bravo"]);

        let mut grid = Grid::new(20, 2);
        Log::new(&messages)
            .offset(5)
            .render(&mut Surface::new(&mut grid, area, 0)); // scrolled back past the start

        // Nothing drawn; both rows stay whatever they were (default/empty).
        assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
        assert_eq!(grid[Pos::new(0, 1)].glyph(), ' ');
    }

    #[test]
    fn fewer_messages_than_visible_rows_leaves_the_rest_untouched() {
        let area = Rect::new(0, 0, 20, 4);
        let messages = lines(&["only"]);

        let mut grid = Grid::new(20, 4);
        Log::new(&messages).render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'o'); // "only"
        assert_eq!(grid[Pos::new(0, 1)].glyph(), ' '); // untouched
        assert_eq!(grid[Pos::new(0, 2)].glyph(), ' '); // untouched
    }

    #[test]
    fn clips_long_lines_to_area_width() {
        let area = Rect::new(0, 0, 5, 1);
        let messages = lines(&["a much longer message than fits"]);

        let mut grid = Grid::new(5, 1);
        Log::new(&messages).render(&mut Surface::new(&mut grid, area, 0));

        // "a much longer..." clipped to 5 columns is "a muc".
        assert_eq!(grid[Pos::new(4, 0)].glyph(), 'c');
    }

    #[cfg(feature = "egc")]
    #[test]
    fn wrap_word_wraps_a_long_message_across_rows() {
        let area = Rect::new(0, 0, 7, 3);
        let messages = lines(&["hello world"]);

        let mut grid = Grid::new(7, 3);
        Log::new(&messages)
            .wrap(true)
            .render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'h'); // "hello"
        assert_eq!(grid[Pos::new(0, 1)].glyph(), 'w'); // "world"
        assert_eq!(grid[Pos::new(0, 2)].glyph(), ' '); // untouched
    }

    #[cfg(feature = "egc")]
    #[test]
    fn height_for_sums_wrapped_rows_when_wrap_is_set() {
        let messages = lines(&["hello world", "hi"]);
        assert_eq!(Log::new(&messages).wrap(true).height_for(7), 3); // 2 rows + 1 row
        assert_eq!(Log::new(&messages).height_for(7), 2); // unwrapped: one row per message
    }

    #[cfg(feature = "egc")]
    #[test]
    fn wrap_keeps_offset_counting_whole_messages_not_rows() {
        // "hello world" wraps to 2 rows; offset(1) should skip the whole newer message
        // ("hi", 1 row) rather than 1 row of it.
        let area = Rect::new(0, 0, 7, 4);
        let messages = lines(&["hello world", "hi"]);

        let mut grid = Grid::new(7, 4);
        Log::new(&messages)
            .wrap(true)
            .offset(1)
            .render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'h'); // "hello"
        assert_eq!(grid[Pos::new(0, 1)].glyph(), 'w'); // "world"
        assert_eq!(grid[Pos::new(0, 2)].glyph(), ' '); // "hi" scrolled out entirely
    }

    #[cfg(feature = "egc")]
    #[test]
    fn wrap_leaves_out_a_message_that_would_only_partially_fit() {
        // Only 1 row of budget left after "hi" (the newest message); "hello world" needs 2 rows,
        // so it's left out entirely rather than showing only its bottom row.
        let area = Rect::new(0, 0, 7, 2);
        let messages = lines(&["hello world", "hi"]);

        let mut grid = Grid::new(7, 2);
        Log::new(&messages)
            .wrap(true)
            .render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'h'); // "hi"
        assert_eq!(grid[Pos::new(1, 0)].glyph(), 'i');
        assert_eq!(grid[Pos::new(0, 1)].glyph(), ' '); // untouched
    }
}
