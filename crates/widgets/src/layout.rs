//! Constraint-based `Rect` splitter for multi-panel UIs.
//!
//! Splits a [`Rect`](retroglyph_core::Rect) into stacked rows ([`split_v`]) or
//! side-by-side columns ([`split_h`]) according to a slice of [`Constraint`]s.
//!
//! The solver sums the [`Fixed`](Constraint::Fixed) and [`Percent`](Constraint::Percent)
//! amounts, then distributes whatever remains equally across the
//! [`Fill`](Constraint::Fill), [`Min`](Constraint::Min), and [`Max`](Constraint::Max)
//! panes. Sizes are clamped so the panes never spill past `area`. This is a single
//! sequential pass, not an iterative constraint solver: a [`Max`](Constraint::Max)
//! pane that is capped below its equal share does not redistribute the excess to
//! other panes, so leftover space can remain unclaimed (see [`Flex`] for how that
//! leftover is placed via [`split_v_flex`]/[`split_h_flex`]).
use retroglyph_core::Rect;

/// How a single pane claims space along the split axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Constraint {
    /// An exact number of cells.
    Fixed(u16),
    /// A percentage (0–100) of the axis length.
    Percent(u16),
    /// Claim an equal share of whatever space the fixed/percent panes leave.
    Fill,
    /// Like [`Fill`](Self::Fill), but guarantees at least this many cells
    /// even if the axis is too small for every pane to get an equal share.
    Min(u16),
    /// Like [`Fill`](Self::Fill), but never grows past this many cells; any
    /// share past the cap is left unclaimed rather than redistributed.
    Max(u16),
}

impl Constraint {
    /// Resolve this constraint's base size against `total` axis length.
    /// [`Fill`](Self::Fill) and [`Max`](Self::Max) resolve to zero here;
    /// [`Min`](Self::Min) reserves its floor up front like [`Fixed`](Self::Fixed).
    /// Flexible sizes are filled in later by [`solve`].
    fn base(self, total: u16) -> u16 {
        match self {
            Self::Fixed(n) | Self::Min(n) => n.min(total),
            Self::Percent(p) => {
                let p = u32::from(p.min(100));
                #[allow(clippy::cast_possible_truncation)]
                {
                    (u32::from(total) * p / 100) as u16
                }
            }
            Self::Fill | Self::Max(_) => 0,
        }
    }
}

/// Compute the length of each pane along an axis of `total` cells.
fn solve(total: u16, constraints: &[Constraint]) -> Vec<u16> {
    let mut sizes: Vec<u16> = constraints.iter().map(|c| c.base(total)).collect();

    // Clamp the fixed/percent sum so it never exceeds the axis. If it does,
    // shave from the tail so earlier panes keep their requested size.
    let mut used: u16 = 0;
    for size in &mut sizes {
        let room = total.saturating_sub(used);
        *size = (*size).min(room);
        used += *size;
    }

    // Distribute the remainder equally across the Fill, Min, and Max panes.
    // Min panes add their share on top of the floor already reserved above;
    // Max panes start at zero and are capped at their declared value (any
    // share past the cap is simply left unclaimed, not redistributed).
    let flexible: Vec<(usize, Option<u16>)> = constraints
        .iter()
        .enumerate()
        .filter_map(|(i, c)| match c {
            Constraint::Fill | Constraint::Min(_) => Some((i, None)),
            Constraint::Max(cap) => Some((i, Some(*cap))),
            Constraint::Fixed(_) | Constraint::Percent(_) => None,
        })
        .collect();
    if !flexible.is_empty() {
        let remainder = total.saturating_sub(used);
        #[allow(clippy::cast_possible_truncation)]
        let each = remainder / flexible.len() as u16;
        #[allow(clippy::cast_possible_truncation)]
        let mut extra = remainder % flexible.len() as u16;
        for &(i, cap) in &flexible {
            let share = each + u16::from(extra > 0);
            extra = extra.saturating_sub(1);
            let grown = sizes[i].saturating_add(share);
            sizes[i] = cap.map_or(grown, |max| grown.min(max));
        }
    }

    sizes
}

/// Split `area` into stacked rows top-to-bottom.
///
/// Returns one [`Rect`] per constraint; empty panes (zero height) are still
/// returned so indices line up with `constraints`.
///
/// # Examples
///
/// ```
/// use retroglyph_core::Rect;
/// use retroglyph_widgets::{Constraint, split_v};
///
/// let area = Rect::new(0, 0, 20, 10);
/// let panes = split_v(area, &[Constraint::Fixed(1), Constraint::Fill, Constraint::Fixed(1)]);
/// assert_eq!(panes.iter().map(Rect::height).collect::<Vec<_>>(), vec![1, 8, 1]);
/// ```
#[must_use]
pub fn split_v(area: Rect, constraints: &[Constraint]) -> Vec<Rect> {
    let sizes = solve(area.height(), constraints);
    let mut y = area.top();
    sizes
        .into_iter()
        .map(|h| {
            let rect = Rect::new(area.left(), y, area.width(), h);
            y = y.saturating_add(h);
            rect
        })
        .collect()
}

/// Split `area` into columns left-to-right.
///
/// Returns one [`Rect`] per constraint; empty panes (zero width) are still
/// returned so indices line up with `constraints`.
///
/// # Examples
///
/// ```
/// use retroglyph_core::Rect;
/// use retroglyph_widgets::{Constraint, split_h};
///
/// let area = Rect::new(0, 0, 100, 5);
/// let panes = split_h(area, &[Constraint::Percent(30), Constraint::Fill]);
/// assert_eq!(panes.iter().map(Rect::width).collect::<Vec<_>>(), vec![30, 70]);
/// ```
#[must_use]
pub fn split_h(area: Rect, constraints: &[Constraint]) -> Vec<Rect> {
    let sizes = solve(area.width(), constraints);
    let mut x = area.left();
    sizes
        .into_iter()
        .map(|w| {
            let rect = Rect::new(x, area.top(), w, area.height());
            x = x.saturating_add(w);
            rect
        })
        .collect()
}

/// How leftover space is placed along the split axis, once [`Constraint`]s
/// are resolved.
///
/// Only matters when the resolved pane sizes sum to less than `area`'s
/// length; passed to [`split_v_flex`]/[`split_h_flex`].
///
/// [`split_v`]/[`split_h`] always behave like [`Start`](Self::Start): any
/// leftover space trails after the last pane, unclaimed. This matches their
/// existing documented behavior, so adding `Flex` does not change them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Flex {
    /// Panes are packed at the start of the area; leftover space trails
    /// after the last pane. The default, and what [`split_v`]/[`split_h`] use.
    #[default]
    Start,
    /// Panes are packed at the end of the area; leftover space leads before
    /// the first pane.
    End,
    /// Leftover space is split evenly before and after the panes.
    Center,
    /// Leftover space is distributed as gaps between panes (none before the
    /// first or after the last). No-op with fewer than two panes.
    SpaceBetween,
    /// Leftover space is distributed as equal-width gaps around every pane,
    /// including before the first and after the last.
    SpaceAround,
}

/// Compute each pane's starting offset along an axis of `total` cells for
/// the resolved `sizes`, per `flex`. Companion to [`solve`]; used by
/// [`split_v_flex`]/[`split_h_flex`].
fn place(total: u16, sizes: &[u16], flex: Flex) -> Vec<u16> {
    let content: u16 = sizes.iter().fold(0u16, |a, &b| a.saturating_add(b));
    let slack = total.saturating_sub(content);
    let n = sizes.len();
    let mut offsets = Vec::with_capacity(n);

    let packed_from = |start: u16| {
        let mut pos = start;
        sizes
            .iter()
            .map(|&s| {
                let at = pos;
                pos = pos.saturating_add(s);
                at
            })
            .collect::<Vec<u16>>()
    };

    match flex {
        Flex::End => offsets = packed_from(slack),
        Flex::Center => offsets = packed_from(slack / 2),
        Flex::SpaceBetween if n > 1 => {
            #[allow(clippy::cast_possible_truncation)]
            let gaps = n as u16 - 1;
            let gap = slack / gaps;
            let mut extra = slack % gaps;
            let mut pos = 0;
            for (i, &s) in sizes.iter().enumerate() {
                offsets.push(pos);
                pos = pos.saturating_add(s);
                if i + 1 < n {
                    pos = pos.saturating_add(gap + u16::from(extra > 0));
                    extra = extra.saturating_sub(1);
                }
            }
        }
        Flex::Start | Flex::SpaceBetween => offsets = packed_from(0),
        Flex::SpaceAround => {
            #[allow(clippy::cast_possible_truncation)]
            let gaps = n as u16 + 1;
            let unit = slack / gaps;
            let mut extra = slack % gaps;
            let mut pos = unit + u16::from(extra > 0);
            extra = extra.saturating_sub(u16::from(extra > 0));
            for &s in sizes {
                offsets.push(pos);
                pos = pos.saturating_add(s);
                pos = pos.saturating_add(unit + u16::from(extra > 0));
                extra = extra.saturating_sub(u16::from(extra > 0));
            }
        }
    }

    offsets
}

/// Split `area` into stacked rows top-to-bottom, like [`split_v`], but with
/// explicit control over how leftover space is placed via [`Flex`].
#[must_use]
pub fn split_v_flex(area: Rect, constraints: &[Constraint], flex: Flex) -> Vec<Rect> {
    let sizes = solve(area.height(), constraints);
    let offsets = place(area.height(), &sizes, flex);
    offsets
        .into_iter()
        .zip(sizes)
        .map(|(y, h)| Rect::new(area.left(), area.top().saturating_add(y), area.width(), h))
        .collect()
}

/// Split `area` into columns left-to-right, like [`split_h`], but with
/// explicit control over how leftover space is placed via [`Flex`].
#[must_use]
pub fn split_h_flex(area: Rect, constraints: &[Constraint], flex: Flex) -> Vec<Rect> {
    let sizes = solve(area.width(), constraints);
    let offsets = place(area.width(), &sizes, flex);
    offsets
        .into_iter()
        .zip(sizes)
        .map(|(x, w)| Rect::new(area.left().saturating_add(x), area.top(), w, area.height()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertical_split_sums_and_clamps() {
        let area = Rect::new(0, 0, 20, 10);
        let panes = split_v(
            area,
            &[Constraint::Fixed(1), Constraint::Fill, Constraint::Fixed(1)],
        );
        assert_eq!(panes.len(), 3);
        // Heights: 1 + 8 + 1 = 10, exactly filling the area.
        assert_eq!(panes[0].height(), 1);
        assert_eq!(panes[1].height(), 8);
        assert_eq!(panes[2].height(), 1);
        // Panes are contiguous and never exceed the area bottom.
        assert_eq!(panes[0].top(), 0);
        assert_eq!(panes[1].top(), 1);
        assert_eq!(panes[2].top(), 9);
        assert_eq!(panes[2].bottom(), area.bottom());
        // Width is preserved across all panes.
        for p in &panes {
            assert_eq!(p.width(), 20);
        }
    }

    #[test]
    fn horizontal_percent_and_fill() {
        let area = Rect::new(0, 0, 100, 5);
        let panes = split_h(area, &[Constraint::Percent(30), Constraint::Fill]);
        assert_eq!(panes[0].width(), 30);
        assert_eq!(panes[1].width(), 70);
        assert_eq!(panes[0].left(), 0);
        assert_eq!(panes[1].left(), 30);
        assert_eq!(panes[1].right(), area.right());
    }

    #[test]
    fn fill_remainder_distributes_evenly() {
        let area = Rect::new(0, 0, 10, 1);
        // 10 cells across 3 fills: 4, 3, 3 (leftover goes to the front).
        let panes = split_h(
            area,
            &[Constraint::Fill, Constraint::Fill, Constraint::Fill],
        );
        let widths: Vec<u16> = panes.iter().map(Rect::width).collect();
        assert_eq!(widths, vec![4, 3, 3]);
        assert_eq!(widths.iter().sum::<u16>(), 10);
    }

    #[test]
    fn oversized_fixed_is_clamped() {
        let area = Rect::new(0, 0, 5, 3);
        // Requested 10 + 10 but only 5 columns exist: first takes all, rest zero.
        let panes = split_h(area, &[Constraint::Fixed(10), Constraint::Fixed(10)]);
        assert_eq!(panes[0].width(), 5);
        assert_eq!(panes[1].width(), 0);
        // No pane extends past the area.
        for p in &panes {
            assert!(p.right() <= area.right());
        }
    }

    #[test]
    fn no_fill_leaves_gap() {
        let area = Rect::new(0, 0, 10, 4);
        let panes = split_v(area, &[Constraint::Fixed(2), Constraint::Fixed(2)]);
        // Only 4 of 10 rows consumed; that is fine — panes still fit.
        assert_eq!(panes[0].height(), 2);
        assert_eq!(panes[1].height(), 2);
        assert_eq!(panes[1].bottom(), 4);
    }

    #[test]
    fn min_gets_at_least_its_floor_plus_a_share() {
        let area = Rect::new(0, 0, 10, 1);
        // Min(3) and Fill both get an equal share (5 each) of the full 10
        // cells, since Min's floor is reserved up front and then also
        // shares in distributing the remaining 7: Min ends up with
        // 3 (floor) + 4 (share, rounded up) = 7, Fill gets the other 3.
        let panes = split_h(area, &[Constraint::Min(3), Constraint::Fill]);
        let widths: Vec<u16> = panes.iter().map(Rect::width).collect();
        assert_eq!(widths, vec![7, 3]);
        assert_eq!(widths.iter().sum::<u16>(), 10);
    }

    #[test]
    fn min_floor_holds_when_share_would_be_smaller() {
        let area = Rect::new(0, 0, 10, 1);
        // Three flexible panes would each get ~3, but Min(4) guarantees 4:
        // its floor (4) plus an equal share of the remaining 6 across all
        // three (2 each) gives Min(4) a total of 6, leaving 2 each for the
        // two Fill panes.
        let panes = split_h(
            area,
            &[Constraint::Min(4), Constraint::Fill, Constraint::Fill],
        );
        let widths: Vec<u16> = panes.iter().map(Rect::width).collect();
        assert_eq!(widths[0], 6);
        assert_eq!(widths[1], 2);
        assert_eq!(widths[2], 2);
        assert_eq!(widths.iter().sum::<u16>(), 10);
    }

    #[test]
    fn max_caps_its_share_and_leaves_the_rest_unclaimed() {
        let area = Rect::new(0, 0, 10, 1);
        // Fill and Max(2) would each get 5; Max(2) is capped, and its extra
        // 3 cells are left unclaimed (no redistribution), not given to Fill.
        let panes = split_h(area, &[Constraint::Fill, Constraint::Max(2)]);
        let widths: Vec<u16> = panes.iter().map(Rect::width).collect();
        assert_eq!(widths, vec![5, 2]);
        assert_eq!(widths.iter().sum::<u16>(), 7);
    }

    #[test]
    fn flex_start_matches_split_v() {
        let area = Rect::new(0, 0, 10, 4);
        let constraints = [Constraint::Fixed(2), Constraint::Fixed(2)];
        let legacy = split_v(area, &constraints);
        let flexed = split_v_flex(area, &constraints, Flex::Start);
        assert_eq!(legacy, flexed);
    }

    #[test]
    fn flex_end_pushes_leftover_before_the_panes() {
        let area = Rect::new(0, 0, 10, 10);
        let panes = split_v_flex(
            area,
            &[Constraint::Fixed(2), Constraint::Fixed(2)],
            Flex::End,
        );
        // 6 rows of slack lead before the first pane.
        assert_eq!(panes[0].top(), 6);
        assert_eq!(panes[1].top(), 8);
        assert_eq!(panes[1].bottom(), 10);
    }

    #[test]
    fn flex_center_splits_leftover_around_the_panes() {
        let area = Rect::new(0, 0, 10, 10);
        let panes = split_v_flex(area, &[Constraint::Fixed(4)], Flex::Center);
        // 6 rows of slack, 3 leading before the single pane.
        assert_eq!(panes[0].top(), 3);
        assert_eq!(panes[0].bottom(), 7);
    }

    #[test]
    fn flex_space_between_puts_leftover_between_panes_only() {
        let area = Rect::new(0, 0, 10, 1);
        let panes = split_h_flex(
            area,
            &[Constraint::Fixed(2), Constraint::Fixed(2)],
            Flex::SpaceBetween,
        );
        // 6 cells of slack become a single gap between the two panes.
        assert_eq!(panes[0].left(), 0);
        assert_eq!(panes[0].right(), 2);
        assert_eq!(panes[1].left(), 8);
        assert_eq!(panes[1].right(), 10);
    }

    #[test]
    fn flex_space_around_puts_equal_gaps_at_both_edges() {
        let area = Rect::new(0, 0, 9, 1);
        let panes = split_h_flex(area, &[Constraint::Fixed(3)], Flex::SpaceAround);
        // 6 cells of slack split into 2 gaps (before and after) of 3 each.
        assert_eq!(panes[0].left(), 3);
        assert_eq!(panes[0].right(), 6);
    }
}
