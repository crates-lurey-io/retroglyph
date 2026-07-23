//! Single-line column-clipping, unicode-width aware.
//!
//! For word-wrapping multi-line text, see `retroglyph_core::layout::TextLayout`
//! (behind the `egc` feature) rather than reimplementing wrapping here: it
//! already handles grapheme clusters, hard newlines, and per-span styling.
use unicode_width::UnicodeWidthChar;

/// Truncate `s` so its display width is at most `max_cols` terminal columns.
///
/// Truncates on a whole-character boundary; a character that would push the
/// total over `max_cols` is dropped along with the rest of the string.
///
/// Returns a borrowed slice of `s`, so this allocates nothing. See
/// [`truncate_owned`] if you need an owned `String` (e.g. to store past the
/// lifetime of `s`).
#[must_use]
pub fn truncate(s: &str, max_cols: usize) -> &str {
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
    &s[..end]
}

/// Owned variant of [`truncate`]: truncate `s` to `max_cols` display columns and copy the
/// surviving prefix into a new `String`.
///
/// Prefer [`truncate`] on hot paths (it borrows instead of allocating); reach for this only when
/// an owned `String` is actually needed.
#[must_use]
pub fn truncate_owned(s: &str, max_cols: usize) -> String {
    truncate(s, max_cols).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_stops_at_the_column_budget() {
        assert_eq!(truncate("hello world", 5), "hello");
        assert_eq!(truncate("hi", 10), "hi");
        assert_eq!(truncate("hi", 0), "");
    }

    #[test]
    fn truncate_counts_wide_characters_as_two_columns() {
        // "あ" (U+3042 HIRAGANA LETTER A) is 2 columns wide, not 1: a naive
        // `chars().count()`-based truncation would let it through at budget
        // 2, but the display width does not fit alongside "a".
        assert_eq!(truncate("aあb", 2), "a");
        assert_eq!(truncate("aあb", 3), "aあ");
        assert_eq!(truncate("ああ", 3), "あ");
    }
}
