//! Constraint-based `Rect` splitter for multi-panel UIs.
//!
//! Splits a [`Rect`] into stacked rows ([`split_v`]) or side-by-side columns
//! ([`split_h`]) according to a slice of [`Constraint`]s. [`split_h_spaced`]/[`split_v_spaced`]
//! do the same but also carve a fixed-cell gap between every adjacent pair of panes, without the
//! caller having to interleave `Constraint::Fixed(spacing)` gap constraints and filter them back
//! out by hand.
//!
//! The solver sums the [`Fixed`](Constraint::Fixed) and [`Percent`](Constraint::Percent)
//! amounts, then distributes whatever remains across the [`Fill`](Constraint::Fill),
//! [`Min`](Constraint::Min), and [`Max`](Constraint::Max) panes in proportion to their
//! weight: a `Fill(w)` pane claims a share proportional to `w` relative
//! to the other flexible panes, while [`Min`](Constraint::Min) and [`Max`](Constraint::Max)
//! panes always weigh 1. `Fill(1)` (equivalent to every pane weighing 1) reproduces plain
//! equal distribution. Sizes are clamped so the panes never spill past `area`. This is a
//! single sequential pass, not an iterative constraint solver: a [`Max`](Constraint::Max)
//! pane that is capped below its share does not redistribute the excess to other panes, so
//! leftover space can remain unclaimed (see [`Flex`] for how that leftover is placed via
//! [`split_v_flex`]/[`split_h_flex`]).
use retroglyph_core::Rect;

/// How a single pane claims space along the split axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Constraint {
    /// An exact number of cells.
    Fixed(u16),
    /// A percentage (0–100) of the axis length.
    Percent(u16),
    /// Claim a share of whatever space the fixed/percent panes leave, proportional to
    /// `weight` relative to the other [`Fill`](Self::Fill)/[`Min`](Self::Min)/[`Max`](Self::Max)
    /// panes in the same split ([`Min`](Self::Min)/[`Max`](Self::Max) panes always weigh 1).
    /// `Fill(1)` reproduces plain equal distribution across an all-`Fill` split; a weight of
    /// 0 claims no share of the remainder.
    Fill(u16),
    /// Like [`Fill`](Self::Fill), but guarantees at least this many cells even if the axis
    /// is too small for every pane to get its share, and always weighs 1.
    Min(u16),
    /// Like [`Fill`](Self::Fill), but never grows past this many cells (any share past the
    /// cap is left unclaimed rather than redistributed), and always weighs 1.
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
            Self::Fill(_) | Self::Max(_) => 0,
        }
    }
}

/// Constraint counts at or below this stay on the stack in [`SmallBuf`]; larger splits fall back
/// to a heap `Vec`. Chosen comfortably above a typical multi-panel layout (a header, a handful of
/// flexible content panes, a status bar) while staying correct for arbitrarily many panes -- see
/// the `layout_solve` benchmark's 100-pane case, which exercises the heap fallback.
const STACK_CAP: usize = 8;

/// A small buffer that stays inline on the stack for up to `N` items and only allocates on the
/// heap past that. `solve` uses this for its scratch buffers (pane sizes, the flexible-pane
/// index/weight/cap list, and the largest-remainder distribution pass) so that the common case of
/// a handful of panes per split -- called several times per frame by multi-panel UIs -- does not
/// pay for a heap allocation at all.
enum SmallBuf<T: Copy + Default, const N: usize> {
    Stack([T; N], usize),
    Heap(Vec<T>),
}

impl<T: Copy + Default, const N: usize> SmallBuf<T, N> {
    /// Create a buffer able to hold `cap` items without reallocating: inline on the stack if
    /// `cap` fits within `N`, otherwise a heap `Vec` pre-sized to `cap`.
    fn with_capacity(cap: usize) -> Self {
        if cap <= N {
            Self::Stack([T::default(); N], 0)
        } else {
            Self::Heap(Vec::with_capacity(cap))
        }
    }

    /// Append `value`.
    ///
    /// # Panics
    ///
    /// Panics if the buffer is the `Stack` variant and already holds `N` items -- callers must
    /// size `with_capacity` to the true upper bound of pushes, as `solve` does.
    fn push(&mut self, value: T) {
        match self {
            Self::Stack(buf, len) => {
                buf[*len] = value;
                *len += 1;
            }
            Self::Heap(vec) => vec.push(value),
        }
    }
}

impl<T: Copy + Default, const N: usize> std::ops::Deref for SmallBuf<T, N> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        match self {
            Self::Stack(buf, len) => &buf[..*len],
            Self::Heap(vec) => vec,
        }
    }
}

impl<T: Copy + Default, const N: usize> std::ops::DerefMut for SmallBuf<T, N> {
    fn deref_mut(&mut self) -> &mut [T] {
        match self {
            Self::Stack(buf, len) => &mut buf[..*len],
            Self::Heap(vec) => vec,
        }
    }
}

impl<T: Copy + Default, const N: usize> std::ops::Index<usize> for SmallBuf<T, N> {
    type Output = T;

    fn index(&self, idx: usize) -> &T {
        &(**self)[idx]
    }
}

impl<T: Copy + Default, const N: usize> std::ops::IndexMut<usize> for SmallBuf<T, N> {
    fn index_mut(&mut self, idx: usize) -> &mut T {
        &mut (**self)[idx]
    }
}

/// Compute the length of each pane along an axis of `total` cells.
fn solve(total: u16, constraints: &[Constraint]) -> SmallBuf<u16, STACK_CAP> {
    let mut sizes: SmallBuf<u16, STACK_CAP> = SmallBuf::with_capacity(constraints.len());
    for c in constraints {
        sizes.push(c.base(total));
    }

    // Clamp the fixed/percent sum so it never exceeds the axis. If it does,
    // shave from the tail so earlier panes keep their requested size.
    let mut used: u16 = 0;
    for size in sizes.iter_mut() {
        let room = total.saturating_sub(used);
        *size = (*size).min(room);
        used += *size;
    }

    // Distribute the remainder across the Fill, Min, and Max panes in proportion to
    // their weight (Fill(w) weighs w; Min/Max always weigh 1). Min panes add their
    // share on top of the floor already reserved above; Max panes start at zero and
    // are capped at their declared value (any share past the cap is simply left
    // unclaimed, not redistributed).
    let mut flexible: SmallBuf<(usize, u16, Option<u16>), STACK_CAP> =
        SmallBuf::with_capacity(constraints.len());
    for (i, c) in constraints.iter().enumerate() {
        match c {
            Constraint::Fill(weight) => flexible.push((i, *weight, None)),
            Constraint::Min(_) => flexible.push((i, 1, None)),
            Constraint::Max(cap) => flexible.push((i, 1, Some(*cap))),
            Constraint::Fixed(_) | Constraint::Percent(_) => {}
        }
    }
    if !flexible.is_empty() {
        let remainder = total.saturating_sub(used);
        let total_weight: u32 = flexible.iter().map(|&(_, w, _)| u32::from(w)).sum();
        if let Some(total_weight) = std::num::NonZeroU32::new(total_weight) {
            // Largest-remainder method: give every pane the integer floor of its
            // proportional share, then hand out the leftover cells one at a time to
            // the panes with the largest fractional remainder (ties -> earlier pane
            // first). For equal weights every fraction ties, so this reduces to the
            // original round-robin-from-the-front behavior exactly.
            let mut shares: SmallBuf<u32, STACK_CAP> = SmallBuf::with_capacity(flexible.len());
            let mut fracs: SmallBuf<u32, STACK_CAP> = SmallBuf::with_capacity(flexible.len());
            let mut floor_sum: u32 = 0;
            for &(_, weight, _) in flexible.iter() {
                let product = u32::from(remainder) * u32::from(weight);
                let share = product / total_weight;
                fracs.push(product % total_weight);
                shares.push(share);
                floor_sum += share;
            }
            let mut leftover = u32::from(remainder).saturating_sub(floor_sum);
            let mut order: SmallBuf<usize, STACK_CAP> = SmallBuf::with_capacity(flexible.len());
            for idx in 0..flexible.len() {
                order.push(idx);
            }
            order.sort_by(|&a, &b| fracs[b].cmp(&fracs[a]).then(a.cmp(&b)));
            for &idx in order.iter() {
                if leftover == 0 {
                    break;
                }
                shares[idx] += 1;
                leftover -= 1;
            }
            for (k, &(i, _, cap)) in flexible.iter().enumerate() {
                #[allow(clippy::cast_possible_truncation)]
                let share = shares[k] as u16;
                let grown = sizes[i].saturating_add(share);
                sizes[i] = cap.map_or(grown, |max| grown.min(max));
            }
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
/// let panes = split_v(area, &[Constraint::Fixed(1), Constraint::Fill(1), Constraint::Fixed(1)]);
/// assert_eq!(panes.iter().map(Rect::height).collect::<Vec<_>>(), vec![1, 8, 1]);
/// ```
#[must_use]
pub fn split_v(area: Rect, constraints: &[Constraint]) -> Vec<Rect> {
    let sizes = solve(area.height(), constraints);
    let mut y = area.top();
    sizes
        .iter()
        .copied()
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
/// let panes = split_h(area, &[Constraint::Percent(30), Constraint::Fill(1)]);
/// assert_eq!(panes.iter().map(Rect::width).collect::<Vec<_>>(), vec![30, 70]);
/// ```
#[must_use]
pub fn split_h(area: Rect, constraints: &[Constraint]) -> Vec<Rect> {
    let sizes = solve(area.width(), constraints);
    let mut x = area.left();
    sizes
        .iter()
        .copied()
        .map(|w| {
            let rect = Rect::new(x, area.top(), w, area.height());
            x = x.saturating_add(w);
            rect
        })
        .collect()
}

/// Interleaves a `Constraint::Fixed(spacing)` gap between every pair of adjacent `constraints`.
///
/// `[c0, c1, c2]` with `spacing` becomes `[c0, Fixed(spacing), c1, Fixed(spacing), c2]` -- the
/// same shape a caller would otherwise have to build (and then remember to filter back out) by
/// hand. No-op with fewer than two constraints.
fn interleave_gaps(constraints: &[Constraint], spacing: u16) -> Vec<Constraint> {
    let mut out = Vec::with_capacity(constraints.len().saturating_mul(2).saturating_sub(1));
    for (i, &c) in constraints.iter().enumerate() {
        if i > 0 {
            out.push(Constraint::Fixed(spacing));
        }
        out.push(c);
    }
    out
}

/// Split `area` into columns left-to-right, like [`split_h`], but with a fixed `spacing`-cell gap
/// carved out between every adjacent pair of panes.
///
/// Equivalent to interleaving `Constraint::Fixed(spacing)` between `constraints` and calling
/// [`split_h`], then discarding the gap panes -- but the caller only ever sees the content panes,
/// with no gap indices to filter out themselves. `spacing` gaps come out of `area` before
/// `constraints` are resolved, so [`Fill`](Constraint::Fill)/[`Percent`](Constraint::Percent) panes
/// share only what's left after every gap is reserved. No-op (falls back to [`split_h`]) with
/// fewer than two panes or zero spacing.
///
/// # Examples
///
/// ```
/// use retroglyph_core::Rect;
/// use retroglyph_widgets::{Constraint, split_h_spaced};
///
/// let area = Rect::new(0, 0, 59, 6);
/// let panes = split_h_spaced(area, &[Constraint::Fill(1); 3], 1);
/// assert_eq!(panes.iter().map(Rect::width).collect::<Vec<_>>(), vec![19, 19, 19]);
/// assert_eq!(panes[1].left(), panes[0].right() + 1); // one gap cell between panes
/// ```
#[must_use]
pub fn split_h_spaced(area: Rect, constraints: &[Constraint], spacing: u16) -> Vec<Rect> {
    if spacing == 0 || constraints.len() < 2 {
        return split_h(area, constraints);
    }
    split_h(area, &interleave_gaps(constraints, spacing))
        .into_iter()
        .step_by(2)
        .collect()
}

/// Split `area` into stacked rows top-to-bottom, like [`split_v`], but with a fixed `spacing`-cell
/// gap carved out between every adjacent pair of panes.
///
/// See [`split_h_spaced`] for the full behavior; this is the same operation along the vertical
/// axis.
#[must_use]
pub fn split_v_spaced(area: Rect, constraints: &[Constraint], spacing: u16) -> Vec<Rect> {
    if spacing == 0 || constraints.len() < 2 {
        return split_v(area, constraints);
    }
    split_v(area, &interleave_gaps(constraints, spacing))
        .into_iter()
        .step_by(2)
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
        .zip(sizes.iter().copied())
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
        .zip(sizes.iter().copied())
        .map(|(x, w)| Rect::new(area.left().saturating_add(x), area.top(), w, area.height()))
        .collect()
}

/// Compute a `width`×`height` [`Rect`] centered within `screen`.
///
/// `width`/`height` are clamped down to `screen`'s own dimensions if larger,
/// so the result never extends past `screen`'s edges -- a modal, dialog, or
/// tooltip box built from this is always fully on-screen, even on a
/// terminal too small to fit the box's requested size. Pure layout math: no
/// drawing, no `Terminal`. Pairs with `panel`/`modal` in `retroglyph-widgets`
/// (the `draw` module) for a centered, bordered box.
#[must_use]
pub fn centered_rect(screen: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(screen.width());
    let height = height.min(screen.height());
    let x = screen.left() + (screen.width() - width) / 2;
    let y = screen.top() + (screen.height() - height) / 2;
    Rect::new(x, y, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertical_split_sums_and_clamps() {
        let area = Rect::new(0, 0, 20, 10);
        let panes = split_v(
            area,
            &[
                Constraint::Fixed(1),
                Constraint::Fill(1),
                Constraint::Fixed(1),
            ],
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
        let panes = split_h(area, &[Constraint::Percent(30), Constraint::Fill(1)]);
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
            &[
                Constraint::Fill(1),
                Constraint::Fill(1),
                Constraint::Fill(1),
            ],
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
        let panes = split_h(area, &[Constraint::Min(3), Constraint::Fill(1)]);
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
            &[Constraint::Min(4), Constraint::Fill(1), Constraint::Fill(1)],
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
        let panes = split_h(area, &[Constraint::Fill(1), Constraint::Max(2)]);
        let widths: Vec<u16> = panes.iter().map(Rect::width).collect();
        assert_eq!(widths, vec![5, 2]);
        assert_eq!(widths.iter().sum::<u16>(), 7);
    }

    #[test]
    fn weighted_fill_splits_proportionally() {
        let area = Rect::new(0, 0, 12, 1);
        // Fill(2) claims twice the share of Fill(1): 4 and 8 of 12.
        let panes = split_h(area, &[Constraint::Fill(1), Constraint::Fill(2)]);
        let widths: Vec<u16> = panes.iter().map(Rect::width).collect();
        assert_eq!(widths, vec![4, 8]);
        assert_eq!(widths.iter().sum::<u16>(), 12);
    }

    #[test]
    fn weighted_fill_at_weight_one_matches_equal_distribution() {
        let area = Rect::new(0, 0, 10, 1);
        // Every pane weighing the same value (not just 1) still divides
        // evenly, since distribution is by weight *ratio*, not magnitude.
        let panes = split_h(
            area,
            &[
                Constraint::Fill(5),
                Constraint::Fill(5),
                Constraint::Fill(5),
            ],
        );
        let widths: Vec<u16> = panes.iter().map(Rect::width).collect();
        assert_eq!(widths, vec![4, 3, 3]);
        assert_eq!(widths.iter().sum::<u16>(), 10);
    }

    #[test]
    fn weighted_fill_leftover_goes_to_the_largest_fractional_share() {
        let area = Rect::new(0, 0, 10, 1);
        // Ideal shares are 30/7 ~= 4.29, 20/7 ~= 2.86, 20/7 ~= 2.86. Floors are
        // 4, 2, 2 (sum 8); the 2 leftover cells go to the panes with the
        // largest fractional remainder, in this case the two Fill(2)s tied
        // ahead of Fill(3) -- not to the first pane in the slice.
        let panes = split_h(
            area,
            &[
                Constraint::Fill(3),
                Constraint::Fill(2),
                Constraint::Fill(2),
            ],
        );
        let widths: Vec<u16> = panes.iter().map(Rect::width).collect();
        assert_eq!(widths, vec![4, 3, 3]);
        assert_eq!(widths.iter().sum::<u16>(), 10);
    }

    #[test]
    fn fill_weight_zero_claims_no_share_of_the_remainder() {
        let area = Rect::new(0, 0, 10, 1);
        let panes = split_h(area, &[Constraint::Fill(0), Constraint::Fill(1)]);
        let widths: Vec<u16> = panes.iter().map(Rect::width).collect();
        assert_eq!(widths, vec![0, 10]);
    }

    #[test]
    fn all_fill_weights_zero_leaves_the_remainder_unclaimed() {
        let area = Rect::new(0, 0, 10, 1);
        let panes = split_h(area, &[Constraint::Fill(0), Constraint::Fill(0)]);
        let widths: Vec<u16> = panes.iter().map(Rect::width).collect();
        assert_eq!(widths, vec![0, 0]);
    }

    #[test]
    fn weighted_fill_mixes_with_min_and_max_at_weight_one() {
        let area = Rect::new(0, 0, 20, 1);
        // Fill(3) claims 3 parts of the 6-way weight pool (3 + 1 + 1 + 1 = 6);
        // Min(2) and Max(10) each claim 1 part like before. Remainder after
        // Min's floor: 20 - 2 = 18, split 3:1:1:1 -> 9, 3, 3, 3; Min ends at
        // 2 + 3 = 5.
        let panes = split_h(
            area,
            &[
                Constraint::Fill(3),
                Constraint::Min(2),
                Constraint::Fill(1),
                Constraint::Max(10),
            ],
        );
        let widths: Vec<u16> = panes.iter().map(Rect::width).collect();
        assert_eq!(widths, vec![9, 5, 3, 3]);
        assert_eq!(widths.iter().sum::<u16>(), 20);
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

    #[test]
    fn spaced_split_carves_out_gaps_between_panes() {
        let area = Rect::new(0, 0, 59, 6);
        let panes = split_h_spaced(
            area,
            &[
                Constraint::Fill(1),
                Constraint::Fill(1),
                Constraint::Fill(1),
            ],
            1,
        );
        assert_eq!(panes.len(), 3);
        let widths: Vec<u16> = panes.iter().map(Rect::width).collect();
        assert_eq!(widths, vec![19, 19, 19]);
        // Adjacent panes are separated by exactly one gap cell, not touching.
        assert_eq!(panes[1].left(), panes[0].right() + 1);
        assert_eq!(panes[2].left(), panes[1].right() + 1);
    }

    #[test]
    fn spaced_split_falls_back_with_one_pane_or_no_spacing() {
        let area = Rect::new(0, 0, 10, 1);
        assert_eq!(
            split_h_spaced(area, &[Constraint::Fill(1)], 1),
            split_h(area, &[Constraint::Fill(1)])
        );
        assert_eq!(
            split_h_spaced(area, &[Constraint::Fill(1), Constraint::Fill(1)], 0),
            split_h(area, &[Constraint::Fill(1), Constraint::Fill(1)])
        );
    }

    #[test]
    fn vertical_spaced_split_matches_horizontal_shape() {
        let area = Rect::new(0, 0, 6, 59);
        let panes = split_v_spaced(
            area,
            &[
                Constraint::Fill(1),
                Constraint::Fill(1),
                Constraint::Fill(1),
            ],
            1,
        );
        let heights: Vec<u16> = panes.iter().map(Rect::height).collect();
        assert_eq!(heights, vec![19, 19, 19]);
        assert_eq!(panes[1].top(), panes[0].bottom() + 1);
    }

    #[test]
    fn centered_rect_centers_within_the_screen() {
        let screen = Rect::new(0, 0, 20, 10);
        let r = centered_rect(screen, 10, 4);
        assert_eq!(r, Rect::new(5, 3, 10, 4));
    }

    #[test]
    fn centered_rect_clamps_to_the_screen_size_when_larger() {
        let screen = Rect::new(0, 0, 20, 10);
        let r = centered_rect(screen, 100, 100);
        assert_eq!(r, Rect::new(0, 0, 20, 10));
    }

    #[test]
    fn centered_rect_respects_a_non_origin_screen() {
        let screen = Rect::new(5, 5, 20, 10);
        let r = centered_rect(screen, 10, 4);
        assert_eq!(r, Rect::new(10, 8, 10, 4));
    }

    /// `solve`'s internal `SmallBuf` scratch buffers stay on the stack for up to `STACK_CAP`
    /// (8) items and fall back to the heap past that; this covers a constraint count past the
    /// cap (all-`Fixed`, so `sizes` alone crosses into the heap path) and asserts the result is
    /// identical in shape to what an all-`Vec` implementation would produce: every pane keeps its
    /// requested size and the total exactly fills the area.
    #[test]
    fn split_beyond_stack_cap_matches_small_case_behavior() {
        let panes = 20; // > STACK_CAP
        let area = Rect::new(0, 0, panes as u16, 1);
        let constraints = vec![Constraint::Fixed(1); panes];
        let widths: Vec<u16> = split_h(area, &constraints)
            .iter()
            .map(Rect::width)
            .collect();
        assert_eq!(widths, vec![1u16; panes]);
        assert_eq!(widths.iter().sum::<u16>(), panes as u16);
    }

    /// Same as above, but exercises the flexible-pane path (`flexible`/`shares`/`fracs`/`order`
    /// scratch buffers) past `STACK_CAP` by mixing every `Constraint` kind across enough panes
    /// that the flexible subset alone also crosses the stack cap.
    #[test]
    fn weighted_fill_beyond_stack_cap_matches_small_case_proportions() {
        let area = Rect::new(0, 0, 100, 1);
        // 20 Fill(1) panes: same proportional-split logic as the 2/3-pane cases above, just at
        // a pane count that forces every scratch buffer in `solve` onto the heap.
        let constraints = vec![Constraint::Fill(1); 20];
        let widths: Vec<u16> = split_h(area, &constraints)
            .iter()
            .map(Rect::width)
            .collect();
        assert_eq!(widths.len(), 20);
        assert_eq!(widths.iter().sum::<u16>(), 100);
        // Equal weights distribute as evenly as integer division allows: every width is 5.
        assert!(widths.iter().all(|&w| w == 5));
    }
}
