use retroglyph_core::text::{char_width, width_usize};
use retroglyph_core::{Event, KeyCode, KeyModifiers};

/// A `String` value, a byte cursor into it, and a horizontal scroll offset.
///
/// The state a single-line editable text field needs, mirroring [`ListState`](crate::ListState)'s
/// split between "what's selected/typed" and "how it's drawn".
///
/// Holds no reference to any widget: the same `TextInputState` can be reused across frames (and
/// across a resized field) the way `ListState` is reused across a resized list.
///
/// `cursor` is a byte index into `value`, not a char or display-column index, and every mutating
/// method here maintains the invariant that it always lands on a char boundary: `insert`/
/// `insert_str`/`backspace`/`delete` never split a multi-byte character, and `move_left`/
/// `move_right` step by whole `char`s. Display-column math (where the caret actually draws, and
/// how far the field has scrolled) is a separate concern handled by
/// [`ensure_visible`](Self::ensure_visible) and the [`TextInput`](crate::TextInput) widget itself, via
/// `retroglyph_core::text::width_usize`: a byte or char count is the wrong unit once the value
/// contains a double-width character.
///
/// Handles typed characters (`Event::Key(KeyCode::Char(c))`) and pasted text (`Event::Paste`)
/// only, not IME/text composition or multi-line editing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextInputState {
    value: String,
    cursor: usize,
    scroll: u16,
}

impl TextInputState {
    /// An empty field: empty value, cursor at `0`, scroll at `0`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            value: String::new(),
            cursor: 0,
            scroll: 0,
        }
    }

    /// The current text content.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Replace the entire content and move the cursor to its end. Resets the scroll offset to
    /// zero. Call [`ensure_visible`](Self::ensure_visible) afterward if the new value should
    /// scroll to keep the cursor (still at the end) in view.
    pub fn set_value(&mut self, s: impl Into<String>) {
        self.value = s.into();
        self.cursor = self.value.len();
        self.scroll = 0;
    }

    /// The cursor's byte offset into [`value`](Self::value). Always on a char boundary.
    #[must_use]
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    /// The current horizontal scroll offset, in display columns from the start of `value`. See
    /// [`ensure_visible`](Self::ensure_visible).
    #[must_use]
    pub const fn scroll(&self) -> u16 {
        self.scroll
    }

    /// Insert `c` at the cursor and move the cursor past it.
    pub fn insert(&mut self, c: char) {
        self.value.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    /// Insert `s` at the cursor and move the cursor past it, e.g. from an
    /// [`Event::Paste`](retroglyph_core::Event::Paste).
    pub fn insert_str(&mut self, s: &str) {
        self.value.insert_str(self.cursor, s);
        self.cursor += s.len();
    }

    /// Delete the character before the cursor, if any, and move the cursor onto its place.
    pub fn backspace(&mut self) {
        let Some(prev) = self.prev_boundary() else {
            return;
        };
        self.value.drain(prev..self.cursor);
        self.cursor = prev;
    }

    /// Delete the character at the cursor, if any. The cursor itself does not move.
    pub fn delete(&mut self) {
        let Some(next) = self.next_boundary() else {
            return;
        };
        self.value.drain(self.cursor..next);
    }

    /// Move the cursor one character left, if not already at the start.
    pub fn move_left(&mut self) {
        if let Some(prev) = self.prev_boundary() {
            self.cursor = prev;
        }
    }

    /// Move the cursor one character right, if not already at the end.
    pub fn move_right(&mut self) {
        if let Some(next) = self.next_boundary() {
            self.cursor = next;
        }
    }

    /// Move the cursor to the start of the value.
    pub const fn move_home(&mut self) {
        self.cursor = 0;
    }

    /// Move the cursor to the end of the value.
    pub const fn move_end(&mut self) {
        self.cursor = self.value.len();
    }

    /// Apply one input event: typed characters, Backspace/Delete, Left/Right/Home/End, and
    /// pasted text. Returns whether the event was consumed (so a caller can decide whether to
    /// fall through to its own key handling, e.g. Enter/Escape/Tab, none of which this consumes).
    ///
    /// Key releases and auto-repeats other than presses are ignored except that auto-repeat
    /// presses are treated the same as a press (matches [`FocusRing`](crate::FocusRing)/
    /// [`Shortcuts`](crate::Shortcuts)'s own `is_down` gating). [`Event`](retroglyph_core::Event)
    /// has no IME/composition variant to begin with; see the scope note above.
    pub fn handle_event(&mut self, event: &Event) -> bool {
        match event {
            Event::Key(key) if key.is_down() => match key.code {
                KeyCode::Char(c)
                    if (key.modifiers
                        & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER))
                        .is_empty() =>
                {
                    self.insert(c);
                    true
                }
                KeyCode::Backspace => {
                    self.backspace();
                    true
                }
                KeyCode::Delete => {
                    self.delete();
                    true
                }
                KeyCode::Left => {
                    self.move_left();
                    true
                }
                KeyCode::Right => {
                    self.move_right();
                    true
                }
                KeyCode::Home => {
                    self.move_home();
                    true
                }
                KeyCode::End => {
                    self.move_end();
                    true
                }
                _ => false,
            },
            Event::Paste(s) => {
                self.insert_str(s);
                true
            }
            _ => false,
        }
    }

    /// Nudge the scroll offset by the minimum amount needed to keep the cursor within a
    /// `width`-column window, the way [`ListState::ensure_visible`](crate::ListState::ensure_visible)
    /// keeps a selection in view.
    ///
    /// A no-op if `width` is zero or the cursor is already visible. Call this once per frame
    /// before rendering (with the actual, current field width, since that can change on resize)
    /// rather than only after an edit: it's cheap and idempotent.
    pub fn ensure_visible(&mut self, width: u16) {
        if width == 0 {
            return;
        }
        let caret_col = self.caret_column();
        if caret_col < self.scroll {
            self.scroll = self.snap_to_char_boundary(caret_col);
        } else if caret_col >= self.scroll.saturating_add(width) {
            let target = caret_col.saturating_add(1).saturating_sub(width);
            self.scroll = self.snap_to_char_boundary(target);
        }
    }

    /// The largest display column `<= col` at which some character in `value` starts, so that
    /// `retroglyph_core::text::split_at_width(value, that_column)` never has to refuse to split a
    /// wide character's own cell (see issue #712).
    ///
    /// Mirrors `split_at_width`'s own column-accumulation walk: a column is only ever a valid
    /// split point if it lines up with a char boundary, not the right half of a double-width
    /// glyph's footprint.
    #[must_use]
    fn snap_to_char_boundary(&self, col: u16) -> u16 {
        let mut cols = 0u16;
        for ch in self.value.chars() {
            let next = cols.saturating_add(char_width(ch));
            if next > col {
                break;
            }
            cols = next;
        }
        cols
    }

    /// The cursor's position in display columns from the start of `value`, saturating at
    /// `u16::MAX` the way [`width`](retroglyph_core::text::width) does.
    #[must_use]
    fn caret_column(&self) -> u16 {
        #[allow(clippy::cast_possible_truncation)] // saturated to u16::MAX below
        let col = width_usize(&self.value[..self.cursor]).min(usize::from(u16::MAX)) as u16;
        col
    }

    /// The byte index of the char boundary immediately before the cursor, or `None` at the
    /// start of the value.
    fn prev_boundary(&self) -> Option<usize> {
        self.value[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
    }

    /// The byte index of the char boundary immediately after the cursor, or `None` at the end
    /// of the value.
    fn next_boundary(&self) -> Option<usize> {
        let mut chars = self.value[self.cursor..].char_indices();
        chars.next()?;
        Some(
            chars
                .next()
                .map_or(self.value.len(), |(i, _)| self.cursor + i),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use retroglyph_core::{KeyEvent, KeyEventKind};

    #[test]
    fn insert_and_backspace_move_the_byte_cursor() {
        let mut state = TextInputState::new();
        state.insert('h');
        state.insert('i');
        assert_eq!(state.value(), "hi");
        assert_eq!(state.cursor(), 2);

        state.backspace();
        assert_eq!(state.value(), "h");
        assert_eq!(state.cursor(), 1);
    }

    #[test]
    fn backspace_and_delete_stay_on_char_boundaries_for_multibyte_chars() {
        let mut state = TextInputState::new();
        state.set_value("aあb");
        // "aあb": a=1 byte, あ=3 bytes, b=1 byte. Cursor starts at end (5).
        state.move_left(); // before 'b', cursor = 4
        state.backspace(); // removes 'あ' (3 bytes), not a partial byte of it
        assert_eq!(state.value(), "ab");
        assert_eq!(state.cursor(), 1);

        state.set_value("aあb");
        state.move_home();
        state.move_right(); // after 'a', cursor = 1
        state.delete(); // removes 'あ' whole
        assert_eq!(state.value(), "ab");
        assert_eq!(state.cursor(), 1);
    }

    #[test]
    fn move_left_right_home_end_traverse_whole_characters() {
        let mut state = TextInputState::new();
        state.set_value("hi");
        state.move_home();
        assert_eq!(state.cursor(), 0);
        state.move_right();
        assert_eq!(state.cursor(), 1);
        state.move_end();
        assert_eq!(state.cursor(), 2);
        state.move_left();
        assert_eq!(state.cursor(), 1);
    }

    #[test]
    fn insert_str_handles_paste() {
        let mut state = TextInputState::new();
        state.insert_str("hello");
        assert_eq!(state.value(), "hello");
        assert_eq!(state.cursor(), 5);
    }

    #[test]
    fn handle_event_consumes_typed_keys_and_paste() {
        let mut state = TextInputState::new();
        let key = |code| Event::Key(KeyEvent::new(code, KeyModifiers::NONE));

        assert!(state.handle_event(&key(KeyCode::Char('x'))));
        assert_eq!(state.value(), "x");

        assert!(state.handle_event(&Event::Paste("yz".into())));
        assert_eq!(state.value(), "xyz");

        assert!(state.handle_event(&key(KeyCode::Backspace)));
        assert_eq!(state.value(), "xy");

        // Not consumed: this widget has no opinion on Enter/Escape/Tab.
        assert!(!state.handle_event(&key(KeyCode::Enter)));
    }

    #[test]
    fn text_input_handle_event_does_not_type_ctrl_shortcut_characters() {
        let mut state = TextInputState::new();
        let ctrl_s = Event::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        let consumed = state.handle_event(&ctrl_s);

        assert_eq!(state.value(), "");
        assert!(!consumed);
    }

    #[test]
    fn handle_event_still_types_shifted_characters() {
        let mut state = TextInputState::new();
        let shift_s = Event::Key(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT));
        let consumed = state.handle_event(&shift_s);

        assert_eq!(state.value(), "S");
        assert!(consumed);
    }

    #[test]
    fn handle_event_ignores_key_releases() {
        let mut state = TextInputState::new();
        let release = Event::Key(KeyEvent::with_kind(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));
        assert!(!state.handle_event(&release));
        assert_eq!(state.value(), "");
    }

    #[test]
    fn ensure_visible_scrolls_to_keep_the_caret_in_view() {
        let mut state = TextInputState::new();
        state.set_value("hello world");
        state.ensure_visible(5);
        // Cursor is at the end (column 11); a 5-wide window scrolls to show it.
        assert_eq!(state.scroll(), 11 + 1 - 5);

        state.move_home();
        state.ensure_visible(5);
        assert_eq!(state.scroll(), 0);
    }

    #[test]
    fn ensure_visible_accounts_for_wide_characters() {
        let mut state = TextInputState::new();
        state.set_value("aああ"); // columns: a=1, あ=2, あ=2 -> caret at end is column 5
        state.ensure_visible(3);
        assert_eq!(state.scroll(), 5 + 1 - 3);
    }

    #[test]
    fn ensure_visible_does_not_overflow_when_the_caret_column_saturates() {
        // retroglyph#729: `caret_col + 1 - width` used to overflow on the add once a long enough
        // paste saturated `caret_column()` at `u16::MAX`.
        let mut state = TextInputState::new();
        state.insert_str(&"a".repeat(70_000));
        state.ensure_visible(5);
        assert_eq!(state.scroll(), u16::MAX - 5);
    }

    #[test]
    fn ensure_visible_is_a_noop_for_zero_width() {
        let mut state = TextInputState::new();
        state.set_value("hello");
        state.ensure_visible(0);
        assert_eq!(state.scroll(), 0);
    }

    #[test]
    fn text_input_ensure_visible_scroll_does_not_split_a_wide_character() {
        // retroglyph#712: naive column arithmetic (caret_col + 1 - width) landed scroll == 1,
        // inside "あ"'s own two-column cell, which `split_at_width` refuses to split. Snapping
        // down to the nearest char boundary keeps scroll at 0, the start of the first "あ".
        let mut state = TextInputState::new();
        state.set_value("ああ"); // two 2-column characters, caret at end is column 4
        state.ensure_visible(4);
        assert_eq!(state.scroll(), 0);
    }
}
