//! [`Response`]: what [`Interaction::interact`](crate::Interaction::interact)
//! hands back to a widget call site.

use retroglyph_core::{Pos, Rect};

/// What happened to a widget this frame, as reported by
/// [`Interaction::interact`](crate::Interaction::interact).
///
/// Every field is scoped to *this* frame only (e.g. [`clicked`](Self::clicked)
/// is `true` for exactly the one frame the release lands on), except
/// [`focused`](Self::focused), which stays `true` across frames until focus
/// moves elsewhere. Fields a widget didn't ask for via
/// [`Sense`](crate::Sense) are always `false`/`0`: a widget sensed with
/// only [`Sense::HOVER`](crate::Sense::HOVER) never reports
/// [`clicked`](Self::clicked), for instance.
// Flat, independent fields by design: `Response` is a per-frame report card, not a state
// machine: collapsing it into enums would only make `interact`'s construction of it more
// awkward for no reader benefit.
//
// `id` is a bare `Id`, not `Option<Id>`: every `Response` that ever reaches app code comes from
// `Interaction::interact`, which always has a real id in hand, so wrapping it in `Option` would
// just make every caller unwrap a value that's never actually absent. The one place this crate
// builds a `Response` without going through `interact` is [`Response::default`], used as a
// synthetic "nothing happened" value (see [`Widget`](crate::Widget) impls that share their
// [`InteractiveWidget`](crate::InteractiveWidget) drawing routine, and this crate's own tests).
// `Default` is implemented for `Response<()>` specifically, not `impl<Id: Default>` generically:
// nothing here ever needs a default `Response` under a real app `Id`, since a real `Id` only ever
// reaches a `Response` through `interact`, so there's no reason to demand every `Id` a caller
// picks implement `Default` just so `Response<Id>` itself can.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Response<Id> {
    pub(crate) id: Id,
    pub(crate) hovered: bool,
    pub(crate) pressed: bool,
    pub(crate) released: bool,
    pub(crate) clicked: bool,
    pub(crate) double_clicked: bool,
    pub(crate) held: bool,
    pub(crate) dragging: bool,
    pub(crate) focused: bool,
    pub(crate) gained_focus: bool,
    pub(crate) lost_focus: bool,
    pub(crate) secondary_clicked: bool,
    pub(crate) disabled: bool,
    pub(crate) scroll_delta: i32,
    pub(crate) pointer_pos: Option<Pos>,
    pub(crate) press_origin: Option<Pos>,
    pub(crate) drag_delta: Option<(i32, i32)>,
    pub(crate) rect: Rect,
}

impl Default for Response<()> {
    fn default() -> Self {
        Self {
            id: (),
            hovered: false,
            pressed: false,
            released: false,
            clicked: false,
            double_clicked: false,
            held: false,
            dragging: false,
            focused: false,
            gained_focus: false,
            lost_focus: false,
            secondary_clicked: false,
            disabled: false,
            scroll_delta: 0,
            pointer_pos: None,
            press_origin: None,
            drag_delta: None,
            rect: Rect::default(),
        }
    }
}

impl<Id: Copy> Response<Id> {
    /// The `id` passed to [`Interaction::interact`](crate::Interaction::interact) this frame,
    /// echoed back so a call site that only has the resolved `Response` in hand (e.g. one
    /// returned from a widget it drew in a loop) can still tell which id it belongs to. For a
    /// [`Response::default`] built directly rather than through `interact`, this is just
    /// `Id::default()`, not a real widget's id.
    #[must_use]
    pub const fn id(&self) -> Id {
        self.id
    }
}

impl<Id> Response<Id> {
    /// The pointer is over this widget's rect, resolved from last frame's
    /// hit-test: see [`Interaction`](crate::Interaction) for why there's a
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

    /// A second [`clicked`](Self::clicked) landed on this widget within
    /// [`Interaction::with_double_click_window`](crate::Interaction::with_double_click_window)
    /// frames of the first, e.g. to open a file on double-click while a single click just
    /// selects it. Implies [`clicked`](Self::clicked) is also `true` this frame. A third click
    /// starts counting fresh rather than re-firing every frame after the second: each pair of
    /// qualifying clicks reports exactly one `double_clicked` frame.
    #[must_use]
    pub const fn double_clicked(&self) -> bool {
        self.double_clicked
    }

    /// The primary pointer button is down *and* the pointer is currently over this widget's
    /// rect, re-checked live every frame, unlike [`pressed`](Self::pressed), which fires
    /// once on the down edge and never re-checks position. Automatically cancels (goes
    /// `false`) the instant the pointer slides off this widget's rect, even before release,
    /// without waiting for a release event: the same "slide-to-cancel" feedback
    /// `is_pointer_button_down_on` gives egui widgets and `IsItemHovered() && IsItemActive()`
    /// gives Dear `ImGui` widgets. Only ever `true` for widgets sensed with
    /// [`Sense::CLICK`](crate::Sense::CLICK).
    #[must_use]
    pub const fn held(&self) -> bool {
        self.held
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

    /// This widget just became [`focused`](Self::focused) this frame, having not been focused
    /// last frame: a one-shot edge for a widget that wants to react only on the transition (e.g.
    /// select-all-on-focus for a text input), rather than every frame [`focused`](Self::focused)
    /// happens to be `true`.
    #[must_use]
    pub const fn gained_focus(&self) -> bool {
        self.gained_focus
    }

    /// This widget was [`focused`](Self::focused) last frame but isn't anymore: the mirror of
    /// [`gained_focus`](Self::gained_focus), e.g. to commit a text input's edits when focus
    /// moves away.
    #[must_use]
    pub const fn lost_focus(&self) -> bool {
        self.lost_focus
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
    /// of it; see [`Sense::SCROLL`](crate::Sense::SCROLL)): positive
    /// scrolls forward/down, negative scrolls backward/up. Feeds straight
    /// into [`ListState::scroll_by`](crate::ListState::scroll_by). Zero
    /// unless sensed with `SCROLL` and something scrolled.
    #[must_use]
    pub const fn scroll_delta(&self) -> i32 {
        self.scroll_delta
    }

    /// This widget was interacted with via a [`Sense`](crate::Sense) that
    /// had [`Sense::DISABLED`](crate::Sense::DISABLED) set.
    /// [`hovered`](Self::hovered) still works, so a disabled control can
    /// show a tooltip explaining why; every other field above is `false`
    /// (or `0`) regardless of what the pointer/keyboard did. Widgets should
    /// read this instead of threading a parallel `enabled` bool into their
    /// own draw call.
    #[must_use]
    pub const fn disabled(&self) -> bool {
        self.disabled
    }

    /// Where the pointer is, in grid coordinates, resolved from the same last-frame snapshot
    /// [`hovered`](Self::hovered) is computed from: `Some` exactly when this widget's rect
    /// contains the pointer, `None` when it's elsewhere or this widget wasn't sensed with a
    /// pointer-registering [`Sense`](crate::Sense). Lets a composite widget shown under a single
    /// id (a list of rows, a strip of tabs) resolve which of its own parts the pointer is over,
    /// without needing a distinct id per part.
    #[must_use]
    pub const fn pointer_pos(&self) -> Option<Pos> {
        self.pointer_pos
    }

    /// Where the pointer was when the press currently active on this widget landed. Unlike
    /// [`pointer_pos`](Self::pointer_pos), this stays put for the duration of a press or drag
    /// rather than tracking the pointer's current position: useful for a composite widget (e.g.
    /// a scrollbar thumb) that needs to measure a drag's total displacement from where it
    /// started. `None` unless this widget is the one a press is currently active on.
    #[must_use]
    pub const fn press_origin(&self) -> Option<Pos> {
        self.press_origin
    }

    /// How far the pointer has moved from [`press_origin`](Self::press_origin), signed and in
    /// grid cells: `(pointer.x - press_origin.x, pointer.y - press_origin.y)`. Unlike
    /// [`pointer_pos`](Self::pointer_pos), this keeps reporting once the pointer slides outside
    /// this widget's own rect, which is the common case for a [`Sense::DRAG`](crate::Sense::DRAG)
    /// widget like a scrollbar thumb or a resizable pane divider: both need the drag's full
    /// displacement, not just the part of it that stayed over the thumb. `None` under the same
    /// condition [`press_origin`](Self::press_origin) is `None`: no press is currently active on
    /// this widget.
    #[must_use]
    pub const fn drag_delta(&self) -> Option<(i32, i32)> {
        self.drag_delta
    }

    /// The `area` this widget was shown at, as passed to
    /// [`Interaction::interact`](crate::Interaction::interact) this frame. Useful for anything
    /// that draws relative to where this widget landed, e.g. a tooltip anchored under its
    /// trigger.
    #[must_use]
    pub const fn rect(&self) -> Rect {
        self.rect
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_all_falsy() {
        let r: Response<()> = Response::default();
        assert_eq!(r.id(), ());
        assert!(!r.hovered());
        assert!(!r.pressed());
        assert!(!r.released());
        assert!(!r.clicked());
        assert!(!r.double_clicked());
        assert!(!r.held());
        assert!(!r.dragging());
        assert!(!r.focused());
        assert!(!r.gained_focus());
        assert!(!r.lost_focus());
        assert!(!r.secondary_clicked());
        assert!(!r.disabled());
        assert_eq!(r.scroll_delta(), 0);
        assert_eq!(r.pointer_pos(), None);
        assert_eq!(r.press_origin(), None);
        assert_eq!(r.drag_delta(), None);
        assert_eq!(r.rect(), Rect::default());
    }
}
