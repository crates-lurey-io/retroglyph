//! [`Density`]: touch vs. mouse sizing for interactive widgets.

use retroglyph_core::Size;

/// How much room an interactive widget's hit target should claim.
///
/// It exists so an app choosing between a phone-sized and a desktop-sized
/// layout has one place to ask "how big should this button/row/slider be",
/// rather than inventing its own ad hoc breakpoint constants per widget. An
/// [`InteractiveWidget`](crate::InteractiveWidget) reads
/// [`min_target_size`](Self::min_target_size) the same way it reads
/// [`sense`](crate::InteractiveWidget::sense); [`for_width`](Self::for_width) is the other
/// half, turning a terminal width into a `Density` in the first place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Density {
    /// Larger interactive targets, for a fingertip on a phone-width terminal
    /// or other touch input, at the cost of showing fewer rows at once.
    Touch,
    /// Dense, single-line interactive targets, for a mouse on a normal
    /// desktop-sized terminal, where precise clicking doesn't need extra
    /// height.
    Mouse,
}

impl Density {
    /// Below this width, in cells, [`for_width`](Self::for_width) picks [`Density::Touch`]; at
    /// or above it, [`Density::Mouse`]. An app that wants a different breakpoint states it
    /// relative to this default (e.g. `if width < Density::DEFAULT_BREAKPOINT_WIDTH - 4 { .. }`)
    /// rather than inventing an unrelated constant from scratch.
    pub const DEFAULT_BREAKPOINT_WIDTH: u16 = 64;

    /// The minimum size, in cells, an interactive target should claim at
    /// this density.
    #[must_use]
    pub const fn min_target_size(self) -> Size {
        match self {
            Self::Touch => Size::new(6, 3),
            Self::Mouse => Size::new(6, 1),
        }
    }

    /// Picks a density from a terminal (or pane) width, in cells, against
    /// [`DEFAULT_BREAKPOINT_WIDTH`](Self::DEFAULT_BREAKPOINT_WIDTH).
    ///
    /// A one-line replacement for the ad hoc `if width < N { .. }` every consumer of this crate
    /// has otherwise had to write for itself, with a different `N` each time.
    #[must_use]
    pub const fn for_width(width: u16) -> Self {
        if width < Self::DEFAULT_BREAKPOINT_WIDTH {
            Self::Touch
        } else {
            Self::Mouse
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use retroglyph_core::HasSize;

    #[test]
    fn touch_rows_are_taller_than_mouse_for_fingertip_targets() {
        let touch = Density::Touch.min_target_size();
        let mouse = Density::Mouse.min_target_size();
        assert!(touch.height() > mouse.height());
    }

    #[test]
    fn mouse_still_claims_more_than_a_single_cell_wide() {
        let size = Density::Mouse.min_target_size();
        assert!(size.width() > 1);
    }

    #[test]
    fn for_width_picks_touch_below_the_default_breakpoint() {
        assert_eq!(
            Density::for_width(Density::DEFAULT_BREAKPOINT_WIDTH - 1),
            Density::Touch
        );
    }

    #[test]
    fn for_width_picks_mouse_at_or_above_the_default_breakpoint() {
        assert_eq!(
            Density::for_width(Density::DEFAULT_BREAKPOINT_WIDTH),
            Density::Mouse
        );
        assert_eq!(
            Density::for_width(Density::DEFAULT_BREAKPOINT_WIDTH + 1),
            Density::Mouse
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serializes_as_a_plain_string() {
        let json = serde_json::to_string(&Density::Touch).expect("serialize");
        assert_eq!(json, "\"Touch\"");
        assert_eq!(
            serde_json::from_str::<Density>(&json).expect("deserialize"),
            Density::Touch
        );
    }
}
