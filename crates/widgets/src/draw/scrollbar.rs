//! [`thumb_geometry`]/[`offset_for_pos`]: pure scrollbar geometry, kept as
//! functions (not [`widget::Scrollbar`](crate::widget::Scrollbar) methods)
//! because they have legitimate standalone callers that never draw anything
//! -- e.g. hit-testing a click/drag against the thumb via
//! [`Interaction::interact`](crate::Interaction::interact) with
//! [`Sense::DRAG`](crate::Sense::DRAG), independently of (and possibly
//! before) ever rendering a [`widget::Scrollbar`](crate::widget::Scrollbar).
//! This module has no dependency on (or awareness of) [`crate::interact`],
//! and stays that way on purpose.

use retroglyph_core::{Pos, Rect};

/// The thumb's row span within `area` (`(start, len)`, both relative to
/// `area.top()`) for a vertical scrollbar covering `total_len` items in a
/// `visible_len`-row viewport currently starting at `offset`.
///
/// `None` if there's nothing to scroll (`area` has no rows, `visible_len`
/// is zero, or `total_len <= visible_len` -- the whole track already fits
/// in the viewport). [`widget::Scrollbar`](crate::widget::Scrollbar) falls
/// back to drawing a plain, thumb-less track in that case.
///
/// The thumb is sized proportionally to `visible_len / total_len` (clamped
/// to at least one row so it's never invisible) and positioned
/// proportionally to `offset` within the remaining scrollable range.
#[must_use]
pub fn thumb_geometry(
    area: Rect,
    total_len: usize,
    visible_len: usize,
    offset: usize,
) -> Option<(u16, u16)> {
    let track = area.height();
    if track == 0 || visible_len == 0 || total_len <= visible_len {
        return None;
    }

    let track_f = f32::from(track);
    #[allow(clippy::cast_precision_loss)]
    let ratio = visible_len as f32 / total_len as f32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let len = (track_f * ratio).round().clamp(1.0, track_f) as u16;

    let max_offset = total_len - visible_len; // > 0, since total_len > visible_len here
    let max_start = track.saturating_sub(len); // last row the thumb can start on
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let start = ((offset as f32 / max_offset as f32) * f32::from(max_start)).round() as u16;

    Some((start.min(max_start), len))
}

/// The offset a vertical scrollbar should jump to for a click/drag at `pos`.
///
/// Covers `total_len` items in a `visible_len`-row `area`; useful for
/// click-to-jump or drag-to-scroll interactions built on top of
/// [`thumb_geometry`]. `None` if `pos` falls outside `area`, or (mirroring
/// [`thumb_geometry`]) there's nothing to scroll.
#[must_use]
pub fn offset_for_pos(area: Rect, total_len: usize, visible_len: usize, pos: Pos) -> Option<usize> {
    if !area.contains_pos(pos) || total_len <= visible_len {
        return None;
    }

    let max_offset = total_len - visible_len;
    // height() - 1, not height(): mapping the *last* row to max_offset (not
    // one row short of it) needs `rel` to range over 0..=track, not
    // 0..track -- an off-by-one that would otherwise leave the bottom row
    // of the track unable to reach the maximum offset.
    let track = area.height().saturating_sub(1).max(1);
    let rel = pos.y.saturating_sub(area.top()).min(track);
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let offset = (f32::from(rel) / f32::from(track) * max_offset as f32).round() as usize;

    Some(offset.min(max_offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_to_scroll_when_everything_fits() {
        assert_eq!(thumb_geometry(Rect::new(0, 0, 1, 10), 5, 10, 0), None);
        assert_eq!(thumb_geometry(Rect::new(0, 0, 1, 10), 10, 10, 0), None);
    }

    #[test]
    fn thumb_shrinks_with_the_visible_fraction() {
        // Half the content visible -> roughly half the track.
        let (_, len) = thumb_geometry(Rect::new(0, 0, 1, 20), 20, 10, 0).unwrap();
        assert_eq!(len, 10);

        // A tiny fraction still gets at least one row, never zero.
        let (_, len) = thumb_geometry(Rect::new(0, 0, 1, 20), 2000, 1, 0).unwrap();
        assert_eq!(len, 1);
    }

    #[test]
    fn thumb_moves_from_top_to_bottom_as_offset_increases() {
        let area = Rect::new(0, 0, 1, 20);
        let (start_at_zero, len) = thumb_geometry(area, 40, 10, 0).unwrap();
        assert_eq!(start_at_zero, 0);

        let (start_at_max, _) = thumb_geometry(area, 40, 10, 30).unwrap(); // max_offset = 30
        assert_eq!(start_at_max, area.height() - len); // flush with the bottom

        let (start_at_mid, _) = thumb_geometry(area, 40, 10, 15).unwrap();
        assert!(start_at_mid > start_at_zero && start_at_mid < start_at_max);
    }

    #[test]
    fn offset_for_pos_round_trips_thumb_geometry_endpoints() {
        let area = Rect::new(0, 0, 1, 20);
        assert_eq!(
            offset_for_pos(area, 40, 10, Pos::new(0, area.top())),
            Some(0)
        );
        assert_eq!(
            offset_for_pos(area, 40, 10, Pos::new(0, area.bottom() - 1)),
            Some(30) // max_offset
        );
    }

    #[test]
    fn offset_for_pos_outside_the_area_is_none() {
        let area = Rect::new(5, 5, 1, 10);
        assert_eq!(offset_for_pos(area, 40, 10, Pos::new(0, 0)), None);
    }
}
