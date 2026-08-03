//! [`thumb_geometry`]/[`offset_for_pos`]: pure scrollbar geometry, kept as
//! functions (not [`widget::Scrollbar`](crate::widget::Scrollbar) methods)
//! because they have legitimate standalone callers that never draw anything:
//! e.g. hit-testing a click/drag against the thumb via
//! [`Interaction::interact`](crate::Interaction::interact) with
//! [`Sense::DRAG`](crate::Sense::DRAG), independently of (and possibly
//! before) ever rendering a [`widget::Scrollbar`](crate::widget::Scrollbar).
//! This module has no dependency on (or awareness of) [`crate::interact`],
//! and stays that way on purpose.

use retroglyph_core::{Pos, Rect};

/// The thumb's length, in rows, for a `track`-row-tall vertical scrollbar covering `total_len`
/// items in a `visible_len`-row viewport: proportional to `visible_len / total_len`, clamped to
/// `1..=track` so it's never invisible (and never taller than the track itself).
///
/// Shared by [`thumb_geometry`] and [`offset_for_pos`] so both agree on how much of the track the
/// thumb occupies, and therefore on [`TrackMap::max_start`], the last row the thumb can start on.
fn thumb_len(track: u16, total_len: usize, visible_len: usize) -> u16 {
    let track_f = f32::from(track);
    // `visible_len`/`total_len` are item counts feeding a display ratio; losing mantissa bits
    // above f32's 2^23 threshold has no visible effect on scrollbar geometry.
    #[allow(clippy::cast_precision_loss)]
    let ratio = visible_len as f32 / total_len as f32;
    // Explicitly clamped to `1.0..=track_f`, itself derived from `area`'s `u16` height, so the
    // result always narrows back exactly and is never negative.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let len = (track_f * ratio).round().clamp(1.0, track_f) as u16;

    len
}

/// A linear map between a scrollbar's `offset` domain (`0..=max_offset`, item units) and its
/// thumb's row domain (`0..=max_start`, the rows the thumb's *top* can start on within the
/// track): the single shared implementation behind both [`thumb_geometry`]'s `offset -> row` and
/// [`offset_for_pos`]'s `row -> offset`, so the two agree on both the denominator and the
/// rounding, instead of independently re-deriving (and disagreeing on) the same conversion.
///
/// `row_for`/`offset_for` are each other's nearest-integer inverse, computed from the same two
/// fields with the same rounding, rather than (as before this type existed) two independently
/// re-derived formulas that disagreed on both. That makes `row -> offset -> row` an *exact* round
/// trip whenever `max_start <= max_offset` (the usual case: a track has far fewer rows than a
/// scrollable list has items, so `max_start`, itself capped by the track's row count, is the
/// smaller of the two): `row_for(offset_for(row)) == row` for every `row` in `0..=max_start`. The
/// reverse, `offset -> row -> offset`, can't be exact in that same common case, since several
/// offsets necessarily share a single row; `offset_for(row_for(offset))` is only guaranteed to
/// land within one row's worth of `offset`, not always exactly `offset` (this is what the old
/// mismatched formulas got wrong: they could drift by several rows' worth, not just one).
struct TrackMap {
    max_offset: usize,
    max_start: u16,
}

impl TrackMap {
    /// `offset` (clamped to `0..=max_offset`) -> the thumb's top row, in `0..=max_start`.
    fn row_for(&self, offset: usize) -> u16 {
        if self.max_offset == 0 {
            return 0;
        }
        let offset = offset.min(self.max_offset);
        // `offset <= max_offset`, so the ratio is in `0.0..=1.0`; the result is clamped below to
        // `max_start` (itself a `u16`) and is never negative. `offset`/`max_offset` are item
        // counts (same precision-loss rationale as `thumb_len`'s ratio).
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        let row =
            ((offset as f32 / self.max_offset as f32) * f32::from(self.max_start)).round() as u16;
        row.min(self.max_start)
    }

    /// `row` (clamped to `0..=max_start`) -> an offset in `0..=max_offset`.
    fn offset_for(&self, row: u16) -> usize {
        if self.max_start == 0 {
            return self.max_offset;
        }
        let row = row.min(self.max_start);
        // Mirrors `row_for`'s cast rationale, in the opposite direction.
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        let offset = ((f32::from(row) / f32::from(self.max_start)) * self.max_offset as f32).round()
            as usize;
        offset.min(self.max_offset)
    }
}

/// The thumb's row span within `area` (`(start, len)`, both relative to
/// `area.top()`) for a vertical scrollbar covering `total_len` items in a
/// `visible_len`-row viewport currently starting at `offset`.
///
/// `None` if there's nothing to scroll (`area` has no rows, `visible_len`
/// is zero, or `total_len <= visible_len`, the whole track already fits
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

    let len = thumb_len(track, total_len, visible_len);
    let max_offset = total_len - visible_len; // > 0, since total_len > visible_len here
    let max_start = track.saturating_sub(len); // last row the thumb can start on
    let map = TrackMap {
        max_offset,
        max_start,
    };

    Some((map.row_for(offset), len))
}

/// The offset a vertical scrollbar should jump to for a click/drag at `pos`.
///
/// Covers `total_len` items in a `visible_len`-row `area`; useful for
/// click-to-jump or drag-to-scroll interactions built on top of
/// [`thumb_geometry`], whose exact inverse it is (both share the same private row/offset
/// conversion internally). `None` if `pos` falls outside `area`, or (mirroring
/// [`thumb_geometry`]) there's nothing to scroll.
#[must_use]
pub fn offset_for_pos(area: Rect, total_len: usize, visible_len: usize, pos: Pos) -> Option<usize> {
    let track = area.height();
    if !area.contains_pos(pos) || track == 0 || visible_len == 0 || total_len <= visible_len {
        return None;
    }

    // `area.contains_pos(pos)` above already guarantees `pos.y >= area.top()`; `checked_sub`
    // (rather than `saturating_sub`) makes that the one place the absolute-`Pos`-to-local-row
    // conversion happens, instead of leaving it implicit.
    let row = pos.y.checked_sub(area.top())?;

    let len = thumb_len(track, total_len, visible_len);
    let max_offset = total_len - visible_len;
    let max_start = track.saturating_sub(len);
    let map = TrackMap {
        max_offset,
        max_start,
    };

    Some(map.offset_for(row))
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

    #[test]
    fn offset_for_pos_mirrors_thumb_geometry_for_a_zero_height_viewport() {
        let area = Rect::new(0, 0, 1, 10);
        assert_eq!(thumb_geometry(area, 40, 0, 0), None); // visible_len == 0: nothing to scroll
        assert_eq!(offset_for_pos(area, 40, 0, Pos::new(0, 5)), None);
    }

    /// Any row at or below the thumb's own start (`max_start`, here `15`: `track=20` minus
    /// `len=5`) now maps to `max_offset`, not just the very bottom row: `offset_for_pos` clamps
    /// to the same `max_start` [`thumb_geometry`] uses, so anywhere the thumb's body can visibly
    /// sit already reads as "scrolled all the way". Before [`TrackMap`], `offset_for_pos` instead
    /// divided by `track - 1` on its own, so a click a few rows above the very bottom (still
    /// within the thumb, once scrolled all the way) landed short of `max_offset`, a mismatch a
    /// user could see the thumb visibly slide to correct on release.
    #[test]
    fn offset_for_pos_clamps_to_max_offset_for_any_row_the_thumb_can_start_on() {
        let area = Rect::new(0, 0, 1, 20);
        let (max_start, len) = thumb_geometry(area, 40, 10, 30).unwrap(); // max_offset = 30
        assert_eq!((max_start, len), (15, 5));

        for row in max_start..area.height() {
            assert_eq!(
                offset_for_pos(area, 40, 10, Pos::new(0, area.top() + row)),
                Some(30),
                "row {row}"
            );
        }
    }

    /// The bug this module was named after (retroglyph#761): `thumb_geometry` and
    /// `offset_for_pos` independently rounded through different denominators (`track - len` vs.
    /// `track - 1`), so dragging the thumb to a spot and re-deriving an offset from where it
    /// visually landed could jump the content by several rows' worth at once (the issue's repro,
    /// these same `area`/`total_len`/`visible_len`, jumped by 6). With both functions now sharing
    /// one [`TrackMap`], the round trip can still be off (rounding a wide `offset` range down into
    /// a narrow `row` range is inherently lossy, see [`TrackMap`]'s docs), but never by more than
    /// one row's worth of offset.
    #[test]
    fn offset_for_pos_round_trips_thumb_geometry_within_one_row() {
        let area = Rect::new(0, 0, 1, 20);
        let (total, visible) = (40, 10);
        let max_offset = total - visible;

        for offset in 0..=max_offset {
            let (start, _len) = thumb_geometry(area, total, visible, offset).unwrap();
            let back =
                offset_for_pos(area, total, visible, Pos::new(0, area.top() + start)).unwrap();
            let diff = back.abs_diff(offset);
            assert!(
                diff <= 1,
                "offset {offset} -> row {start} -> {back} (diff {diff})"
            );
        }
    }

    /// [`TrackMap`]'s exact-inverse guarantee: `row -> offset -> row` recovers the original row
    /// whenever there are at least as many offsets as rows (`max_start <= max_offset`), which is
    /// the case for any scrollbar with more scrollable items than screen rows. Swept across a
    /// spread of track heights and total/visible lengths, not just one `area`, since the claim is
    /// about the map's construction, not one particular size.
    #[test]
    fn track_map_row_then_offset_round_trips_exactly() {
        for height in [1u16, 2, 3, 5, 7, 10, 20, 50] {
            for total_len in [2usize, 5, 10, 40, 100, 1000] {
                for visible_len in [1usize, 2, 5, 10, 50] {
                    if total_len <= visible_len {
                        continue;
                    }
                    let area = Rect::new(0, 0, 1, height);
                    let Some((_, len)) = thumb_geometry(area, total_len, visible_len, 0) else {
                        continue;
                    };
                    let max_start = height.saturating_sub(len);
                    let max_offset = total_len - visible_len;
                    if usize::from(max_start) > max_offset {
                        continue; // the lossy direction here instead; see the type's docs.
                    }

                    for row in 0..=max_start {
                        let offset = offset_for_pos(
                            area,
                            total_len,
                            visible_len,
                            Pos::new(0, area.top() + row),
                        )
                        .unwrap();
                        let (back_row, _) =
                            thumb_geometry(area, total_len, visible_len, offset).unwrap();
                        assert_eq!(
                            back_row, row,
                            "height={height} total_len={total_len} visible_len={visible_len} row={row}"
                        );
                    }
                }
            }
        }
    }
}
