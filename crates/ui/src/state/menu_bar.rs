use crate::state::MenuState;

/// Which label's popup is open (if any) for a [`MenuBar`](crate::widget::MenuBar), plus that
/// popup's own [`MenuState`].
///
/// A menu bar has at most one open popup at a time, so this wraps a single [`MenuState`] rather
/// than one per label: switching which label is open always goes through [`open`](Self::open),
/// which closes whatever was open first. Holds no reference to the bar's own titles/rows, the
/// same design [`MenuState`] itself uses: every method that needs to know a label's row count
/// takes `len` and a `selectable` predicate rather than a concrete row type, so this stays
/// reusable across whatever row type a call site's [`MenuBar`](crate::widget::MenuBar) uses.
///
/// [`open_label`](Self::open_label) is gated on [`MenuState::is_open`] rather than tracked as an
/// independent flag: [`MenuState::handle_event`] (Escape) and
/// [`Menu::show`](crate::widget::Menu::show) (an outside click, or a row activation) already
/// close the inner [`MenuState`] on the caller's behalf, and gating the getter this way means
/// those paths automatically clear which label reads as open too, with nothing to keep in sync
/// by hand.
///
/// # Examples
///
/// ```
/// use retroglyph_ui::state::MenuBarState;
///
/// // Two labels, each with one selectable row.
/// let selectable = |_: usize| true;
///
/// let mut state = MenuBarState::new();
/// state.open(0, 1, selectable);
/// assert_eq!(state.open_label(), Some(0));
///
/// // Hovering another label while one is open switches to it (see `MenuBar::show`); modeled
/// // here directly as another `open` call.
/// state.open(1, 1, selectable);
/// assert_eq!(state.open_label(), Some(1));
///
/// state.close();
/// assert_eq!(state.open_label(), None);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MenuBarState {
    open: usize,
    menu: MenuState,
    /// The label [`MenuBar::show`](crate::widget::MenuBar::show) resolved the pointer over as of
    /// the *last* frame it ran, so it can tell a genuinely new hover (this frame's resolved
    /// label differs from this) from a pointer that's simply still sitting over the same label
    /// it was last frame. Without this, re-checking "is the pointer over a different label than
    /// the one that's open" fresh every frame would immediately undo a keyboard-driven
    /// [`MenuBar::handle_event`](crate::widget::MenuBar::handle_event) switch the instant the
    /// pointer happens to still be resting over whatever label was open *before* that keypress,
    /// since nothing else would tell the difference between that and a pointer that had just
    /// freshly moved there.
    hover: Option<usize>,
}

impl MenuBarState {
    /// A closed menu bar: no label open.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            open: 0,
            menu: MenuState::new(),
            hover: None,
        }
    }

    /// The label whose popup is currently open, if any. See the type docs for why this is
    /// derived from [`MenuState::is_open`] rather than stored independently.
    #[must_use]
    pub const fn open_label(&self) -> Option<usize> {
        if self.menu.is_open() {
            Some(self.open)
        } else {
            None
        }
    }

    /// The open popup's own state, for driving it through
    /// [`Menu::show`](crate::widget::Menu::show).
    #[must_use]
    pub const fn menu(&self) -> &MenuState {
        &self.menu
    }

    /// Mutable access to the open popup's own state, for driving it through
    /// [`Menu::show`](crate::widget::Menu::show).
    pub const fn menu_mut(&mut self) -> &mut MenuState {
        &mut self.menu
    }

    /// Open `label`'s popup, selecting its first selectable row among `0..len` (skipping any
    /// leading separators/labels), closing whatever label's popup was open first. A no-op
    /// (closes without opening the new label) if `label` has no selectable row at all, mirroring
    /// [`MenuState::open`].
    pub fn open(&mut self, label: usize, len: usize, selectable: impl Fn(usize) -> bool) {
        self.menu.close();
        self.menu.open(len, selectable);
        self.open = label;
    }

    /// Close whichever label's popup is open. A no-op if none is.
    pub const fn close(&mut self) {
        self.menu.close();
    }

    /// [`close`](Self::close) if `label`'s popup is already open, otherwise
    /// [`open`](Self::open) it.
    pub fn toggle(&mut self, label: usize, len: usize, selectable: impl Fn(usize) -> bool) {
        if self.open_label() == Some(label) {
            self.close();
        } else {
            self.open(label, len, selectable);
        }
    }

    /// Switch to `label`'s popup, but only on the frame `label` first differs from what
    /// [`hover`](Self::hover) resolved to last time this was called -- a no-op both while a
    /// pointer sits still over the same label across frames, and while no popup is open at all
    /// (a cold hover; see [`MenuBar`](crate::widget::MenuBar)'s docs). See the `hover` field's
    /// own doc comment for why this edge-triggering, rather than a fresh "is `label` different
    /// from what's open" check every frame, is what keeps this from fighting a
    /// [`MenuBar::handle_event`](crate::widget::MenuBar::handle_event) keyboard switch.
    pub fn hover(&mut self, label: Option<usize>, len: usize, selectable: impl Fn(usize) -> bool) {
        if label != self.hover {
            if let Some(label) = label
                && self.open_label().is_some_and(|open| open != label)
            {
                self.open(label, len, selectable);
            }
            self.hover = label;
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
    fn starts_with_nothing_open() {
        let state = MenuBarState::new();
        assert_eq!(state.open_label(), None);
        assert!(!state.menu().is_open());
    }

    #[test]
    fn open_selects_the_first_selectable_row() {
        let mut state = MenuBarState::new();
        state.open(1, 3, selectable_at(&[1, 2]));
        assert_eq!(state.open_label(), Some(1));
        assert_eq!(state.menu().selected(), Some(1));
    }

    #[test]
    fn opening_a_different_label_closes_the_first() {
        let mut state = MenuBarState::new();
        state.open(0, 2, selectable_at(&[0, 1]));
        state.open(1, 2, selectable_at(&[0, 1]));
        assert_eq!(state.open_label(), Some(1));
        // Selection reset for the newly opened label, not carried over from label 0.
        assert_eq!(state.menu().selected(), Some(0));
    }

    #[test]
    fn a_label_with_no_selectable_row_never_opens() {
        let mut state = MenuBarState::new();
        state.open(0, 2, selectable_at(&[]));
        assert_eq!(state.open_label(), None);
    }

    #[test]
    fn close_clears_open_label() {
        let mut state = MenuBarState::new();
        state.open(0, 1, selectable_at(&[0]));
        state.close();
        assert_eq!(state.open_label(), None);
        assert!(!state.menu().is_open());
    }

    #[test]
    fn toggle_closes_the_same_label_but_opens_a_different_one() {
        let mut state = MenuBarState::new();
        state.toggle(0, 1, selectable_at(&[0]));
        assert_eq!(state.open_label(), Some(0));
        state.toggle(0, 1, selectable_at(&[0]));
        assert_eq!(state.open_label(), None);

        state.toggle(0, 1, selectable_at(&[0]));
        state.toggle(1, 1, selectable_at(&[0]));
        assert_eq!(state.open_label(), Some(1));
    }

    /// `MenuState::close` (as `MenuState::handle_event`'s Escape path, or
    /// `Menu::show`'s outside-click/activation path, would call on `menu_mut()`) is enough on
    /// its own to make `open_label` report `None`, with no separate bookkeeping to keep in sync.
    #[test]
    fn open_label_reads_none_once_the_inner_menu_state_closes_directly() {
        let mut state = MenuBarState::new();
        state.open(2, 1, selectable_at(&[0]));
        state.menu_mut().close();
        assert_eq!(state.open_label(), None);
    }
}
