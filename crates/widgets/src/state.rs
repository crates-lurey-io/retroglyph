//! Reusable widget state, kept separate from drawing.
//!
//! Widgets in this crate are free functions with no retained state (see the
//! crate docs). But *something* still has to remember which item is
//! selected and how far a list has scrolled between frames -- that's app
//! state, not widget state, and [`ListState`] is a small, tested, headless
//! (no [`Backend`](retroglyph_core::Backend) dependency) building block for
//! it so every consumer doesn't hand-roll its own wraparound-cursor math.

/// Selection index and scroll offset for a selectable, scrollable list.
///
/// Holds no reference to the list's actual items: `len` is passed in to each
/// mutating method, so the same `ListState` can be reused across lists that
/// change size (menus, reward pools, deck views, ...) without going stale.
///
/// Selection movement wraps around `len` (pressing "next" past the last item
/// lands on the first, and vice versa), matching the cursor behavior common
/// to menu-driven TUIs. Scrolling is a separate, unbounded-above counter
/// (clamped only at zero) since only the caller knows the content length and
/// viewport height needed to clamp it from above.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ListState {
    selected: Option<usize>,
    offset: usize,
}

impl ListState {
    /// An empty state: nothing selected, no scroll.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            selected: None,
            offset: 0,
        }
    }

    /// The currently selected index, if any.
    #[must_use]
    pub const fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// The current scroll offset (index of the first visible item/line).
    #[must_use]
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// Select an explicit index (or clear the selection with `None`).
    pub const fn select(&mut self, index: Option<usize>) {
        self.selected = index;
    }

    /// Set the scroll offset directly.
    pub const fn set_offset(&mut self, offset: usize) {
        self.offset = offset;
    }

    /// Clear both the selection and the scroll offset, e.g. after the
    /// underlying list has been replaced with different content.
    pub const fn reset(&mut self) {
        self.selected = None;
        self.offset = 0;
    }

    /// Move the scroll offset by `delta`, clamped at zero. There is no upper
    /// clamp here: only the caller knows the content length and viewport
    /// height needed to bound it from above.
    pub fn scroll_by(&mut self, delta: i32) {
        let next = i64::from(delta).saturating_add(i64::try_from(self.offset).unwrap_or(i64::MAX));
        self.offset = next.max(0).try_into().unwrap_or(usize::MAX);
    }

    /// Select the next item, wrapping past the end back to the first.
    /// Selects index 0 if nothing was selected yet. No-op (clears the
    /// selection) if `len` is zero.
    pub fn select_next(&mut self, len: usize) {
        self.selected = Self::wrapped(self.selected, 1, len);
    }

    /// Select the previous item, wrapping past the start back to the last.
    /// Selects the last item if nothing was selected yet. No-op (clears the
    /// selection) if `len` is zero.
    pub fn select_previous(&mut self, len: usize) {
        self.selected = Self::wrapped(self.selected, -1, len);
    }

    /// Select the first item, or clear the selection if `len` is zero.
    pub fn select_first(&mut self, len: usize) {
        self.selected = (len > 0).then_some(0);
    }

    /// Select the last item, or clear the selection if `len` is zero.
    pub fn select_last(&mut self, len: usize) {
        self.selected = (len > 0).then(|| len - 1);
    }

    /// Shared wraparound math for `select_next`/`select_previous`. `delta`
    /// is `1` or `-1`; a missing selection starts from the end opposite the
    /// direction of travel so the first press lands somewhere sensible.
    fn wrapped(current: Option<usize>, delta: i32, len: usize) -> Option<usize> {
        if len == 0 {
            return None;
        }
        let Ok(len) = i32::try_from(len) else {
            return current; // absurdly large len; leave selection alone
        };
        let base = current.map_or(if delta > 0 { -1 } else { 0 }, |i| {
            i32::try_from(i).unwrap_or(0)
        });
        Some(usize::try_from((base + delta).rem_euclid(len)).unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_empty() {
        let s = ListState::new();
        assert_eq!(s.selected(), None);
        assert_eq!(s.offset(), 0);
    }

    #[test]
    fn next_from_none_selects_first() {
        let mut s = ListState::new();
        s.select_next(3);
        assert_eq!(s.selected(), Some(0));
    }

    #[test]
    fn previous_from_none_selects_last() {
        let mut s = ListState::new();
        s.select_previous(3);
        assert_eq!(s.selected(), Some(2));
    }

    #[test]
    fn next_wraps_past_the_end() {
        let mut s = ListState::new();
        s.select(Some(2));
        s.select_next(3);
        assert_eq!(s.selected(), Some(0));
    }

    #[test]
    fn previous_wraps_past_the_start() {
        let mut s = ListState::new();
        s.select(Some(0));
        s.select_previous(3);
        assert_eq!(s.selected(), Some(2));
    }

    #[test]
    fn zero_length_clears_selection() {
        let mut s = ListState::new();
        s.select(Some(0));
        s.select_next(0);
        assert_eq!(s.selected(), None);
        s.select(Some(0));
        s.select_previous(0);
        assert_eq!(s.selected(), None);
    }

    #[test]
    fn select_first_and_last() {
        let mut s = ListState::new();
        s.select_last(5);
        assert_eq!(s.selected(), Some(4));
        s.select_first(5);
        assert_eq!(s.selected(), Some(0));
        s.select_first(0);
        assert_eq!(s.selected(), None);
    }

    #[test]
    fn reset_clears_selection_and_offset() {
        let mut s = ListState::new();
        s.select(Some(2));
        s.set_offset(5);
        s.reset();
        assert_eq!(s.selected(), None);
        assert_eq!(s.offset(), 0);
    }

    #[test]
    fn scroll_by_clamps_at_zero() {
        let mut s = ListState::new();
        s.scroll_by(-5);
        assert_eq!(s.offset(), 0);
        s.scroll_by(3);
        assert_eq!(s.offset(), 3);
        s.scroll_by(-1);
        assert_eq!(s.offset(), 2);
    }
}
