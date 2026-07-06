//! Opinionated, color-baked widgets for the system-monitor dashboard demo:
//! gauges, sparklines, and tables.
//!
//! Unlike [`primitives`](super::primitives), these hardcode a specific
//! dark-theme RGB palette (see [`meter_ramp`]) rather than taking every color
//! as a parameter. They stay free functions on purpose -- the dashboard demo
//! exists to decide whether a `Widget` trait earns its keep (see
//! `.matan/dashboard-demo.md`).

use retroglyph_core::Backend;
use retroglyph_core::Color;
use retroglyph_core::Rect;
use retroglyph_core::Style;
use retroglyph_core::Terminal;

use crate::text::truncate as truncate_to_cols;

/// Map a load `ratio` in `0.0..=1.0` to a green→yellow→red color ramp.
///
/// Low load is green, mid load yellow, high load red. Values outside the range
/// are clamped. Delegates to [`Color::lerp`] (backed by `gem`) rather than
/// hand-rolling RGB interpolation.
#[must_use]
pub fn meter_ramp(ratio: f32) -> Color {
    const GREEN: Color = Color::Rgb {
        r: 80,
        g: 200,
        b: 120,
    };
    const YELLOW: Color = Color::Rgb {
        r: 220,
        g: 200,
        b: 90,
    };
    const RED: Color = Color::Rgb {
        r: 220,
        g: 90,
        b: 90,
    };

    let t = ratio.clamp(0.0, 1.0);
    if t < 0.5 {
        Color::lerp(GREEN, YELLOW, t * 2.0)
    } else {
        Color::lerp(YELLOW, RED, (t - 0.5) * 2.0)
    }
}

/// Draw a labeled gauge: a `label`, then a bar filling `ratio` (0.0–1.0) of the
/// remaining width, colored by [`meter_ramp`], with a trailing percentage.
///
/// Only the first row of `area` is used. Generalizes
/// [`progress_bar`](crate::progress_bar) with a load-colored fill and inline
/// label/readout.
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
