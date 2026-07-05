//! Plain-text utilities shared across widgets: column-clipping and greedy
//! word-wrap.
//!
//! Both are aware of each character's display width, so wide/CJK characters
//! and combining marks are counted correctly, not just bytes or `char`s.
use unicode_width::UnicodeWidthChar;

/// Truncate `s` so its display width is at most `max_cols` terminal columns.
///
/// Truncates on a whole-character boundary; a character that would push the
/// total over `max_cols` is dropped along with the rest of the string.
#[must_use]
pub fn truncate(s: &str, max_cols: usize) -> String {
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

/// Greedily word-wrap `s` to `width` columns.
///
/// Words are whitespace-separated and never split; a single word wider than
/// `width` overflows its own line rather than being broken mid-word. Returns
/// no lines for an empty (or all-whitespace) string.
#[must_use]
pub fn wrap(s: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;

    for word in s.split_whitespace() {
        let word_width: usize = word.chars().map(|c| c.width().unwrap_or(0)).sum();
        if current.is_empty() {
            word.clone_into(&mut current);
            current_width = word_width;
        } else if current_width + 1 + word_width <= width {
            current.push(' ');
            current.push_str(word);
            current_width += 1 + word_width;
        } else {
            lines.push(core::mem::take(&mut current));
            word.clone_into(&mut current);
            current_width = word_width;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
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

    #[test]
    fn wrap_breaks_on_whitespace_within_budget() {
        let lines = wrap("the quick brown fox jumps", 10);
        assert_eq!(lines, vec!["the quick", "brown fox", "jumps"]);
    }

    #[test]
    fn wrap_empty_string_has_no_lines() {
        assert!(wrap("", 10).is_empty());
        assert!(wrap("   ", 10).is_empty());
    }

    #[test]
    fn wrap_overflowing_word_gets_its_own_line() {
        // "supercalifragilisticexpialidocious" is wider than the 10-col
        // budget; it is not split, just left to overflow on its own line.
        let lines = wrap("hi supercalifragilisticexpialidocious there", 10);
        assert_eq!(
            lines,
            vec!["hi", "supercalifragilisticexpialidocious", "there"]
        );
    }

    #[test]
    fn wrap_single_line_fits_as_is() {
        assert_eq!(wrap("short", 10), vec!["short"]);
    }

    #[test]
    fn wrap_counts_wide_characters_toward_the_budget() {
        // Each "あ" is 2 columns wide, so 3 of them (6 columns) already fill
        // a 6-col budget; "b" (1 more column) must overflow to the next line.
        // A naive char-count-based wrap would fit all 4 units into 6 "cols".
        let lines = wrap("あああ b", 6);
        assert_eq!(lines, vec!["あああ", "b"]);
    }

    #[test]
    fn wrap_zero_width_gives_each_word_its_own_overflowing_line() {
        let lines = wrap("a bb ccc", 0);
        assert_eq!(lines, vec!["a", "bb", "ccc"]);
    }
}
