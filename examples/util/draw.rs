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
use retroglyph::color::Color;
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

// ── Dashboard widgets ─────────────────────────────────────────────────────
//
// Immediate-mode helpers for the system-monitor demo. Each takes
// `(&mut Terminal<B>, area: Rect, …)` and draws directly; none retain state.
// They stay free functions on purpose — the dashboard demo exists to decide
// whether a `Widget` trait earns its keep (see .matan/dashboard-demo.md).

/// Map a load `ratio` in `0.0..=1.0` to a green→yellow→red color ramp.
///
/// Low load is green, mid load yellow, high load red. Values outside the range
/// are clamped. Uses plain RGB lerp so it needs no cargo features.
#[must_use]
pub fn meter_ramp(ratio: f32) -> Color {
    const GREEN: (u8, u8, u8) = (80, 200, 120);
    const YELLOW: (u8, u8, u8) = (220, 200, 90);
    const RED: (u8, u8, u8) = (220, 90, 90);

    let t = ratio.clamp(0.0, 1.0);
    let (r, g, b) = if t < 0.5 {
        lerp_rgb(GREEN, YELLOW, t * 2.0)
    } else {
        lerp_rgb(YELLOW, RED, (t - 0.5) * 2.0)
    };
    Color::Rgb { r, g, b }
}

fn lerp_rgb(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let lerp = |x: u8, y: u8| {
        let v = (f32::from(y) - f32::from(x)).mul_add(t, f32::from(x));
        #[allow(clippy::cast_sign_loss)]
        {
            v.round().clamp(0.0, 255.0) as u8
        }
    };
    (lerp(a.0, b.0), lerp(a.1, b.1), lerp(a.2, b.2))
}

/// Draw a labeled gauge: a `label`, then a bar filling `ratio` (0.0–1.0) of the
/// remaining width, colored by [`meter_ramp`], with a trailing percentage.
///
/// Only the first row of `area` is used. Generalizes [`progress_bar`] with a
/// load-colored fill and inline label/readout.
pub fn gauge<B: Backend>(term: &mut Terminal<B>, area: Rect, label: &str, ratio: f32) {
    if area.width() < 4 {
        return;
    }
    let y = area.top();
    let ratio = ratio.clamp(0.0, 1.0);
    let color = meter_ramp(ratio);

    // Layout: "<label> [########----]  87%"
    let pct = format!("{:>3}%", (ratio * 100.0).round() as i32);
    let label_w = label.len().min(area.width_usize());
    let reserved = label_w + 1 + pct.len() + 1; // label + space + gap + pct
    let bar_w = area.width_usize().saturating_sub(reserved);

    let label_style = Style::new().fg(Color::Rgb {
        r: 180,
        g: 180,
        b: 200,
    });
    let mut x = area.left();
    for ch in label.chars().take(label_w) {
        term.put_styled(x, y, ch, label_style);
        x += 1;
    }
    x += 1; // gap after label

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let filled = (ratio * bar_w as f32).round() as usize;
    let filled_style = Style::new().fg(color);
    let empty_style = Style::new().fg(Color::Rgb {
        r: 50,
        g: 50,
        b: 60,
    });
    for i in 0..bar_w {
        let (ch, style) = if i < filled {
            ('█', filled_style)
        } else {
            ('░', empty_style)
        };
        term.put_styled(x, y, ch, style);
        x += 1;
    }

    x += 1; // gap before percentage
    for ch in pct.chars() {
        if x >= area.right() {
            break;
        }
        term.put_styled(x, y, ch, Style::new().fg(color));
        x += 1;
    }
    term.reset_style();
}

/// Vertical block glyphs from empty to full, indexed 0..=8.
const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Draw a single-row sparkline of `samples` across `area`, scaled to the sample
/// max, using the eight vertical block glyphs `▁▂▃▄▅▆▇█`.
///
/// The most recent samples are right-aligned so the graph scrolls left as new
/// data arrives. Bar height (and color) tracks each sample's fraction of the
/// max via [`meter_ramp`]. Only the first row of `area` is drawn.
pub fn sparkline<B: Backend>(term: &mut Terminal<B>, area: Rect, samples: &[f32]) {
    let width = area.width_usize();
    if width == 0 {
        return;
    }
    let y = area.top();
    let max = samples.iter().copied().fold(0.0_f32, f32::max).max(1e-6);

    // Take the last `width` samples so the graph is right-aligned.
    let start = samples.len().saturating_sub(width);
    let recent = &samples[start..];
    let pad = width - recent.len();

    for i in 0..width {
        let x = area.left() + i as u16;
        if i < pad {
            term.put_styled(x, y, ' ', Style::new());
            continue;
        }
        let ratio = (recent[i - pad] / max).clamp(0.0, 1.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let level = (ratio * 8.0).round() as usize;
        term.put_styled(
            x,
            y,
            BLOCKS[level.min(8)],
            Style::new().fg(meter_ramp(ratio)),
        );
    }
    term.reset_style();
}

/// Draw a fixed-column table with a highlighted `selected` row.
///
/// `headers` render on the first row of `area`; `rows` follow, one per line,
/// clipped to `area`. `widths` gives each column's cell width; columns are
/// space-separated and truncated to fit. The `selected` row (if within view)
/// is drawn with an inverted highlight background.
pub fn table<B: Backend>(
    term: &mut Terminal<B>,
    area: Rect,
    headers: &[&str],
    widths: &[u16],
    rows: &[Vec<String>],
    selected: usize,
) {
    if area.width() == 0 || area.height() == 0 {
        return;
    }
    let header_style = Style::new()
        .fg(Color::Rgb {
            r: 210,
            g: 210,
            b: 230,
        })
        .bold();
    draw_row(term, area, area.top(), headers, widths, header_style, None);

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
    for (i, row) in rows.iter().take(visible_rows).enumerate() {
        let y = area.top() + 1 + i as u16;
        let (style, bg) = if i == selected {
            (
                Style::new().fg(Color::BRIGHT_WHITE).bg(sel_bg),
                Some(sel_bg),
            )
        } else {
            (Style::new().fg(base_fg), None)
        };
        let cells: Vec<&str> = row.iter().map(String::as_str).collect();
        draw_row(term, area, y, &cells, widths, style, bg);
    }
    term.reset_style();
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
        for x in area.left()..area.right() {
            term.put_styled(x, y, ' ', Style::new().bg(bg));
        }
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
            .bg(style.background())
            .modifier(style.modifiers());
        term.print(x, y, &text);
        x = x.saturating_add(w + 1); // one-column gap between columns
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
