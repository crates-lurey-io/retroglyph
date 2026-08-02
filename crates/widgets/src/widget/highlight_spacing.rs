//! [`HighlightSpacing`]: whether [`List`](super::List)'s marker column reserves width when
//! nothing is selected.

/// Whether [`List`](super::List)'s [`highlight_symbol`](super::List::highlight_symbol) marker
/// column reserves width even when [`ListState::selected`](crate::ListState::selected) is
/// `None`.
///
/// [`HighlightSpacing::WhenSelected`] is the default: with nothing selected, the list renders no
/// marker column at all, and one only appears -- shifting every item's text right by the
/// symbol's width -- the moment something becomes selected. [`HighlightSpacing::Always`] reserves
/// the column unconditionally, so item text never shifts as selection comes and goes.
/// [`HighlightSpacing::Never`] never reserves it, so the symbol never renders even while
/// something is selected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HighlightSpacing {
    /// Always reserve the marker column's width, whether or not anything is selected.
    Always,
    /// Reserve the marker column's width only while
    /// [`ListState::selected`](crate::ListState::selected) is `Some`.
    #[default]
    WhenSelected,
    /// Never reserve the marker column's width; the highlight symbol never renders.
    Never,
}
