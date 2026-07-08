//! [`Response`]: what [`Interaction::interact`](crate::Interaction::interact)
//! hands back to a widget call site.

/// What happened to a widget this frame, as reported by
/// [`Interaction::interact`](crate::Interaction::interact).
///
/// Every field is scoped to *this* frame only (e.g. [`clicked`](Self::clicked)
/// is `true` for exactly the one frame the release lands on), except
/// [`focused`](Self::focused), which stays `true` across frames until focus
/// moves elsewhere. Fields a widget didn't ask for via
/// [`Sense`](crate::Sense) are always `false`/`0` -- a widget sensed with
/// only [`Sense::HOVER`](crate::Sense::HOVER) never reports
/// [`clicked`](Self::clicked), for instance.
// Eight flat, independent fields by design: `Response` is a per-frame
// report card, not a state machine -- collapsing it into enums would only
// make `interact`'s construction of it more awkward for no reader benefit.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Response {
    pub(crate) hovered: bool,
    pub(crate) pressed: bool,
    pub(crate) released: bool,
    pub(crate) clicked: bool,
    pub(crate) dragging: bool,
    pub(crate) focused: bool,
    pub(crate) secondary_clicked: bool,
    pub(crate) scroll_delta: i32,
}

impl Response {
    /// The pointer is over this widget's rect, resolved from last frame's
    /// hit-test -- see [`Interaction`](crate::Interaction) for why there's a
    /// frame of latency.
    #[must_use]
    pub const fn hovered(&self) -> bool {
        self.hovered
    }

    /// The primary pointer button went down on this widget this frame, or
    /// (sensed with [`Sense::FOCUSABLE`](crate::Sense::FOCUSABLE)) Enter or
    /// Space was pressed while it was focused.
    #[must_use]
    pub const fn pressed(&self) -> bool {
        self.pressed
    }

    /// The primary pointer button (or an activating key) was released this
    /// frame while this widget was the active one. Fires whether or not the
    /// release also counts as a [`clicked`](Self::clicked) (e.g. it doesn't,
    /// if the gesture crossed the drag threshold first).
    #[must_use]
    pub const fn released(&self) -> bool {
        self.released
    }

    /// A full press-release cycle landed on this widget this frame: pressed
    /// and released while still hovered, never crossing the drag threshold.
    /// Also fires from keyboard activation (Enter/Space while focused) --
    /// terminals are frequently mouse-less, so [`Sense::click`](crate::Sense::click)
    /// widgets are keyboard-operable by default.
    #[must_use]
    pub const fn clicked(&self) -> bool {
        self.clicked
    }

    /// The pointer moved past the drag threshold while pressed on this
    /// widget. Only ever `true` for widgets sensed with
    /// [`Sense::DRAG`](crate::Sense::DRAG).
    #[must_use]
    pub const fn dragging(&self) -> bool {
        self.dragging
    }

    /// This widget holds keyboard focus. Unlike the other fields, this is
    /// level state, not a one-shot "this happened" flag: it stays `true`
    /// across frames until focus moves to another widget or is cleared.
    #[must_use]
    pub const fn focused(&self) -> bool {
        self.focused
    }

    /// The secondary (right) mouse button pressed and released on this
    /// widget this frame while still hovered. Only ever `true` for widgets
    /// sensed with [`Sense::SECONDARY_CLICK`](crate::Sense::SECONDARY_CLICK).
    /// Unlike [`clicked`](Self::clicked), there's no keyboard equivalent --
    /// a secondary action needs its own trigger (a modifier+Enter, a menu
    /// key, whatever fits the app) since Enter/Space already means
    /// "primary activate".
    #[must_use]
    pub const fn secondary_clicked(&self) -> bool {
        self.secondary_clicked
    }

    /// Scroll wheel delta accumulated this frame while the pointer was
    /// within this widget's rect (regardless of what else was drawn on top
    /// of it -- see [`Sense::SCROLL`](crate::Sense::SCROLL)): positive
    /// scrolls forward/down, negative scrolls backward/up. Feeds straight
    /// into [`ListState::scroll_by`](crate::ListState::scroll_by). Zero
    /// unless sensed with `SCROLL` and something scrolled.
    #[must_use]
    pub const fn scroll_delta(&self) -> i32 {
        self.scroll_delta
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_all_falsy() {
        let r = Response::default();
        assert!(!r.hovered());
        assert!(!r.pressed());
        assert!(!r.released());
        assert!(!r.clicked());
        assert!(!r.dragging());
        assert!(!r.focused());
        assert!(!r.secondary_clicked());
        assert_eq!(r.scroll_delta(), 0);
    }
}
