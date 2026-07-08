//! [`FocusRing`]: keyboard focus and Tab/Shift+Tab cycling over a set of
//! ids established each frame.

use retroglyph_core::{Event, KeyCode};

/// Which id currently holds keyboard focus, plus Tab/Shift+Tab cycling
/// through the ids [`register`](Self::register)ed as focusable.
///
/// Like [`HitTester`](crate::HitTester), registrations are per-frame and
/// draw-ordered, but [`advance`](Self::advance)/[`retreat`](Self::retreat)
/// always walk *last* frame's finalized order -- this frame's registrations
/// aren't complete until the draw pass finishes. `current` itself, unlike
/// the order, persists across frames like any other piece of app state,
/// until focus moves or is [`clear`](Self::clear)ed.
///
/// If the currently focused id isn't in the order being cycled (e.g. it
/// scrolled out of a list, or its widget wasn't drawn this frame), the next
/// [`advance`](Self::advance)/[`retreat`](Self::retreat) treats that the
/// same as nothing being focused, landing on the first/last registered id
/// rather than getting stuck.
#[derive(Debug, Clone)]
pub struct FocusRing<Id> {
    current: Option<Id>,
    order: Vec<Id>,
    pending: Vec<Id>,
}

impl<Id> FocusRing<Id> {
    /// Nothing focused, nothing registered.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            current: None,
            order: Vec::new(),
            pending: Vec::new(),
        }
    }

    /// Finalize this frame's [`register`](Self::register) calls into the
    /// order [`advance`](Self::advance)/[`retreat`](Self::retreat) will walk
    /// during the frame that's about to start, and clear the registration
    /// list for fresh calls. Call once per frame, before drawing.
    pub fn begin_frame(&mut self) {
        self.order = core::mem::take(&mut self.pending);
    }

    /// Drop focus entirely.
    pub fn clear(&mut self) {
        self.current = None;
    }
}

impl<Id: Copy + PartialEq> FocusRing<Id> {
    /// Register `id` as focusable this frame.
    pub fn register(&mut self, id: Id) {
        self.pending.push(id);
    }

    /// The currently focused id, if any.
    #[must_use]
    pub const fn focused(&self) -> Option<Id> {
        self.current
    }

    /// `true` if `id` currently holds focus.
    #[must_use]
    pub fn is_focused(&self, id: Id) -> bool {
        self.current == Some(id)
    }

    /// Explicitly focus `id`, e.g. in response to a click.
    pub const fn request(&mut self, id: Id) {
        self.current = Some(id);
    }

    /// Move focus to the next id in last frame's registration order,
    /// wrapping past the end. Focuses the first registered id if nothing
    /// was focused; a no-op if nothing was registered.
    pub fn advance(&mut self) {
        self.current = Self::step(&self.order, self.current, 1);
    }

    /// Move focus to the previous id in last frame's registration order,
    /// wrapping past the start. Focuses the last registered id if nothing
    /// was focused; a no-op if nothing was registered.
    pub fn retreat(&mut self) {
        self.current = Self::step(&self.order, self.current, -1);
    }

    /// Default Tab/Shift+Tab handling: [`advance`](Self::advance) on `Tab`,
    /// [`retreat`](Self::retreat) on `BackTab` (shift+tab). Called
    /// automatically by [`Interaction::handle_event`](crate::Interaction::handle_event);
    /// call it yourself if you're using `FocusRing` standalone, or skip it
    /// entirely and drive [`advance`](Self::advance)/[`retreat`](Self::retreat)
    /// from something else (a gamepad shoulder button, say) if `Tab` needs
    /// to mean something different in your app (inserting a literal tab
    /// into a text field, for instance).
    pub fn handle_event(&mut self, event: &Event) {
        let Event::Key(key) = event else {
            return;
        };
        if !key.is_down() {
            return;
        }
        match key.code {
            KeyCode::Tab => self.advance(),
            KeyCode::BackTab => self.retreat(),
            _ => {}
        }
    }

    /// Shared wraparound math for `advance`/`retreat`, mirroring
    /// [`ListState`](crate::ListState)'s `select_next`/`select_previous`:
    /// `delta` is `1` or `-1`, and a `current` that's missing (or not found
    /// in `order`) starts from the end opposite the direction of travel so
    /// the first press lands somewhere sensible.
    fn step(order: &[Id], current: Option<Id>, delta: i32) -> Option<Id> {
        if order.is_empty() {
            return None;
        }
        let Ok(len) = i32::try_from(order.len()) else {
            return current; // absurdly large order; leave focus alone
        };
        let index = current.and_then(|id| order.iter().position(|&o| o == id));
        let base = index.map_or(if delta > 0 { -1 } else { 0 }, |i| {
            i32::try_from(i).unwrap_or(0)
        });
        let next = (base + delta).rem_euclid(len);
        usize::try_from(next)
            .ok()
            .and_then(|i| order.get(i))
            .copied()
    }
}

// Not `#[derive(Default)]`: that would add an unnecessary `Id: Default`
// bound to the generated impl, even though empty `Vec<Id>`s never need one.
impl<Id> Default for FocusRing<Id> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::{KeyEvent, KeyModifiers};

    use super::*;

    fn ring_of(ids: &[&'static str]) -> FocusRing<&'static str> {
        let mut ring = FocusRing::new();
        for &id in ids {
            ring.register(id);
        }
        ring.begin_frame();
        ring
    }

    #[test]
    fn advance_from_nothing_focuses_the_first() {
        let mut ring = ring_of(&["a", "b", "c"]);
        ring.advance();
        assert_eq!(ring.focused(), Some("a"));
    }

    #[test]
    fn retreat_from_nothing_focuses_the_last() {
        let mut ring = ring_of(&["a", "b", "c"]);
        ring.retreat();
        assert_eq!(ring.focused(), Some("c"));
    }

    #[test]
    fn advance_wraps_past_the_end() {
        let mut ring = ring_of(&["a", "b"]);
        ring.request("b");
        ring.advance();
        assert_eq!(ring.focused(), Some("a"));
    }

    #[test]
    fn retreat_wraps_past_the_start() {
        let mut ring = ring_of(&["a", "b"]);
        ring.request("a");
        ring.retreat();
        assert_eq!(ring.focused(), Some("b"));
    }

    #[test]
    fn stale_focus_not_in_order_is_treated_as_unfocused() {
        let mut ring = ring_of(&["a", "b"]);
        ring.request("gone"); // e.g. the widget that had focus wasn't drawn this frame
        ring.advance();
        assert_eq!(ring.focused(), Some("a"));
    }

    #[test]
    fn empty_order_is_a_no_op() {
        let mut ring: FocusRing<&str> = FocusRing::new();
        ring.begin_frame();
        ring.advance();
        assert_eq!(ring.focused(), None);
    }

    #[test]
    fn tab_and_backtab_cycle_focus() {
        let mut ring = ring_of(&["a", "b"]);
        ring.handle_event(&Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
        assert_eq!(ring.focused(), Some("a"));
        ring.handle_event(&Event::Key(KeyEvent::new(
            KeyCode::BackTab,
            KeyModifiers::NONE,
        )));
        assert_eq!(ring.focused(), Some("b")); // wraps backward from "a"
    }

    #[test]
    fn clear_drops_focus() {
        let mut ring = ring_of(&["a"]);
        ring.request("a");
        ring.clear();
        assert_eq!(ring.focused(), None);
    }
}
