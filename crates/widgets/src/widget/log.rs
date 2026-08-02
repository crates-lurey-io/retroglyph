//! [`Log`]: a scrolled-back tail of message lines.
use retroglyph_core::Rect;
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
/// division of labor as [`ListState`](crate::ListState) for selection):
/// this widget only reads it. Rows beyond the available messages are left
/// untouched: compose with [`fill_rect`](crate::draw::fill_rect) first
/// for a solid background if one is wanted.
///
/// # Examples
///
/// ```
/// use retroglyph_core::Rect;
/// use retroglyph_core::text::Line;
/// use retroglyph_core::Grid;
/// use retroglyph_widgets::{Log, Surface, Widget};
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
}

impl<'a> Log<'a> {
    /// A log tail over `messages`, starting at the most recent (`offset` 0).
    #[must_use]
    pub const fn new(messages: &'a [Line]) -> Self {
        Self {
            messages,
            offset: 0,
        }
    }

    /// Scroll back `offset` messages from the most recent.
    #[must_use]
    pub const fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }
}

impl Measure for Log<'_> {
    /// One row per message; `width` is ignored, since lines are truncated rather than wrapped.
    /// This is the height needed to show the full backlog, not just the current `offset` window.
    fn height_for(&self, _width: u16) -> u16 {
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

#[cfg(test)]
mod tests {
    use retroglyph_core::{Grid, Pos};

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
}
