//! Shared core of [`super::Gauge`] and [`super::StatBar`]: `label`, then a
//! bar filling `ratio` (clamped here to `0.0..=1.0` for the fill width and
//! color, so an out-of-range `ratio` just caps the bar rather than
//! under/overflowing it) of the remaining width colored by [`super::Meter`],
//! then a caller-formatted trailing `readout` string.
//!
//! Only the first row of `area` is used. [`super::Gauge`] and
//! [`super::StatBar`] differ only in how they compute `ratio` and format
//! `readout` (a `"87%"` percentage for `Gauge`, a `"45/100"` current/max
//! readout for `StatBar`, with `readout` free to reflect an unclamped value
//! even though the bar itself is always clamped); this function owns the
//! shared label/bar/readout layout and coloring. Crate-private: not a
//! widget in its own right, just the two widgets' common implementation.
//!
//! `label` and `readout` are both drawn via [`Text`], not hand-rolled char
//! loops -- the same widget a caller would reach for on its own, used here
//! internally for the same reason [`super::Panel`] composes
//! [`super::BoxBorder`] rather than duplicating its drawing loop.

use retroglyph_core::{Backend, Color, Rect, Style, Terminal};

use super::{Meter, Text, Widget};

/// The default label color, used when a caller doesn't set one via
/// [`super::Gauge::label_style`]/[`super::StatBar::label_style`].
pub(super) fn default_label_style() -> Style {
    Style::new().fg(Color::Rgb {
        r: 180,
        g: 180,
        b: 200,
    })
}

pub(super) fn render<B: Backend>(
    term: &mut Terminal<B>,
    area: Rect,
    label: &str,
    label_style: Style,
    ratio: f32,
    readout: &str,
) {
    if area.width() < 4 {
        return;
    }
    let y = area.top();
    let ratio = ratio.clamp(0.0, 1.0);
    let color = Meter::new(ratio).color();

    // Layout: "<label> [########----]  <readout>"
    let label_w = label.len().min(area.width_usize());
    let reserved = label_w + 1 + readout.len() + 1; // label + space + gap + readout
    let bar_w = area.width_usize().saturating_sub(reserved);

    let label_area = Rect::new(area.left(), y, label_w as u16, 1);
    Text::new(label).style(label_style).render(label_area, term);
    let mut x = area.left() + label_w as u16 + 1; // gap after label

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

    x += 1; // gap before readout
    let readout_area = Rect::new(x, y, area.right().saturating_sub(x), 1);
    Text::new(readout)
        .style(Style::new().fg(color))
        .render(readout_area, term);
    term.reset_style();
}
