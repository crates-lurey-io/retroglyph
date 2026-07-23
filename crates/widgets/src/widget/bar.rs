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

use core::fmt;

use retroglyph_core::{Backend, Color, Rect, Style, Terminal};
use unicode_width::UnicodeWidthStr;

use super::{Meter, Text, Widget};

/// A fixed-capacity, stack-allocated [`fmt::Write`] sink for a widget's short trailing
/// `readout` text (a `"87%"` percentage for [`super::Gauge`], a `"45/100"` current/max pair for
/// [`super::StatBar`]), so formatting it doesn't heap-allocate a `String` every frame.
///
/// `N` is the buffer's byte capacity; pick it large enough for the caller's longest possible
/// output (e.g. `4` for a `"100%"` percentage, `24` for two `u32`s joined by `/`). Writes past
/// `N` bytes are rejected by [`fmt::Write::write_str`] returning `Err`, matching `core::fmt`'s
/// own "stop, don't panic" overflow policy; [`ReadoutBuf::as_str`] then simply returns whatever
/// was successfully written before the overflow.
pub(super) struct ReadoutBuf<const N: usize> {
    bytes: [u8; N],
    len: usize,
}

impl<const N: usize> ReadoutBuf<N> {
    /// An empty buffer, ready for `write!`.
    pub(super) const fn new() -> Self {
        Self {
            bytes: [0; N],
            len: 0,
        }
    }

    /// The bytes written so far, as a `str`.
    ///
    /// Only ASCII digits, `%`, and `/` are ever written into this buffer by [`super::Gauge`] and
    /// [`super::StatBar`], so `len` bytes are always valid UTF-8; this falls back to `""` rather
    /// than panicking if that invariant is ever broken by a future caller.
    pub(super) fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len]).unwrap_or("")
    }
}

impl<const N: usize> fmt::Write for ReadoutBuf<N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        let end = self.len + bytes.len();
        if end > N {
            return Err(fmt::Error);
        }
        self.bytes[self.len..end].copy_from_slice(bytes);
        self.len = end;
        Ok(())
    }
}

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
    let label_w = label.width().min(area.width_usize());
    let reserved = label_w + 1 + readout.width() + 1; // label + space + gap + readout
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

#[cfg(test)]
mod tests {
    use core::fmt::Write as _;

    use retroglyph_core::Headless;

    use super::*;

    #[test]
    fn readout_buf_formats_without_allocating() {
        let mut buf = ReadoutBuf::<4>::new();
        write!(buf, "{:>3}%", 87).unwrap();
        assert_eq!(buf.as_str(), " 87%");
    }

    #[test]
    fn readout_buf_rejects_writes_past_capacity_and_keeps_what_fit() {
        let mut buf = ReadoutBuf::<4>::new();
        // "12345" (5 bytes) doesn't fit in a 4-byte buffer; the write errors out and only
        // whatever was written before the overflow (nothing, here, since it overflows on the
        // very first `write_str` call) is kept.
        assert!(write!(buf, "12345").is_err());
        assert_eq!(buf.as_str(), "");
    }

    #[test]
    fn wide_char_label_uses_display_width_not_byte_length() {
        // "あ" is 1 char, 3 bytes (UTF-8), 2 display columns. A byte-length
        // `label_w` (the pre-fix bug) would reserve 3 columns for it and
        // push the bar's start one column later than it should be.
        let area = Rect::new(0, 0, 20, 1);
        let mut term = Terminal::new(Headless::new(20, 1));
        render(&mut term, area, "あ", default_label_style(), 0.5, "");

        // Bar starts right after the 2-column-wide label plus a 1-column
        // gap, i.e. at column 3, not column 4.
        assert_eq!(term.grid().get(3, 0).glyph(), '█');
    }
}
