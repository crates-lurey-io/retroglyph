//! Theme-agnostic drawing primitives: box borders, filled rects, panels,
//! progress bars, and line printing.
//!
//! Generic over [`Backend`]; works with any backend crate (crossterm,
//! software). None of these hardcode a color palette -- callers supply every
//! [`Style`] used.

use retroglyph_core::Backend;
use retroglyph_core::Line;
use retroglyph_core::Style;
use retroglyph_core::Terminal;
use retroglyph_core::{Pos, Rect};

use crate::text::truncate as truncate_to_cols;

// ── Box-drawing codepoints (single-line) ─────────────────────────────────────

// `pub(crate)`, not private: reused by `style.rs`'s `BoxStyle` border
// rendering, but not part of the public API. The enclosing `primitives`
// module is itself `pub(crate)` for the same reason, which is what makes
// clippy's redundant-pub-crate lint fire here -- allowed rather than
// restructured, since `pub(crate)` is the accurate, intentional visibility.
#[allow(clippy::redundant_pub_crate)]
pub(crate) const TL: char = '┌'; // top-left corner
#[allow(clippy::redundant_pub_crate)]
pub(crate) const TR: char = '┐'; // top-right corner
#[allow(clippy::redundant_pub_crate)]
pub(crate) const BL: char = '└'; // bottom-left corner
#[allow(clippy::redundant_pub_crate)]
pub(crate) const BR: char = '┘'; // bottom-right corner
#[allow(clippy::redundant_pub_crate)]
pub(crate) const H: char = '─'; // horizontal bar
#[allow(clippy::redundant_pub_crate)]
pub(crate) const V: char = '│'; // vertical bar

/// Draw a single-line box border around `rect` using the given `style`.
///
/// The interior of the rectangle is not touched. `rect` must be at least 2×2.
pub fn draw_box<B: Backend>(term: &mut Terminal<B>, rect: Rect, style: Style) {
    if rect.width() < 2 || rect.height() < 2 {
        return;
    }

    let x0 = rect.left();
    let y0 = rect.top();
    let x1 = rect.right().saturating_sub(1);
    let y1 = rect.bottom().saturating_sub(1);

    term.reset_style()
        .fg(style.foreground())
        .bg(style.background())
        .modifier(style.modifiers());

    // Corners
    term.put(x0, y0, TL);
    term.put(x1, y0, TR);
    term.put(x0, y1, BL);
    term.put(x1, y1, BR);

    // Horizontal edges
    for x in (x0 + 1)..x1 {
        term.put(x, y0, H);
        term.put(x, y1, H);
    }

    // Vertical edges
    for y in (y0 + 1)..y1 {
        term.put(x0, y, V);
        term.put(x1, y, V);
    }

    term.reset_style();
}

/// Fill `rect` with `ch` in the given `style`.
///
/// The entire rectangle including corners is overwritten.
pub fn fill_rect<B: Backend>(term: &mut Terminal<B>, rect: Rect, ch: char, style: Style) {
    term.reset_style()
        .fg(style.foreground())
        .bg(style.background())
        .modifier(style.modifiers());
    for y in rect.top()..rect.bottom() {
        for x in rect.left()..rect.right() {
            term.put(x, y, ch);
        }
    }
    term.reset_style();
}

/// Draw a bordered panel: a filled background with a box border and an
/// optional title centred in the top edge.
///
/// - `border_style` applies to the box outline and title.
/// - `fill_style` applies to the interior background.
/// - `title` is truncated if it doesn't fit within the top border.
pub fn panel<B: Backend>(
    term: &mut Terminal<B>,
    rect: Rect,
    title: Option<&str>,
    border_style: Style,
    fill_style: Style,
) {
    if rect.width() < 2 || rect.height() < 2 {
        return;
    }

    // Fill interior (inside the border).
    let inner = Rect::new(
        rect.left() + 1,
        rect.top() + 1,
        rect.width().saturating_sub(2),
        rect.height().saturating_sub(2),
    );
    fill_rect(term, inner, ' ', fill_style);

    draw_box(term, rect, border_style);

    // Render the title into the top border if one was provided.
    if let Some(t) = title {
        let max_title_w = rect.width().saturating_sub(4) as usize; // 2 border + 2 spaces
        if max_title_w == 0 {
            return;
        }
        // Truncate to fit.
        let t = truncate_to_cols(t, max_title_w);
        let title_x = rect.left() + (rect.width() - t.len() as u16 - 2) / 2;
        let title_y = rect.top();
        term.reset_style()
            .fg(border_style.foreground())
            .bg(border_style.background())
            .modifier(border_style.modifiers());
        term.put(title_x, title_y, ' ');
        term.print(title_x + 1, title_y, &t);
        term.put(title_x + 1 + t.len() as u16, title_y, ' ');
        term.reset_style();
    }
}

/// Draw a horizontal progress bar that fills `value / max` of `rect`.
///
/// The filled portion uses `filled_style`, the empty portion `empty_style`.
/// `rect.height()` is ignored; only the first row is drawn.
pub fn progress_bar<B: Backend>(
    term: &mut Terminal<B>,
    rect: Rect,
    value: u32,
    max: u32,
    filled_style: Style,
    empty_style: Style,
) {
    if rect.width() == 0 || max == 0 {
        return;
    }
    let filled_cells = ((value.min(max) as u64 * u64::from(rect.width())) / u64::from(max)) as u16;
    let y = rect.top();
    for x in rect.left()..rect.right() {
        let is_filled = x < rect.left() + filled_cells;
        let style = if is_filled { filled_style } else { empty_style };
        term.reset_style()
            .fg(style.foreground())
            .bg(style.background())
            .modifier(style.modifiers());
        term.put(x, y, if is_filled { '█' } else { '░' });
    }
    term.reset_style();
}

/// Print a [`Line`] at `pos`, clipping to `max_width` columns.
pub fn print_line<B: Backend>(term: &mut Terminal<B>, pos: Pos, line: &Line, max_width: u16) {
    let mut x = pos.x;
    for span in &line.spans {
        if x >= pos.x + max_width {
            break;
        }
        let remaining = (pos.x + max_width - x) as usize;
        let text = truncate_to_cols(&span.content, remaining);
        term.reset_style()
            .fg(span.style.foreground())
            .bg(span.style.background())
            .modifier(span.style.modifiers());
        term.print(x, pos.y, &text);
        x += text.len() as u16;
    }
    term.reset_style();
}

/// Draw the tail of `messages` that fits in `area`, oldest at top, newest at
/// the bottom, each line clipped to `area.width()` via [`print_line`].
///
/// `offset` scrolls back through history: `0` shows the most recent
/// messages, and each increment moves the window one message further into
/// the past. Like [`table`](super::table)'s `state.offset()`, this function
/// does not clamp `offset` -- scrolling back past the start of `messages`
/// shows fewer (or zero) lines rather than wrapping or panicking, and it's
/// the caller's responsibility to stop incrementing `offset` past
/// `messages.len()` if that's undesired. This is a different windowing
/// direction than `table`'s (anchored to the most recent entry and counting
/// backward, rather than anchored to the start and counting forward), so it
/// isn't expressed as the same shared helper -- see `visible_window` in
/// `draw/composite.rs`.
///
/// `messages` is a plain slice the caller owns and appends to (the same
/// division of labor as [`ListState`](crate::ListState) for selection):
/// this function only reads it. Rows beyond the available messages are left
/// untouched -- compose with [`fill_rect`] first for a solid background,
/// the same two-call pattern [`panel`] uses internally for its interior.
pub fn log<B: Backend>(term: &mut Terminal<B>, area: Rect, messages: &[Line], offset: usize) {
    let visible_height = area.height_usize();
    if area.width() == 0 || visible_height == 0 {
        return;
    }

    // Index of the newest message in the visible window; `None` once
    // `offset` has scrolled back past the start of `messages`.
    let Some(bottom) = messages.len().checked_sub(offset.saturating_add(1)) else {
        return;
    };
    let top = bottom.saturating_sub(visible_height - 1);

    for (row, message) in messages[top..=bottom].iter().enumerate() {
        #[allow(clippy::cast_possible_truncation)]
        let y = area.top() + row as u16;
        print_line(term, Pos::new(area.left(), y), message, area.width());
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
        log(&mut term, area, &messages, 0);

        assert_eq!(term.grid().get(0, 0).glyph(), 'c'); // "charlie"
        assert_eq!(term.grid().get(0, 1).glyph(), 'd'); // "delta"
    }

    #[test]
    fn offset_scrolls_back_through_history() {
        let area = Rect::new(0, 0, 20, 2);
        let messages = lines(&["alpha", "bravo", "charlie", "delta"]);

        let mut term = Terminal::new(Headless::new(20, 2));
        log(&mut term, area, &messages, 1); // one message back from the tail

        assert_eq!(term.grid().get(0, 0).glyph(), 'b'); // "bravo"
        assert_eq!(term.grid().get(0, 1).glyph(), 'c'); // "charlie"
    }

    #[test]
    fn offset_past_the_start_shows_fewer_lines_without_panicking() {
        let area = Rect::new(0, 0, 20, 2);
        let messages = lines(&["alpha", "bravo"]);

        let mut term = Terminal::new(Headless::new(20, 2));
        log(&mut term, area, &messages, 5); // scrolled back past the start

        // Nothing drawn; both rows stay whatever they were (default/empty).
        assert_eq!(term.grid().get(0, 0).glyph(), ' ');
        assert_eq!(term.grid().get(0, 1).glyph(), ' ');
    }

    #[test]
    fn fewer_messages_than_visible_rows_leaves_the_rest_untouched() {
        let area = Rect::new(0, 0, 20, 4);
        let messages = lines(&["only"]);

        let mut term = Terminal::new(Headless::new(20, 4));
        log(&mut term, area, &messages, 0);

        assert_eq!(term.grid().get(0, 0).glyph(), 'o'); // "only"
        assert_eq!(term.grid().get(0, 1).glyph(), ' '); // untouched
        assert_eq!(term.grid().get(0, 2).glyph(), ' '); // untouched
    }

    #[test]
    fn clips_long_lines_to_area_width() {
        let area = Rect::new(0, 0, 5, 1);
        let messages = lines(&["a much longer message than fits"]);

        let mut term = Terminal::new(Headless::new(5, 1));
        log(&mut term, area, &messages, 0);

        // "a much longer..." clipped to 5 columns is "a muc".
        assert_eq!(term.grid().get(4, 0).glyph(), 'c');
    }
}
