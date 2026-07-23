//! [`Log`]: a scrolled-back tail of message lines.
use retroglyph_core::text::Line;
use retroglyph_core::{Backend, Rect, Terminal};

use super::{PrintLine, Widget};

/// The tail of `messages` that fits in the area it's rendered into, oldest
/// at top, newest at the bottom, each line clipped to `area.width()` via
/// [`PrintLine`].
///
/// `offset` (set via [`Log::offset`], default `0`) scrolls back through
/// history: `0` shows the most recent messages, and each increment moves
/// the window one message further into the past. Like
/// [`Table`](super::Table)'s `state.offset()`, this does not clamp `offset`
/// -- scrolling back past the start of `messages` shows fewer (or zero)
/// lines rather than wrapping or panicking, and it's the caller's
/// responsibility to stop incrementing `offset` past `messages.len()` if
/// that's undesired. This is a different windowing direction than
/// `Table`'s (anchored to the most recent entry and counting backward,
/// rather than anchored to the start and counting forward), so it isn't
/// expressed as the same shared helper.
///
/// `messages` is a plain slice the caller owns and appends to (the same
/// division of labor as [`ListState`](crate::ListState) for selection):
/// this widget only reads it. Rows beyond the available messages are left
/// untouched -- compose with [`fill_rect`](crate::draw::fill_rect) first
/// for a solid background if one is wanted.
///
/// # Examples
///
/// ```
/// use retroglyph_core::text::Line;
/// use retroglyph_core::{Headless, Rect, Terminal};
/// use retroglyph_widgets::{Log, Widget};
///
/// let messages = [Line::raw("connected"), Line::raw("joined #general")];
/// let mut term = Terminal::new(Headless::new(20, 2));
/// Log::new(&messages).render(Rect::new(0, 0, 20, 2), &mut term);
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

impl<B: Backend> Widget<B> for Log<'_> {
    fn render(self, area: Rect, term: &mut Terminal<B>) {
        let visible_height = area.height_usize();
        if area.width() == 0 || visible_height == 0 {
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

        for (row, message) in self.messages[top..=bottom].iter().enumerate() {
            let y = area.top() + row as u16;
            let row_area = Rect::new(area.left(), y, area.width(), 1);
            PrintLine::new(message).render(row_area, term);
        }
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::Headless;

    use super::*;

    fn lines(texts: &[&str]) -> Vec<Line> {
        texts.iter().map(|t| Line::raw(*t)).collect()
    }

    #[test]
    fn shows_the_most_recent_messages_oldest_at_top() {
        // 2 visible rows; 4 messages, so only the last two should show.
        let area = Rect::new(0, 0, 20, 2);
        let messages = lines(&["alpha", "bravo", "charlie", "delta"]);

        let mut term = Terminal::new(Headless::new(20, 2));
        Log::new(&messages).render(area, &mut term);

        assert_eq!(term.grid().get(0, 0).glyph(), 'c'); // "charlie"
        assert_eq!(term.grid().get(0, 1).glyph(), 'd'); // "delta"
    }

    #[test]
    fn offset_scrolls_back_through_history() {
        let area = Rect::new(0, 0, 20, 2);
        let messages = lines(&["alpha", "bravo", "charlie", "delta"]);

        let mut term = Terminal::new(Headless::new(20, 2));
        Log::new(&messages).offset(1).render(area, &mut term); // one message back from the tail

        assert_eq!(term.grid().get(0, 0).glyph(), 'b'); // "bravo"
        assert_eq!(term.grid().get(0, 1).glyph(), 'c'); // "charlie"
    }

    #[test]
    fn offset_past_the_start_shows_fewer_lines_without_panicking() {
        let area = Rect::new(0, 0, 20, 2);
        let messages = lines(&["alpha", "bravo"]);

        let mut term = Terminal::new(Headless::new(20, 2));
        Log::new(&messages).offset(5).render(area, &mut term); // scrolled back past the start

        // Nothing drawn; both rows stay whatever they were (default/empty).
        assert_eq!(term.grid().get(0, 0).glyph(), ' ');
        assert_eq!(term.grid().get(0, 1).glyph(), ' ');
    }

    #[test]
    fn fewer_messages_than_visible_rows_leaves_the_rest_untouched() {
        let area = Rect::new(0, 0, 20, 4);
        let messages = lines(&["only"]);

        let mut term = Terminal::new(Headless::new(20, 4));
        Log::new(&messages).render(area, &mut term);

        assert_eq!(term.grid().get(0, 0).glyph(), 'o'); // "only"
        assert_eq!(term.grid().get(0, 1).glyph(), ' '); // untouched
        assert_eq!(term.grid().get(0, 2).glyph(), ' '); // untouched
    }

    #[test]
    fn clips_long_lines_to_area_width() {
        let area = Rect::new(0, 0, 5, 1);
        let messages = lines(&["a much longer message than fits"]);

        let mut term = Terminal::new(Headless::new(5, 1));
        Log::new(&messages).render(area, &mut term);

        // "a much longer..." clipped to 5 columns is "a muc".
        assert_eq!(term.grid().get(4, 0).glyph(), 'c');
    }
}
