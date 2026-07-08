//! [`Density`]: compact vs. relaxed sizing for interactive widgets.

use retroglyph_core::Size;

/// How much room an interactive widget's hit target should claim.
///
/// Not itself consulted by anything in this crate -- there are no built-in
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
pub enum Density {
    /// Smaller interactive targets, for narrow or short terminals, or touch
    /// input where every cell of screen space is scarce.
    Compact,
    /// Larger interactive targets, comfortable to hit with a mouse on a
    /// normal desktop-sized terminal.
    Relaxed,
}

impl Density {
    /// The minimum size, in cells, an interactive target should claim at
    /// this density.
    ///
    /// Counter-intuitively, [`Compact`](Self::Compact) targets are *taller*
    /// than [`Relaxed`](Self::Relaxed) ones, not shorter: "compact" here
    /// means a narrow, likely touch-driven layout (a phone-width terminal,
    /// say), where a fingertip needs a noticeably taller row than a mouse
    /// pointer does, at the cost of showing fewer rows at once.
    /// [`Relaxed`](Self::Relaxed) assumes a normal desktop terminal with a
    /// mouse, where dense, single-line rows are both legible and easy to
    /// click precisely.
    #[must_use]
    pub const fn min_target_size(self) -> Size {
        match self {
            Self::Compact => Size {
                width: 6,
                height: 3,
            },
            Self::Relaxed => Size {
                width: 6,
                height: 1,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_rows_are_taller_than_relaxed_for_touch_targets() {
        let compact = Density::Compact.min_target_size();
        let relaxed = Density::Relaxed.min_target_size();
        assert!(compact.height > relaxed.height);
    }

    #[test]
    fn relaxed_still_claims_more_than_a_single_cell_wide() {
        let size = Density::Relaxed.min_target_size();
        assert!(size.width > 1);
    }
}
