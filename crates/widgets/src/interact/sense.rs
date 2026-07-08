//! [`Sense`]: what a widget wants [`Interaction::interact`](crate::Interaction::interact)
//! to compute on its behalf.

use core::ops::{BitOr, BitOrAssign};

/// Which of a [`Response`](crate::Response)'s fields
/// [`Interaction::interact`](crate::Interaction::interact) should actually
/// populate for a given widget call.
///
/// A manual bitflag over `u8` -- mirrors
/// [`KeyModifiers`](retroglyph_core::KeyModifiers)'s shape rather than
/// pulling in the `bitflags` crate for five bits. Combine raw flags with
/// `|` (`Sense::HOVER | Sense::FOCUSABLE`), or reach for one of the named
/// constructors ([`click`](Self::click), [`drag`](Self::drag),
/// [`hover`](Self::hover), [`scroll`](Self::scroll)) for the common cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Sense(u8);

impl Sense {
    /// Register the widget's rect for hit-testing and report
    /// [`Response::hovered`](crate::Response::hovered).
    pub const HOVER: Self = Self(1 << 0);
    /// Report [`Response::pressed`](crate::Response::pressed),
    /// [`Response::released`](crate::Response::released), and
    /// [`Response::clicked`](crate::Response::clicked).
    pub const CLICK: Self = Self(1 << 1);
    /// Report [`Response::dragging`](crate::Response::dragging) once the
    /// pointer moves past the drag threshold while pressed on this widget.
    pub const DRAG: Self = Self(1 << 2);
    /// Register the widget in the [`FocusRing`](crate::FocusRing)'s
    /// Tab/Shift+Tab order and report
    /// [`Response::focused`](crate::Response::focused). Combined with
    /// [`CLICK`](Self::CLICK), Enter/Space also activate the widget while
    /// it's focused -- terminals are frequently mouse-less.
    pub const FOCUSABLE: Self = Self(1 << 3);
    /// Report [`Response::scroll_delta`](crate::Response::scroll_delta)
    /// whenever the pointer is within this widget's rect. Unlike the other
    /// pointer senses, this is deliberately *not* limited to the single
    /// topmost widget under the pointer -- see [`Interaction::interact`](crate::Interaction::interact)'s
    /// doc comment on `scroll_delta` for why.
    pub const SCROLL: Self = Self(1 << 4);
    /// Senses nothing: [`interact`](crate::Interaction::interact) still
    /// registers the id nowhere and returns [`Response::default`](crate::Response).
    pub const NONE: Self = Self(0);

    /// A clickable, hoverable, focusable widget -- buttons, tabs, list
    /// rows. Equivalent to `HOVER | CLICK | FOCUSABLE`.
    #[must_use]
    pub const fn click() -> Self {
        Self(Self::HOVER.0 | Self::CLICK.0 | Self::FOCUSABLE.0)
    }

    /// A draggable widget, e.g. a slider or scrollbar thumb. Equivalent to
    /// <code>[click](Self::click) | DRAG</code>.
    #[must_use]
    pub const fn drag() -> Self {
        Self(Self::click().0 | Self::DRAG.0)
    }

    /// A hover-only widget with no click or focus behavior, e.g. a tooltip
    /// trigger. Equivalent to `HOVER`.
    #[must_use]
    pub const fn hover() -> Self {
        Self::HOVER
    }

    /// A scrollable region, e.g. a list or log panel. Equivalent to
    /// `HOVER | SCROLL`.
    #[must_use]
    pub const fn scroll() -> Self {
        Self(Self::HOVER.0 | Self::SCROLL.0)
    }

    /// `true` if every bit set in `other` is also set in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// `true` if this sense wants pointer hit-testing at all ([`HOVER`](Self::HOVER),
    /// [`CLICK`](Self::CLICK), [`DRAG`](Self::DRAG), or [`SCROLL`](Self::SCROLL)).
    #[must_use]
    pub const fn wants_pointer(self) -> bool {
        self.0 & (Self::HOVER.0 | Self::CLICK.0 | Self::DRAG.0 | Self::SCROLL.0) != 0
    }
}

impl BitOr for Sense {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for Sense {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_checks_all_bits() {
        let s = Sense::HOVER | Sense::FOCUSABLE;
        assert!(s.contains(Sense::HOVER));
        assert!(s.contains(Sense::FOCUSABLE));
        assert!(!s.contains(Sense::CLICK));
        assert!(s.contains(Sense::NONE)); // vacuously true
    }

    #[test]
    fn constructors_match_their_documented_bit_combinations() {
        assert_eq!(
            Sense::click(),
            Sense::HOVER | Sense::CLICK | Sense::FOCUSABLE
        );
        assert_eq!(Sense::drag(), Sense::click() | Sense::DRAG);
        assert_eq!(Sense::hover(), Sense::HOVER);
        assert_eq!(Sense::scroll(), Sense::HOVER | Sense::SCROLL);
    }

    #[test]
    fn wants_pointer_ignores_focusable() {
        assert!(!Sense::FOCUSABLE.wants_pointer());
        assert!(Sense::HOVER.wants_pointer());
        assert!(Sense::CLICK.wants_pointer());
        assert!(Sense::DRAG.wants_pointer());
        assert!(Sense::SCROLL.wants_pointer());
        assert!(!Sense::NONE.wants_pointer());
    }

    #[test]
    fn default_is_none() {
        assert_eq!(Sense::default(), Sense::NONE);
    }
}
