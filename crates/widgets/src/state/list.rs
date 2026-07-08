/// How [`ListState::select_next`]/[`select_previous`](ListState::select_previous) behave when the
/// selection is already at the first/last item.
///
/// Defaults to [`Clamp`](Self::Clamp), matching ratatui's `ListState` (`select_next`/
/// `select_previous` `saturating_add`/clamp at the ends; wraparound is left to the caller, e.g.
/// via `(selected + 1) % len`). Older `tui-rs`-style wraparound is available via [`Wrap`](Self::Wrap)
/// for callers that want circular menu navigation instead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionWrap {
    /// Stop at the first/last item: `select_next` past the last item stays on the last item, and
    /// `select_previous` before the first item stays on the first.
    #[default]
    Clamp,
    /// Wrap around: `select_next` past the last item lands on the first, and `select_previous`
    /// before the first item lands on the last.
    Wrap,
}

/// Selection index and scroll offset for a selectable, scrollable list.
///
/// Holds no reference to the list's actual items: `len` is passed in to each
/// mutating method, so the same `ListState` can be reused across lists that
/// change size (menus, reward pools, deck views, ...) without going stale.
///
/// Selection movement clamps at `len`'s ends by default; see [`SelectionWrap`] (set via
/// [`ListState::set_wrap`]) to switch to wraparound instead. Scrolling is a separate,
/// unbounded-above counter (clamped only at zero) since only the caller knows the content length
/// and viewport height needed to clamp it from above.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ListState {
    selected: Option<usize>,
    offset: usize,
    wrap: SelectionWrap,
}

impl ListState {
    /// An empty state: nothing selected, no scroll.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            selected: None,
            offset: 0,
            wrap: SelectionWrap::Clamp,
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

    /// How `select_next`/`select_previous` behave at the ends of the list. Defaults to
    /// [`SelectionWrap::Clamp`].
    #[must_use]
    pub const fn wrap(&self) -> SelectionWrap {
        self.wrap
    }

    /// Sets how `select_next`/`select_previous` behave at the ends of the list.
    pub const fn set_wrap(&mut self, wrap: SelectionWrap) {
        self.wrap = wrap;
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

    /// Nudge the scroll offset by the minimum amount needed to bring
    /// `selected` into the `visible_height`-row window starting at `offset`.
    ///
    /// A no-op if nothing is selected, `visible_height` is zero, or the
    /// selection is already visible. Call this once per frame before
    /// rendering (with the actual, current viewport height, since that can
    /// change on terminal resize) rather than only after moving the
    /// selection -- it's cheap and idempotent, so redoing it every frame
    /// costs nothing and needs no special-casing for resize.
    pub const fn ensure_visible(&mut self, visible_height: usize) {
        let Some(selected) = self.selected else {
            return;
        };
        if visible_height == 0 {
            return;
        }
        if selected < self.offset {
            self.offset = selected;
        } else if selected >= self.offset + visible_height {
            self.offset = selected + 1 - visible_height;
        }
    }

    /// Move the scroll offset by `delta`, clamped at zero. There is no upper
    /// clamp here: only the caller knows the content length and viewport
    /// height needed to bound it from above.
    pub fn scroll_by(&mut self, delta: i32) {
        let next = i64::from(delta).saturating_add(i64::try_from(self.offset).unwrap_or(i64::MAX));
        self.offset = next.max(0).try_into().unwrap_or(usize::MAX);
    }

    /// Select the next item. Past the last item, clamps (stays on the last item) or wraps to the
    /// first, per [`wrap()`](Self::wrap). Selects index 0 if nothing was selected yet. No-op
    /// (clears the selection) if `len` is zero.
    pub fn select_next(&mut self, len: usize) {
        self.selected = Self::stepped(self.selected, 1, len, self.wrap);
    }

    /// Select the previous item. Before the first item, clamps (stays on the first item) or
    /// wraps to the last, per [`wrap()`](Self::wrap). Selects the last item if nothing was
    /// selected yet. No-op (clears the selection) if `len` is zero.
    pub fn select_previous(&mut self, len: usize) {
        self.selected = Self::stepped(self.selected, -1, len, self.wrap);
    }

    /// Select the first item, or clear the selection if `len` is zero.
    pub fn select_first(&mut self, len: usize) {
        self.selected = (len > 0).then_some(0);
    }

    /// Select the last item, or clear the selection if `len` is zero.
    pub fn select_last(&mut self, len: usize) {
        self.selected = (len > 0).then(|| len - 1);
    }

    /// Shared step math for `select_next`/`select_previous`. `delta` is `1` or `-1`; a missing
    /// selection picks the end opposite the direction of travel (so the first press lands
    /// somewhere sensible) independent of `mode`, since there's no prior index to clamp or wrap
    /// from yet.
    fn stepped(
        current: Option<usize>,
        delta: i32,
        len: usize,
        mode: SelectionWrap,
    ) -> Option<usize> {
        if len == 0 {
            return None;
        }
        let Some(i) = current else {
            return Some(if delta > 0 { 0 } else { len - 1 });
        };
        let Ok(len) = i32::try_from(len) else {
            return current; // absurdly large len; leave selection alone
        };
        let next = i32::try_from(i).unwrap_or(0) + delta;
        let idx = match mode {
            SelectionWrap::Wrap => next.rem_euclid(len),
            SelectionWrap::Clamp => next.clamp(0, len - 1),
        };
        usize::try_from(idx).ok()
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
    fn next_clamps_at_the_end_by_default() {
        let mut s = ListState::new();
        assert_eq!(s.wrap(), SelectionWrap::Clamp);
        s.select(Some(2));
        s.select_next(3);
        assert_eq!(s.selected(), Some(2)); // stays on the last item, does not wrap to 0
    }

    #[test]
    fn previous_clamps_at_the_start_by_default() {
        let mut s = ListState::new();
        s.select(Some(0));
        s.select_previous(3);
        assert_eq!(s.selected(), Some(0)); // stays on the first item, does not wrap to 2
    }

    #[test]
    fn next_wraps_past_the_end_when_wrap_is_set() {
        let mut s = ListState::new();
        s.set_wrap(SelectionWrap::Wrap);
        s.select(Some(2));
        s.select_next(3);
        assert_eq!(s.selected(), Some(0));
    }

    #[test]
    fn previous_wraps_past_the_start_when_wrap_is_set() {
        let mut s = ListState::new();
        s.set_wrap(SelectionWrap::Wrap);
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
    fn ensure_visible_is_a_no_op_when_already_in_view() {
        let mut s = ListState::new();
        s.select(Some(3));
        s.set_offset(2);
        s.ensure_visible(5); // window is [2, 7); 3 is inside it
        assert_eq!(s.offset(), 2);
    }

    #[test]
    fn ensure_visible_scrolls_down_to_reveal_a_later_selection() {
        let mut s = ListState::new();
        s.select(Some(10));
        s.set_offset(0);
        s.ensure_visible(4); // window is [0, 4); 10 is below it
        assert_eq!(s.offset(), 7); // [7, 11) puts 10 as the last visible row
        assert!(s.offset() <= 10 && 10 < s.offset() + 4);
    }

    #[test]
    fn ensure_visible_scrolls_up_to_reveal_an_earlier_selection() {
        let mut s = ListState::new();
        s.select(Some(1));
        s.set_offset(5);
        s.ensure_visible(3); // window is [5, 8); 1 is above it
        assert_eq!(s.offset(), 1);
    }

    #[test]
    fn ensure_visible_is_a_no_op_with_nothing_selected_or_zero_height() {
        let mut s = ListState::new();
        s.set_offset(5);
        s.ensure_visible(10); // nothing selected
        assert_eq!(s.offset(), 5);

        s.select(Some(20));
        s.ensure_visible(0); // zero-height viewport
        assert_eq!(s.offset(), 5);
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
