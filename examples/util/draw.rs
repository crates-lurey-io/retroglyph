#![allow(
    dead_code,
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::items_after_statements
)]
// Drawing helpers: box borders, filled rects, and panels.
//
// Generic over `Backend`; works with crossterm and the software renderer.
// Lives in examples/util/ until the Widget trait pattern stabilizes
// (see ADR-014 for the eventual retroglyph-widgets home).
use retroglyph::Terminal;
use retroglyph::backend::Backend;
use retroglyph::grid::{Pos, Rect};
use retroglyph::style::Style;
use retroglyph::text::Line;

// ── Box-drawing codepoints (single-line) ─────────────────────────────────────

const TL: char = '┌'; // top-left corner
const TR: char = '┐'; // top-right corner
const BL: char = '└'; // bottom-left corner
const BR: char = '┘'; // bottom-right corner
const H: char = '─'; // horizontal bar
const V: char = '│'; // vertical bar

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
        rect.width().saturating_sub(2).into(),
        rect.height().saturating_sub(2).into(),
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

/// Truncate `s` so its display width is at most `max_cols` terminal columns.
///
/// This is a simple byte-boundary truncation — good enough for ASCII/CP437
/// content in the demo. For production use, reach for `unicode-width`.
fn truncate_to_cols(s: &str, max_cols: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    let mut cols = 0usize;
    let mut end = 0usize;
    for ch in s.chars() {
        let w = ch.width().unwrap_or(0);
        if cols + w > max_cols {
            break;
        }
        cols += w;
        end += ch.len_utf8();
    }
    s[..end].to_owned()
}
