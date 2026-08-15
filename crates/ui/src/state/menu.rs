use retroglyph_core::event::{Event, KeyCode};

/// Selection index and scroll offset for an anchored popup menu. The menu is open iff a
/// selection exists: there is no independent open flag to fall out of sync with it.
///
/// [`ListState`](crate::state::ListState)'s sibling for content where some rows (separators,
/// headings) can never be selected: a dropdown, a combobox's list, a command palette.
///
/// Holds no reference to the menu's actual rows, the same design [`ListState`](crate::state::ListState) uses: every
/// method that needs to know which rows exist takes `len` (the row count) and a `selectable`
/// predicate (`impl Fn(usize) -> bool`) rather than a concrete row type, so this stays reusable
/// across whatever row enum a call site defines (see
/// [`Menu`](crate::widget::Menu)/[`MenuRow`](crate::widget::MenuRow) for this crate's own).
/// `selectable` must report the same answer every time it's called this frame: passing a
/// predicate that disagrees with what was drawn (e.g. after an `enabled` flag changed
/// mid-frame) can select or skip past a row inconsistently with the render that follows.
///
/// Selection movement ([`select_next`](Self::select_next)/[`select_previous`](Self::select_previous))
/// clamps at the ends: stepping past the last selectable row stays on it rather than wrapping,
/// matching [`ListState`](crate::state::ListState)'s default. [`open`](Self::open) selects the first selectable row,
/// skipping any leading separators/labels; a menu with no selectable row at all never opens
/// (`open` becomes a no-op), so a caller can never end up with an open menu and no selection to
/// navigate.
///
/// # Examples
///
/// ```
/// use retroglyph_ui::state::MenuState;
///
/// // rows: [Separator, "Cut" (disabled), "Copy", "Paste"]
/// let selectable = |i: usize| matches!(i, 2 | 3);
///
/// let mut state = MenuState::new();
/// state.open(4, selectable);
/// assert_eq!(state.selected(), Some(2)); // skipped the separator and the disabled item
///
/// state.select_next(4, selectable);
/// assert_eq!(state.selected(), Some(3));
/// state.select_next(4, selectable); // clamps: nothing selectable past index 3
/// assert_eq!(state.selected(), Some(3));
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MenuState {
    selected: Option<usize>,
    offset: usize,
}

impl MenuState {
    /// A closed menu: nothing selected, no scroll.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            selected: None,
            offset: 0,
        }
    }

    /// Whether the menu is currently open.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.selected.is_some()
    }

    /// The currently selected row index, if any. Always `None` while [`is_open`](Self::is_open)
    /// is `false`: [`close`](Self::close) clears it.
    #[must_use]
    pub const fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// The current scroll offset (index of the first row a [`Menu`](crate::widget::Menu) draws,
    /// not counting its own overflow indicator row).
    #[must_use]
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// Set the scroll offset directly.
    pub const fn set_offset(&mut self, offset: usize) {
        self.offset = offset;
    }

    /// Select an explicit row index directly, e.g. after resolving a click to a row, or close
    /// the menu by passing `None` (equivalent to [`close`](Self::close)). Stored verbatim, with
    /// no `selectable` check of its own: callers that need "never select a disabled row" should
    /// check that themselves first, the same responsibility
    /// [`ListState::select`](crate::state::ListState::select) leaves to its own callers.
    pub const fn select(&mut self, index: Option<usize>) {
        match index {
            Some(_) => self.selected = index,
            None => self.close(),
        }
    }

    /// Close the menu, clearing the selection and scroll offset.
    pub const fn close(&mut self) {
        self.selected = None;
        self.offset = 0;
    }

    /// Open the menu and select the first selectable row among `0..len`, skipping any leading
    /// separators/labels. A no-op (stays closed) if no row in `0..len` is selectable: an empty
    /// menu never opens.
    pub fn open(&mut self, len: usize, selectable: impl Fn(usize) -> bool) {
        if let Some(first) = first_selectable(len, &selectable) {
            self.selected = Some(first);
            self.offset = 0;
        }
    }

    /// [`close`](Self::close) if open, otherwise [`open`](Self::open).
    pub fn toggle(&mut self, len: usize, selectable: impl Fn(usize) -> bool) {
        if self.is_open() {
            self.close();
        } else {
            self.open(len, selectable);
        }
    }

    /// Move the selection to the next selectable row among `0..len`, skipping separators/labels.
    /// Clamps at the last selectable row rather than wrapping. A no-op while the menu is closed:
    /// navigation never opens it.
    pub fn select_next(&mut self, len: usize, selectable: impl Fn(usize) -> bool) {
        if self.selected.is_none() {
            return;
        }
        self.selected = step(self.selected, 1, len, &selectable);
    }

    /// Move the selection to the previous selectable row among `0..len`, skipping
    /// separators/labels. Clamps at the first selectable row rather than wrapping. A no-op while
    /// the menu is closed: navigation never opens it.
    pub fn select_previous(&mut self, len: usize, selectable: impl Fn(usize) -> bool) {
        if self.selected.is_none() {
            return;
        }
        self.selected = step(self.selected, -1, len, &selectable);
    }

    /// Nudge the scroll offset by the minimum amount needed to bring `selected` into the
    /// `visible_height`-row window starting at `offset`. Same math as
    /// [`ListState::ensure_visible`](crate::state::ListState::ensure_visible); see its docs for why this
    /// is a no-op unless a selection exists and is out of view, and why it's cheap to call every
    /// frame before rendering.
    pub const fn ensure_visible(&mut self, visible_height: usize) {
        let Some(selected) = self.selected else {
            return;
        };
        if visible_height == 0 {
            return;
        }
        if selected < self.offset {
            self.offset = selected;
        } else if selected >= self.offset.saturating_add(visible_height) {
            self.offset = selected.saturating_add(1).saturating_sub(visible_height);
        }
    }

    /// Move the scroll offset by `delta`, clamped at zero: the wheel-scroll half of "independent
    /// wheel-scroll versus keyboard-nav scroll" -- this never touches `selected`, the same split
    /// [`List`](crate::widget::List)/[`ListState::scroll_by`](crate::state::ListState::scroll_by) already
    /// have between a click (which selects) and the wheel (which only scrolls).
    pub fn scroll_by(&mut self, delta: i32) {
        let next = i64::from(delta).saturating_add(i64::try_from(self.offset).unwrap_or(i64::MAX));
        self.offset = next.max(0).try_into().unwrap_or(usize::MAX);
    }

    /// Apply one keyboard event while the menu is open: Up/Down move the selection (via
    /// [`select_previous`](Self::select_previous)/[`select_next`](Self::select_next)), Enter
    /// activates the current selection and [`close`](Self::close)s the menu, and Escape
    /// [`close`](Self::close)s it without activating anything. Every other event, and every
    /// event while [`is_open`](Self::is_open) is `false`, is ignored.
    ///
    /// Returns `Some(index)` for the one frame Enter activates a selectable row, `None`
    /// otherwise -- including on Escape or an outside close, which intentionally report no
    /// activation ("closes without firing").
    ///
    /// Only [`KeyEventKind::Press`](retroglyph_core::event::KeyEventKind::Press)/
    /// [`Repeat`](retroglyph_core::event::KeyEventKind::Repeat) drive this (via
    /// [`KeyEvent::is_down`](retroglyph_core::event::KeyEvent::is_down)); a release is ignored, the
    /// same convention [`TextInputState::handle_event`](crate::state::TextInputState::handle_event) uses.
    pub fn handle_event(
        &mut self,
        event: &Event,
        len: usize,
        selectable: impl Fn(usize) -> bool,
    ) -> Option<usize> {
        if !self.is_open() {
            return None;
        }
        let Event::Key(key) = event else {
            return None;
        };
        if !key.is_down() {
            return None;
        }
        match key.code {
            KeyCode::Up => {
                self.select_previous(len, selectable);
                None
            }
            KeyCode::Down => {
                self.select_next(len, selectable);
                None
            }
            KeyCode::Enter => {
                let index = self.selected.filter(|&i| selectable(i))?;
                self.close();
                Some(index)
            }
            KeyCode::Escape => {
                self.close();
                None
            }
            _ => None,
        }
    }
}

/// The first index in `0..len` for which `selectable` reports `true`, if any.
fn first_selectable(len: usize, selectable: &impl Fn(usize) -> bool) -> Option<usize> {
    (0..len).find(|&i| selectable(i))
}

/// The last index in `0..len` for which `selectable` reports `true`, if any.
fn last_selectable(len: usize, selectable: &impl Fn(usize) -> bool) -> Option<usize> {
    (0..len).rev().find(|&i| selectable(i))
}

/// Shared step math for `select_next`/`select_previous`. `delta` is `1` or `-1`. A missing
/// selection picks [`first_selectable`]/[`last_selectable`] depending on direction, the same
/// "land somewhere sensible" rule [`ListState`](crate::state::ListState)'s own `stepped` uses. Clamps
/// (returns `current` unchanged) once stepping any further would run off the end without
/// finding another selectable row.
fn step(
    current: Option<usize>,
    delta: i32,
    len: usize,
    selectable: &impl Fn(usize) -> bool,
) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let Some(start) = current else {
        return if delta > 0 {
            first_selectable(len, selectable)
        } else {
            last_selectable(len, selectable)
        };
    };
    let Ok(len) = i32::try_from(len) else {
        return current; // absurdly large len; leave selection alone
    };
    let mut i = i32::try_from(start).unwrap_or(0);
    loop {
        i += delta;
        if i < 0 || i >= len {
            return current; // clamp: no further selectable row in this direction
        }
        // `0 <= i < len`, and `len` came from a `usize` above, so this is in range.
        #[allow(clippy::cast_sign_loss)]
        let idx = i as usize;
        if selectable(idx) {
            return Some(idx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selectable_at(indices: &'static [usize]) -> impl Fn(usize) -> bool {
        move |i| indices.contains(&i)
    }

    #[test]
    fn starts_closed_with_nothing_selected() {
        let s = MenuState::new();
        assert!(!s.is_open());
        assert_eq!(s.selected(), None);
        assert_eq!(s.offset(), 0);
    }

    #[test]
    fn open_skips_a_leading_separator() {
        let mut s = MenuState::new();
        // row 0 is a separator (not selectable); row 1 is the first real item.
        s.open(3, selectable_at(&[1, 2]));
        assert!(s.is_open());
        assert_eq!(s.selected(), Some(1));
    }

    #[test]
    fn an_empty_menu_never_opens() {
        let mut s = MenuState::new();
        s.open(3, selectable_at(&[])); // every row is a separator/label
        assert!(!s.is_open());
        assert_eq!(s.selected(), None);
    }

    #[test]
    fn toggle_opens_then_closes() {
        let mut s = MenuState::new();
        s.toggle(2, selectable_at(&[0, 1]));
        assert!(s.is_open());
        s.toggle(2, selectable_at(&[0, 1]));
        assert!(!s.is_open());
        assert_eq!(s.selected(), None);
    }

    #[test]
    fn select_next_and_previous_skip_separators() {
        let mut s = MenuState::new();
        // rows: [Item(0), Separator(1), Item(2)]
        let selectable = selectable_at(&[0, 2]);
        s.open(3, &selectable);
        assert_eq!(s.selected(), Some(0));
        s.select_next(3, &selectable);
        assert_eq!(s.selected(), Some(2)); // skipped the separator
        s.select_next(3, &selectable);
        assert_eq!(s.selected(), Some(2)); // clamps at the last selectable row
        s.select_previous(3, &selectable);
        assert_eq!(s.selected(), Some(0));
        s.select_previous(3, &selectable);
        assert_eq!(s.selected(), Some(0)); // clamps at the first selectable row
    }

    #[test]
    fn label_rows_are_never_selected_by_navigation() {
        let mut s = MenuState::new();
        // rows: [Label(0), Item(1), Label(2), Item(3)]
        let selectable = selectable_at(&[1, 3]);
        s.open(4, &selectable);
        assert_eq!(s.selected(), Some(1));
        s.select_next(4, &selectable);
        assert_eq!(s.selected(), Some(3));
    }

    #[test]
    fn close_clears_selection_and_offset() {
        let mut s = MenuState::new();
        s.open(2, selectable_at(&[0, 1]));
        s.set_offset(5);
        s.close();
        assert!(!s.is_open());
        assert_eq!(s.selected(), None);
        assert_eq!(s.offset(), 0);
    }

    #[test]
    fn ensure_visible_scrolls_down_to_reveal_a_later_selection() {
        let mut s = MenuState::new();
        s.open(20, selectable_at(&[10]));
        s.ensure_visible(4);
        assert_eq!(s.offset(), 7); // [7, 11) puts 10 as the last visible row
    }

    #[test]
    fn scroll_by_is_independent_of_the_selection() {
        let mut s = MenuState::new();
        s.open(5, selectable_at(&[0]));
        s.scroll_by(2);
        assert_eq!(s.offset(), 2);
        assert_eq!(s.selected(), Some(0)); // wheel scroll never touches the selection
        s.scroll_by(-5);
        assert_eq!(s.offset(), 0); // clamped at zero
    }

    #[test]
    fn escape_closes_without_activating() {
        use retroglyph_core::event::{KeyEvent, KeyModifiers};

        let mut s = MenuState::new();
        s.open(2, selectable_at(&[0, 1]));
        let event = Event::Key(KeyEvent::new(KeyCode::Escape, KeyModifiers::NONE));
        let activated = s.handle_event(&event, 2, selectable_at(&[0, 1]));
        assert_eq!(activated, None);
        assert!(!s.is_open());
    }

    #[test]
    fn enter_activates_the_selection_and_closes() {
        use retroglyph_core::event::{KeyEvent, KeyModifiers};

        let mut s = MenuState::new();
        s.open(2, selectable_at(&[0, 1]));
        s.select_next(2, selectable_at(&[0, 1]));
        let event = Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let activated = s.handle_event(&event, 2, selectable_at(&[0, 1]));
        assert_eq!(activated, Some(1));
        assert!(!s.is_open()); // checkbox/plain items alike fire and close
    }

    #[test]
    fn arrow_keys_navigate_while_open() {
        use retroglyph_core::event::{KeyEvent, KeyModifiers};

        let mut s = MenuState::new();
        s.open(3, selectable_at(&[0, 1, 2]));
        let down = Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        s.handle_event(&down, 3, selectable_at(&[0, 1, 2]));
        assert_eq!(s.selected(), Some(1));
    }

    #[test]
    fn events_are_ignored_while_closed() {
        use retroglyph_core::event::{KeyEvent, KeyModifiers};

        let mut s = MenuState::new();
        let down = Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(s.handle_event(&down, 3, selectable_at(&[0, 1, 2])), None);
        assert_eq!(s.selected(), None);
    }

    #[test]
    fn select_next_on_a_closed_menu_is_a_no_op() {
        let mut s = MenuState::new();
        s.select_next(3, selectable_at(&[0, 1, 2]));
        assert!(!s.is_open());
        assert_eq!(s.selected(), None);
    }

    #[test]
    fn select_previous_on_a_closed_menu_is_a_no_op() {
        let mut s = MenuState::new();
        s.select_previous(3, selectable_at(&[0, 1, 2]));
        assert!(!s.is_open());
        assert_eq!(s.selected(), None);
    }

    #[test]
    fn select_while_closed_does_not_report_a_selection() {
        // A selection can no longer exist independently of openness: `select(Some(_))` on a
        // closed menu opens it, rather than leaving behind the old invalid "selected but
        // closed" state.
        let mut s = MenuState::new();
        assert!(!s.is_open());
        s.select(Some(2));
        assert!(s.is_open());
        assert_eq!(s.selected(), Some(2));
    }

    #[test]
    fn select_none_closes() {
        let mut s = MenuState::new();
        s.open(2, selectable_at(&[0, 1]));
        s.set_offset(5);
        s.select(None);
        assert!(!s.is_open());
        assert_eq!(s.selected(), None);
        assert_eq!(s.offset(), 0);
    }
}
