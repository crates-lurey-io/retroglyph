//! Shared scroll-window math for offset-anchored-at-start widgets ([`Table`](super::Table),
//! [`List`](super::List)).
//!
//! [`Log`](super::Log) does not use this: it windows backward from the most recent entry rather
//! than forward from `offset`, a different enough direction that it isn't expressed as the same
//! helper -- see `Log`'s own doc comment.

/// The `(original_index, item)` pairs of `items` visible in a `visible_len`-item window starting
/// at `offset`. Out-of-range `offset` simply yields nothing, the same "no upper clamp, caller's
/// responsibility" contract as [`ListState::scroll_by`](crate::ListState::scroll_by).
pub(super) fn visible_window<T>(
    items: &[T],
    offset: usize,
    visible_len: usize,
) -> impl Iterator<Item = (usize, &T)> {
    items.iter().enumerate().skip(offset).take(visible_len)
}
