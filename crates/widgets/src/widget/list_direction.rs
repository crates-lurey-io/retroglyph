//! [`ListDirection`]: which end of a [`List`](super::List)'s area its items render from.

/// Which end of a [`List`](super::List)'s area its items render from.
///
/// [`ListDirection::TopToBottom`] is the default: `items[state.offset()]` draws at the top row
/// and later items draw downward, the list's only behavior before this type existed.
/// [`ListDirection::BottomToTop`] draws the same window from the bottom of the area instead:
/// `items[state.offset()]` draws at the bottom row and later items draw upward, for a
/// chat/log-style list that grows toward the top.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ListDirection {
    /// `items[state.offset()]` draws at the top of the area; later items draw downward.
    #[default]
    TopToBottom,
    /// `items[state.offset()]` draws at the bottom of the area; later items draw upward.
    BottomToTop,
}
