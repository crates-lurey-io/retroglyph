//! [`Density`]: touch vs. mouse sizing for interactive widgets.

use retroglyph_core::Size;

/// How much room an interactive widget's hit target should claim.
///
/// Not itself consulted by anything in this crate: there are no built-in
/// interactive widgets yet to apply it to; every widget here is a free
/// function or a thin, stateless composition of one (see the crate's module
/// docs). It exists so an app choosing between a phone-sized and a
/// desktop-sized layout has one place to ask "how big should this
/// button/row/slider be", rather than inventing its own ad hoc breakpoint
/// constants per widget (as e.g. `responsive_game_ui`'s own
/// `MIN_TARGET_W`/`MIN_TARGET_H` do today). A future interactive widget in
/// this crate (a checkbox, say) would read [`min_target_size`](Self::min_target_size)
/// the same way it would read [`Sense`](crate::Sense).
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
    /// The minimum size, in cells, an interactive target should claim at
    /// this density.
    #[must_use]
    pub const fn min_target_size(self) -> Size {
        match self {
            Self::Touch => Size::new(6, 3),
            Self::Mouse => Size::new(6, 1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
