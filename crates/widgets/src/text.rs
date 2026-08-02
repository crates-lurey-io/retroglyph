//! Single-line column-clipping, unicode-width aware.
//!
//! For word-wrapping multi-line text, see `retroglyph_core::layout::TextLayout`
//! (behind the `egc` feature) rather than reimplementing wrapping here: it
//! already handles grapheme clusters, hard newlines, and per-span styling.
use retroglyph_core::text::split_at_width;

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

#[cfg(test)]
mod tests {
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
}
