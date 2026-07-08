//! [`Shortcuts`]: a small, focus-scoped keyboard shortcut registry.

use retroglyph_core::{Event, KeyCode, KeyModifiers};

/// One registered key combination and what it resolves to.
#[derive(Debug, Clone, Copy)]
struct Binding<Id, Action> {
    /// `None` = fires regardless of focus. `Some(id)` = only fires while
    /// `id` currently holds focus.
    scope: Option<Id>,
    code: KeyCode,
    modifiers: KeyModifiers,
    action: Action,
}

/// Maps key combinations to app-defined `Action`s, the same way
/// [`HitTester`](crate::HitTester) maps a pointer position to a widget id.
///
/// A lookup table an app consults, not something that owns input handling.
/// Bindings are either global (fire regardless of focus) or scoped to a
/// single [`FocusRing`](crate::FocusRing) id (fire only while that id holds
/// focus); [`resolve`](Self::resolve) checks the scoped binding first, so a
/// widget can shadow a global shortcut for the same key while it's focused.
///
/// This does not replace ad hoc `match key.code { .. }` handling for
/// widget-specific navigation (arrow keys meaning "move selection" only
/// while a particular id is focused, say) -- that kind of binding usually
/// carries extra context (list length, current offset) that doesn't fit a
/// flat `Action` enum. `Shortcuts` is for the simple case: one key, always
/// the same `Action`, wherever it's in scope. Bindings are a fixed table set
/// up once (there's no per-frame `begin_frame`/registration step like
/// [`FocusRing`](crate::FocusRing)'s -- a key combination either exists or
/// it doesn't, regardless of what happened to be drawn this frame).
///
/// # Example
///
/// ```
/// use retroglyph_core::{Event, KeyCode, KeyEvent, KeyModifiers};
/// use retroglyph_widgets::Shortcuts;
///
/// #[derive(Clone, Copy, PartialEq, Eq)]
/// enum Id {
///     SearchBox,
/// }
///
/// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// enum Action {
///     ToggleTheme,
///     ClearSearch,
/// }
///
/// let mut shortcuts = Shortcuts::new();
/// shortcuts.bind_global(KeyCode::Char('t'), KeyModifiers::NONE, Action::ToggleTheme);
/// shortcuts.bind_scoped(
///     Id::SearchBox,
///     KeyCode::Escape,
///     KeyModifiers::NONE,
///     Action::ClearSearch,
/// );
///
/// let escape = Event::Key(KeyEvent::new(KeyCode::Escape, KeyModifiers::NONE));
/// assert_eq!(shortcuts.resolve(&escape, Some(Id::SearchBox)), Some(Action::ClearSearch));
/// assert_eq!(shortcuts.resolve(&escape, None), None); // scoped binding, nothing focused
///
/// let t = Event::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
/// assert_eq!(shortcuts.resolve(&t, None), Some(Action::ToggleTheme)); // global, focus-independent
/// ```
#[derive(Debug, Clone)]
pub struct Shortcuts<Id, Action> {
    bindings: Vec<Binding<Id, Action>>,
}

impl<Id, Action> Shortcuts<Id, Action> {
    /// An empty registry.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bindings: Vec::new(),
        }
    }
}

impl<Id: Copy + PartialEq, Action: Copy> Shortcuts<Id, Action> {
    /// Registers a binding that fires regardless of what holds focus.
    pub fn bind_global(&mut self, code: KeyCode, modifiers: KeyModifiers, action: Action) {
        self.bindings.push(Binding {
            scope: None,
            code,
            modifiers,
            action,
        });
    }

    /// Registers a binding that only fires while `id` holds focus.
    pub fn bind_scoped(&mut self, id: Id, code: KeyCode, modifiers: KeyModifiers, action: Action) {
        self.bindings.push(Binding {
            scope: Some(id),
            code,
            modifiers,
            action,
        });
    }

    /// Resolves `event` against `focused` (typically
    /// [`FocusRing::focused`](crate::FocusRing::focused)).
    ///
    /// `None` for anything but a key-down event. Otherwise: the first
    /// registered binding scoped to `focused` with a matching
    /// code/modifiers, or, failing that, the first matching global binding.
    /// A scoped binding never fires for any id other than the one it named,
    /// including when nothing is focused.
    #[must_use]
    pub fn resolve(&self, event: &Event, focused: Option<Id>) -> Option<Action> {
        let Event::Key(key) = event else {
            return None;
        };
        if !key.is_down() {
            return None;
        }
        let matches = |b: &&Binding<Id, Action>| b.code == key.code && b.modifiers == key.modifiers;

        if let Some(focused) = focused
            && let Some(binding) = self
                .bindings
                .iter()
                .find(|b| b.scope == Some(focused) && matches(b))
        {
            return Some(binding.action);
        }
        self.bindings
            .iter()
            .find(|b| b.scope.is_none() && matches(b))
            .map(|b| b.action)
    }
}

// Not `#[derive(Default)]`: that would add unnecessary `Id`/`Action` bounds
// to the generated impl, even though an empty `Vec` never needs them (same
// rationale as `FocusRing`'s manual `Default`).
impl<Id, Action> Default for Shortcuts<Id, Action> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::KeyEvent;

    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Id {
        List,
        Search,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Action {
        ToggleTheme,
        ClearSearch,
        DeleteSelected,
    }

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn global_binding_fires_regardless_of_focus() {
        let mut shortcuts = Shortcuts::new();
        shortcuts.bind_global(KeyCode::Char('t'), KeyModifiers::NONE, Action::ToggleTheme);

        assert_eq!(
            shortcuts.resolve(&key(KeyCode::Char('t')), None),
            Some(Action::ToggleTheme)
        );
        assert_eq!(
            shortcuts.resolve(&key(KeyCode::Char('t')), Some(Id::List)),
            Some(Action::ToggleTheme)
        );
    }

    #[test]
    fn scoped_binding_only_fires_while_its_id_is_focused() {
        let mut shortcuts = Shortcuts::new();
        shortcuts.bind_scoped(
            Id::Search,
            KeyCode::Escape,
            KeyModifiers::NONE,
            Action::ClearSearch,
        );

        assert_eq!(
            shortcuts.resolve(&key(KeyCode::Escape), Some(Id::Search)),
            Some(Action::ClearSearch)
        );
        assert_eq!(
            shortcuts.resolve(&key(KeyCode::Escape), Some(Id::List)),
            None
        );
        assert_eq!(shortcuts.resolve(&key(KeyCode::Escape), None), None);
    }

    #[test]
    fn scoped_binding_takes_priority_over_a_global_one_for_the_same_key() {
        let mut shortcuts = Shortcuts::new();
        shortcuts.bind_global(KeyCode::Delete, KeyModifiers::NONE, Action::ToggleTheme);
        shortcuts.bind_scoped(
            Id::List,
            KeyCode::Delete,
            KeyModifiers::NONE,
            Action::DeleteSelected,
        );

        assert_eq!(
            shortcuts.resolve(&key(KeyCode::Delete), Some(Id::List)),
            Some(Action::DeleteSelected)
        );
        // Different (or no) focus: falls through to the global binding.
        assert_eq!(
            shortcuts.resolve(&key(KeyCode::Delete), Some(Id::Search)),
            Some(Action::ToggleTheme)
        );
        assert_eq!(
            shortcuts.resolve(&key(KeyCode::Delete), None),
            Some(Action::ToggleTheme)
        );
    }

    #[test]
    fn modifiers_must_match_exactly() {
        let mut shortcuts = Shortcuts::<Id, Action>::new();
        shortcuts.bind_global(
            KeyCode::Char('s'),
            KeyModifiers::CONTROL,
            Action::ToggleTheme,
        );

        let ctrl_s = Event::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert_eq!(shortcuts.resolve(&ctrl_s, None), Some(Action::ToggleTheme));
        assert_eq!(shortcuts.resolve(&key(KeyCode::Char('s')), None), None);
    }

    #[test]
    fn ignores_non_key_and_key_up_events() {
        let mut shortcuts = Shortcuts::<Id, Action>::new();
        shortcuts.bind_global(KeyCode::Char('t'), KeyModifiers::NONE, Action::ToggleTheme);

        assert_eq!(shortcuts.resolve(&Event::Close, None), None);

        let released = Event::Key(KeyEvent::with_kind(
            KeyCode::Char('t'),
            KeyModifiers::NONE,
            retroglyph_core::KeyEventKind::Release,
        ));
        assert_eq!(shortcuts.resolve(&released, None), None);
    }

    #[test]
    fn empty_registry_resolves_nothing() {
        let shortcuts = Shortcuts::<Id, Action>::new();
        assert_eq!(shortcuts.resolve(&key(KeyCode::Char('t')), None), None);
    }
}
