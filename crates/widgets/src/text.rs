//! Single-line column-clipping, unicode-width aware.
//!
//! For word-wrapping multi-line text, see `retroglyph_core::layout::TextLayout`
//! (behind the `egc` feature) rather than reimplementing wrapping here: it
//! already handles grapheme clusters, hard newlines, and per-span styling.
use alloc::borrow::ToOwned as _;
use alloc::string::String;

use retroglyph_core::text::{split_at_width, truncate_measured};
use retroglyph_core::{Pos, Rect, Style};

use crate::{Align, Surface};

/// Truncate `s` so its display width is at most `max_cols` terminal columns.
///
/// Truncates on a whole-character boundary; a character that would push the
/// total over `max_cols` is dropped along with the rest of the string. A
/// thin wrapper over `retroglyph_core::text::split_at_width`; `max_cols` is
/// saturated to `u16::MAX` before splitting, matching that function's own
/// saturation.
///
/// Returns a borrowed slice of `s`, so this allocates nothing. See
/// [`truncate_owned`] if you need an owned `String` (e.g. to store past the
/// lifetime of `s`).
///
/// `max_cols` takes `impl Into<usize>` so a `Rect` dimension (`u16`) can be passed directly,
/// alongside a plain `usize`.
#[must_use]
pub fn truncate(s: &str, max_cols: impl Into<usize>) -> &str {
    let max_cols = max_cols.into();
    #[allow(clippy::cast_possible_truncation)] // clamped to u16::MAX above
    let max_cols = max_cols.min(usize::from(u16::MAX)) as u16;
    split_at_width(s, max_cols).0
}

/// Owned variant of [`truncate`]: truncate `s` to `max_cols` display columns and copy the
/// surviving prefix into a new `String`.
///
/// Prefer [`truncate`] on hot paths (it borrows instead of allocating); reach for this only when
/// an owned `String` is actually needed.
#[must_use]
pub fn truncate_owned(s: &str, max_cols: impl Into<usize>) -> String {
    truncate(s, max_cols).to_owned()
}

/// Truncate `text` to `width` columns, align it within those columns per `align`, and print it
/// into `surface` at `at`. Returns the printed text's display width in columns.
///
/// This is the truncate -> align -> print sequence every single-line widget in this crate needs,
/// collapsed to one call: [`truncate_measured`] to fit `width` and get its own display width back
/// in the same pass (a wide character can make the truncated text narrower than `width`, never
/// wider, so that width isn't just `width` itself), then
/// [`Surface::print_aligned`](retroglyph_core::Surface::print_aligned) to place and print it.
/// `Align` is a plain re-export of the `HAlign` that `print_aligned` itself takes, so no
/// conversion is needed. Delegating keeps the offset math in one place instead of a second copy
/// here: `text` is already fitted to `width` before it reaches `print_aligned`, so its own
/// internal width measurement agrees with `clipped_width` and produces the same offset.
///
/// Reach for this instead of re-deriving the sequence by hand, the same way
/// [`fill_rect`](crate::fill_rect) is reached for instead of a hand-rolled fill loop; see
/// [`Text`](crate::Text)'s and [`PrintLine`](crate::PrintLine)'s own `render` for the base case.
#[must_use]
pub fn draw_clipped(
    surface: &mut Surface<'_>,
    at: impl Into<Pos>,
    width: u16,
    text: &str,
    align: Align,
    style: Style,
) -> u16 {
    let at = at.into();
    let (clipped, clipped_width) = truncate_measured(text, width);
    surface.print_aligned(Rect::new(at.x, at.y, width, 1), clipped, align, style);
    clipped_width
}

#[cfg(test)]
mod tests {
    use retroglyph_core::{Grid, Rect};

    use super::*;

    #[test]
    fn truncate_stops_at_the_column_budget() {
        assert_eq!(truncate("hello world", 5usize), "hello");
        assert_eq!(truncate("hi", 10usize), "hi");
        assert_eq!(truncate("hi", 0usize), "");
    }

    #[test]
    fn truncate_counts_wide_characters_as_two_columns() {
        // "あ" (U+3042 HIRAGANA LETTER A) is 2 columns wide, not 1: a naive
        // `chars().count()`-based truncation would let it through at budget
        // 2, but the display width does not fit alongside "a".
        assert_eq!(truncate("aあb", 2usize), "a");
        assert_eq!(truncate("aあb", 3usize), "aあ");
        assert_eq!(truncate("ああ", 3usize), "あ");
    }

    fn row(grid: &Grid, width: u16) -> String {
        (0..width).map(|x| grid[Pos::new(x, 0)].glyph()).collect()
    }

    #[test]
    fn draw_clipped_left_aligns_and_returns_the_printed_width() {
        let mut grid = Grid::new(10, 1);
        let printed = draw_clipped(
            &mut Surface::new(&mut grid, Rect::new(0, 0, 10, 1), 0),
            (0, 0),
            10,
            "hi",
            Align::Left,
            Style::new(),
        );
        assert_eq!(printed, 2);
        assert_eq!(row(&grid, 10), "hi        ");
    }

    #[test]
    fn draw_clipped_centers_within_width() {
        let mut grid = Grid::new(10, 1);
        let _ = draw_clipped(
            &mut Surface::new(&mut grid, Rect::new(0, 0, 10, 1), 0),
            (0, 0),
            10,
            "hi",
            Align::Center,
            Style::new(),
        );
        assert_eq!(row(&grid, 10), "    hi    ");
    }

    #[test]
    fn draw_clipped_truncates_wide_characters_before_measuring() {
        // "あ" is 2 columns wide: a 3-column budget only fits one, so the printed width
        // (and thus the centering offset) must reflect that, not the raw character count.
        let mut grid = Grid::new(3, 1);
        let printed = draw_clipped(
            &mut Surface::new(&mut grid, Rect::new(0, 0, 3, 1), 0),
            (0, 0),
            3,
            "ああ",
            Align::Center,
            Style::new(),
        );
        assert_eq!(printed, 2);
        assert_eq!(row(&grid, 3), "あ  ");
    }
}
