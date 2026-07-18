//! [`TextInputState`]: cursor/scroll state for a single-line text field.
//!
//! **Design decision: the cursor is a char index, not a grapheme cluster index.** This crate
//! already has an `egc` feature (`unicode-segmentation` + `retroglyph_core::layout::TextLayout`)
//! for grapheme-aware multi-line text, but pulling that in here for a single-line prototype would
//! mean every consumer -- even ones that never touch combining marks or ZWJ emoji -- pays for it.
//! So this state (and the [`TextInput`](crate::TextInput) widget that reads it) works in `char`
//! units: a combining mark (e.g. `"e\u{0301}"`, `e` + COMBINING ACUTE ACCENT) is two cursor stops,
//! not one, and a ZWJ emoji sequence is as many stops as it has scalar values. Documented here as
//! a prototype limitation rather than fixed, since fixing it is exactly the `egc`-dependency
//! tradeoff this file is deliberately not making.

use unicode_width::UnicodeWidthChar;

/// Cursor position, scroll offset, and value of a single-line text field.
///
/// Holds no reference to the widget that draws it, the same division of labor as [`ListState`]:
/// this is app state that outlives one render call, kept separate from
/// [`TextInput`](crate::TextInput)'s drawing logic.
///
/// `cursor` and `offset` are both **char indices** (not byte offsets, not grapheme-cluster
/// indices -- see the module docs above), so `String::insert`/`remove` calls always map through
/// [`char_indices`](str::char_indices) rather than indexing `value` directly. `cursor` ranges
/// `0..=char_count`: `char_count` itself is a valid cursor position, meaning "after the last
/// character."
///
/// [`ListState`]: crate::ListState
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextInputState {
    value: String,
    cursor: usize,
    offset: usize,
}

impl TextInputState {
    /// An empty field: no text, cursor and scroll offset both at zero.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            value: String::new(),
            cursor: 0,
            offset: 0,
        }
    }

    /// A field pre-filled with `value`, cursor placed after the last character.
    #[must_use]
    pub fn with_value(value: impl Into<String>) -> Self {
        let value = value.into();
        let cursor = value.chars().count();
        Self {
            value,
            cursor,
            offset: 0,
        }
    }

    /// The current text.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    /// The cursor position, as a char index in `0..=char_count()`.
    #[must_use]
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    /// The char index of the first visible character -- the scroll offset. Adjusted by
    /// [`ensure_cursor_visible`](Self::ensure_cursor_visible); untouched by every other method
    /// (mirrors how [`ListState::offset`](crate::ListState::offset) is only ever moved by
    /// `ensure_visible`/`scroll_by`, never as a side effect of selecting).
    #[must_use]
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// The number of characters currently in the field.
    #[must_use]
    pub fn char_count(&self) -> usize {
        self.value.chars().count()
    }

    /// Replace the entire value, moving the cursor to the end and resetting the scroll offset --
    /// the same "the old cursor/offset don't mean anything against different content" rationale
    /// as [`ListState::reset`](crate::ListState::reset).
    pub fn set_value(&mut self, value: impl Into<String>) {
        self.value = value.into();
        self.cursor = self.value.chars().count();
        self.offset = 0;
    }

    /// Clear the value, cursor, and scroll offset back to empty.
    pub fn clear(&mut self) {
        self.value.clear();
        self.cursor = 0;
        self.offset = 0;
    }

    /// Insert `ch` at the cursor and advance the cursor past it.
    pub fn insert(&mut self, ch: char) {
        let byte = self.byte_index(self.cursor);
        self.value.insert(byte, ch);
        self.cursor += 1;
    }

    /// Insert `s` at the cursor and advance the cursor past it.
    pub fn insert_str(&mut self, s: &str) {
        let byte = self.byte_index(self.cursor);
        self.value.insert_str(byte, s);
        self.cursor += s.chars().count();
    }

    /// Delete the character before the cursor and move the cursor back onto it. A no-op at the
    /// start of the field.
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = self.byte_index(self.cursor - 1);
        let end = self.byte_index(self.cursor);
        self.value.replace_range(start..end, "");
        self.cursor -= 1;
    }

    /// Delete the character under the cursor, leaving the cursor in place. A no-op at the end of
    /// the field.
    pub fn delete(&mut self) {
        let char_count = self.char_count();
        if self.cursor >= char_count {
            return;
        }
        let start = self.byte_index(self.cursor);
        let end = self.byte_index(self.cursor + 1);
        self.value.replace_range(start..end, "");
    }

    /// Move the cursor one character left, clamped at zero.
    pub const fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// Move the cursor one character right, clamped at `char_count`.
    pub fn move_right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.char_count());
    }

    /// Move the cursor to the start of the field.
    pub const fn move_home(&mut self) {
        self.cursor = 0;
    }

    /// Move the cursor to the end of the field.
    pub fn move_end(&mut self) {
        self.cursor = self.char_count();
    }

    /// Move the cursor to an explicit char index, clamped at `char_count`.
    pub fn set_cursor(&mut self, cursor: usize) {
        self.cursor = cursor.min(self.char_count());
    }

    /// Nudge the scroll offset by the minimum amount needed to bring the cursor into the
    /// `width`-column window starting at `offset` -- the [`TextInput`](crate::TextInput) analog
    /// of [`ListState::ensure_visible`](crate::ListState::ensure_visible).
    ///
    /// A no-op if `width` is zero. Column math (not char-count math) uses `unicode-width`, the
    /// same crate [`crate::text::truncate`] uses, so wide characters (CJK, emoji) count as two
    /// columns rather than one. When the cursor sits after the last character (nothing to draw a
    /// caret over), one column of the window is reserved for the caret itself; when the cursor
    /// sits on a character, the caret is drawn over that character's own cell, so no extra column
    /// is needed. Call this once per frame before rendering, with the actual current viewport
    /// width, exactly as `ListState::ensure_visible` recommends for `visible_height`.
    pub fn ensure_cursor_visible(&mut self, width: u16) {
        if width == 0 {
            return;
        }
        let width = width as usize;
        let char_count = self.char_count();

        if self.cursor < self.offset {
            self.offset = self.cursor;
            return;
        }

        let max_cols = if self.cursor == char_count {
            width.saturating_sub(1)
        } else {
            width
        };

        loop {
            if self.offset >= self.cursor {
                break;
            }
            let col = self.column_between(self.offset, self.cursor);
            if col < max_cols {
                break;
            }
            self.offset += 1;
        }
    }

    /// Sum of display widths of the characters in `[from, to)`.
    fn column_between(&self, from: usize, to: usize) -> usize {
        self.value
            .chars()
            .skip(from)
            .take(to.saturating_sub(from))
            .map(|ch| ch.width().unwrap_or(0))
            .sum()
    }

    /// Maps a char index to a byte offset into `value`, via `char_indices` (never by indexing
    /// `value` directly -- see the module docs). `n >= char_count` maps to `value.len()`, the
    /// "after the last character" position.
    fn byte_index(&self, n: usize) -> usize {
        self.value
            .char_indices()
            .nth(n)
            .map_or(self.value.len(), |(i, _)| i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_empty() {
        let s = TextInputState::new();
        assert_eq!(s.value(), "");
        assert_eq!(s.cursor(), 0);
        assert_eq!(s.offset(), 0);
    }

    #[test]
    fn with_value_places_cursor_at_the_end() {
        let s = TextInputState::with_value("hello");
        assert_eq!(s.value(), "hello");
        assert_eq!(s.cursor(), 5);
    }

    #[test]
    fn insert_advances_the_cursor() {
        let mut s = TextInputState::new();
        s.insert('a');
        s.insert('b');
        assert_eq!(s.value(), "ab");
        assert_eq!(s.cursor(), 2);
    }

    #[test]
    fn insert_mid_string_splices_at_the_cursor() {
        let mut s = TextInputState::with_value("ac");
        s.set_cursor(1);
        s.insert('b');
        assert_eq!(s.value(), "abc");
        assert_eq!(s.cursor(), 2);
    }

    #[test]
    fn backspace_at_start_is_a_no_op() {
        let mut s = TextInputState::with_value("abc");
        s.set_cursor(0);
        s.backspace();
        assert_eq!(s.value(), "abc");
        assert_eq!(s.cursor(), 0);
    }

    #[test]
    fn backspace_deletes_before_the_cursor() {
        let mut s = TextInputState::with_value("abc");
        s.set_cursor(2);
        s.backspace();
        assert_eq!(s.value(), "ac");
        assert_eq!(s.cursor(), 1);
    }

    #[test]
    fn delete_at_end_is_a_no_op() {
        let mut s = TextInputState::with_value("abc");
        s.move_end();
        s.delete();
        assert_eq!(s.value(), "abc");
        assert_eq!(s.cursor(), 3);
    }

    #[test]
    fn delete_removes_the_character_under_the_cursor() {
        let mut s = TextInputState::with_value("abc");
        s.set_cursor(1);
        s.delete();
        assert_eq!(s.value(), "ac");
        assert_eq!(s.cursor(), 1);
    }

    #[test]
    fn move_left_and_right_clamp_at_the_ends() {
        let mut s = TextInputState::with_value("ab");
        s.set_cursor(0);
        s.move_left();
        assert_eq!(s.cursor(), 0);
        s.move_right();
        s.move_right();
        s.move_right();
        assert_eq!(s.cursor(), 2);
    }

    #[test]
    fn move_home_and_end() {
        let mut s = TextInputState::with_value("abc");
        s.set_cursor(1);
        s.move_home();
        assert_eq!(s.cursor(), 0);
        s.move_end();
        assert_eq!(s.cursor(), 3);
    }

    #[test]
    fn set_cursor_clamps_to_char_count() {
        let mut s = TextInputState::with_value("ab");
        s.set_cursor(100);
        assert_eq!(s.cursor(), 2);
    }

    #[test]
    fn clear_resets_value_cursor_and_offset() {
        let mut s = TextInputState::with_value("abc");
        s.set_cursor(1);
        s.ensure_cursor_visible(2);
        s.clear();
        assert_eq!(s.value(), "");
        assert_eq!(s.cursor(), 0);
        assert_eq!(s.offset(), 0);
    }

    #[test]
    fn insert_handles_multi_byte_and_wide_characters_by_char_index_not_byte_index() {
        let mut s = TextInputState::new();
        s.insert('あ'); // 3 bytes, 2 columns wide
        s.insert('b');
        assert_eq!(s.value(), "あb");
        assert_eq!(s.cursor(), 2);
        s.set_cursor(1);
        s.insert('c');
        assert_eq!(s.value(), "あcb"); // spliced between the two chars, not by byte offset
        assert_eq!(s.char_count(), 3);
    }

    #[test]
    fn ensure_cursor_visible_scrolls_right_past_the_window() {
        let mut s = TextInputState::with_value("abcdefgh");
        s.set_cursor(5);
        s.ensure_cursor_visible(4); // window is 4 cols wide; cursor sits on a character
        assert!(s.offset() <= 5);
        assert!(s.column_between(s.offset(), 5) < 4);
        assert!(s.offset() > 0); // scrolled right from the default offset of 0
    }

    #[test]
    fn ensure_cursor_visible_scrolls_right_when_cursor_is_at_the_end() {
        let mut s = TextInputState::with_value("abcdefgh");
        s.ensure_cursor_visible(4); // cursor at char_count == 8; 1 col reserved for the caret
        assert!(s.column_between(s.offset(), s.cursor()) < 3); // 4 - 1 reserved column
    }

    #[test]
    fn ensure_cursor_visible_scrolls_left_to_reveal_an_earlier_cursor() {
        let mut s = TextInputState::with_value("abcdefgh");
        s.ensure_cursor_visible(4); // cursor at the end scrolls the window forward first
        assert!(s.offset() > 0);
        s.set_cursor(0);
        s.ensure_cursor_visible(4); // cursor moved before the window; scrolls back to 0
        assert_eq!(s.offset(), 0);
    }

    #[test]
    fn ensure_cursor_visible_is_a_no_op_when_already_visible() {
        let mut s = TextInputState::with_value("abcdefgh");
        s.set_cursor(2);
        s.ensure_cursor_visible(4);
        assert_eq!(s.offset(), 0);
    }

    #[test]
    fn ensure_cursor_visible_is_a_no_op_with_zero_width() {
        let mut s = TextInputState::with_value("abcdefgh");
        s.set_cursor(5);
        s.ensure_cursor_visible(0);
        assert_eq!(s.offset(), 0);
    }
}
