//! Shared core of [`super::Gauge`] and [`super::StatBar`]: `label`, then a
//! bar filling `ratio` (clamped here to `0.0..=1.0` for the fill width and
//! color, so an out-of-range `ratio` just caps the bar rather than
//! under/overflowing it) of the remaining width colored by a caller-supplied
//! `fill_color` ramp (defaulting to [`super::Meter`]'s), then a
//! caller-formatted trailing `readout` string.
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
//! loops: the same widget a caller would reach for on its own, used here
//! internally for the same reason [`super::Panel`] composes
//! [`super::BoxBorder`] rather than duplicating its drawing loop.

use core::fmt;

use retroglyph_core::text::width_usize as measured_width;
use retroglyph_core::{Color, Rect, Style};

use super::{Meter, Text, Widget};
use crate::Surface;

/// The default `fill_color` for [`super::Gauge`] and [`super::StatBar`]: [`Meter`]'s
/// green→yellow→red load ramp. A plain fn item (not a closure) so it coerces to the
/// `fn(f32) -> Color` type both widgets store, keeping them `Copy`.
pub(super) fn meter_fill_color(ratio: f32) -> Color {
    Meter::new(ratio).color()
}

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

pub(super) fn render(
    surface: &mut Surface<'_>,
    label: &str,
    label_style: Style,
    ratio: f32,
    readout: &str,
    fill_color: fn(f32) -> Color,
) {
    let width = surface.width();
    if width < 4 {
        return;
    }
    let ratio = ratio.clamp(0.0, 1.0);
    let color = fill_color(ratio);

    // Layout: "<label> [########----]  <readout>". Direct `put` calls below address this
    // surface's own local (0, 0)..(width, 1) row; sub-widgets are handed a `scope`d rect
    // instead, which (unlike `put`) addresses the same grid-space `surface.area()` does, so
    // those rects are built from `area`'s own top-left.
    let area = surface.area();
    let width_usize = usize::from(width);
    let label_w = measured_width(label).min(width_usize);
    let reserved = label_w + 1 + measured_width(readout) + 1; // label + space + gap + readout
    let bar_w = width_usize.saturating_sub(reserved);

    // `label_w` is `measured_width(label).min(width_usize)`, so it never exceeds this surface's
    // own `u16` width: narrowing it back is always exact.
    #[allow(clippy::cast_possible_truncation)]
    let label_w_u16 = label_w as u16;
    let label_area = Rect::new(area.left(), area.top(), label_w_u16, 1);
    Text::new(label)
        .style(label_style)
        .render(&mut surface.scope(label_area));
    let mut x = label_w_u16 + 1; // gap after label, in this surface's own local coordinates

    // `bar_w` is bounded by the terminal's column count (well under f32's 2^24 exact-integer
    // range), `ratio` is clamped to `0.0..=1.0`, so `filled` always lands in `0..=bar_w`.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let filled = libm::roundf(ratio * bar_w as f32) as usize;
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
        surface.put((x, 0), ch, style);
        x += 1;
    }

    x += 1; // gap before readout
    let readout_area = Rect::new(area.left() + x, area.top(), width.saturating_sub(x), 1);
    Text::new(readout)
        .style(Style::new().fg(color))
        .render(&mut surface.scope(readout_area));
}

#[cfg(test)]
mod tests {
    use core::fmt::Write as _;

    use retroglyph_core::{Grid, Pos};

    use super::*;
    use crate::Surface;

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
        let mut grid = Grid::new(20, 1);
        render(
            &mut Surface::new(&mut grid, area, 0),
            "あ",
            Style::new(),
            0.5,
            "",
            meter_fill_color,
        );

        // Bar starts right after the 2-column-wide label plus a 1-column
        // gap, i.e. at column 3, not column 4.
        assert_eq!(grid[Pos::new(3, 0)].glyph(), '█');
    }
}
