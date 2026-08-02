//! Constraint-based `Rect` splitter for multi-panel UIs.
//!
//! Splits a [`Rect`] into stacked rows ([`split_v`]) or side-by-side columns
//! ([`split_h`]) according to a slice of [`Constraint`]s. [`split_h_spaced`]/[`split_v_spaced`]
//! do the same but also carve a fixed-cell gap between every adjacent pair of panes, without the
//! caller having to interleave `Constraint::Fixed(spacing)` gap constraints and filter them back
//! out by hand.
//!
//! Every `split_*` function has a const-generic `split_*_n` sibling ([`split_v_n`]/[`split_h_n`],
//! plus the `_flex`/`_spaced` combinations) that takes `[Constraint; N]` and returns `[Rect; N]`
//! instead of allocating a `Vec<Rect>`: useful for a fixed pane count re-split every frame, and
//! the array return type lets a caller destructure (`let [header, body] = split_v_n(area, [..]);`)
//! instead of indexing into a `Vec` that can silently drift out of sync with the constraint list.
//!
//! The solver sums the [`Fixed`](Constraint::Fixed), [`Percent`](Constraint::Percent), and
//! [`Ratio`](Constraint::Ratio) amounts, then distributes whatever remains across the
//! [`Fill`](Constraint::Fill),
//! [`Min`](Constraint::Min), and [`Max`](Constraint::Max) panes in proportion to their
//! weight: a `Fill(w)` pane claims a share proportional to `w` relative
//! to the other flexible panes, while [`Min`](Constraint::Min) and [`Max`](Constraint::Max)
//! panes always weigh 1. `Fill(1)` (equivalent to every pane weighing 1) reproduces plain
//! equal distribution. Sizes are clamped so the panes never spill past `area`. This is a
//! single sequential pass, not an iterative constraint solver: a [`Max`](Constraint::Max)
//! pane that is capped below its share does not redistribute the excess to other panes, so
//! leftover space can remain unclaimed (see [`Flex`] for how that leftover is placed via
//! [`split_v_flex`]/[`split_h_flex`]).
use retroglyph_core::{Rect, Size};

/// How a single pane claims space along the split axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Constraint {
    /// An exact number of cells.
    Fixed(u16),
    /// A percentage (0–100) of the axis length.
    Percent(u16),
    /// A proportional share of the axis length, `numerator / denominator`, without picking
    /// an arbitrary [`Fill`](Self::Fill) weight. Resolves like [`Percent`](Self::Percent): a
    /// fixed size computed up front, not a weighted share of the remainder. A zero
    /// `denominator` resolves to zero rather than panicking.
    Ratio(u16, u16),
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
                // `p` is clamped to `0..=100`, so `total * p / 100 <= total`, itself a `u16`.
                #[allow(clippy::cast_possible_truncation)]
                {
                    (u32::from(total) * p / 100) as u16
                }
            }
            Self::Ratio(num, den) => {
                if den == 0 {
                    0
                } else {
                    // `num / den` can exceed 1 (e.g. `Ratio(3, 2)`), so the result is clamped to
                    // `total` rather than relying on the ratio alone to stay in range.
                    #[allow(clippy::cast_possible_truncation)]
                    {
                        (u32::from(total) * u32::from(num) / u32::from(den)).min(u32::from(total))
                            as u16
                    }
                }
            }
            Self::Fill(_) | Self::Max(_) => 0,
        }
    }
}

/// Constraint counts at or below this stay on the stack in [`SmallBuf`]; larger splits fall back
/// to a heap `Vec`. Chosen comfortably above a typical multi-panel layout (a header, a handful of
/// flexible content panes, a status bar) while staying correct for arbitrarily many panes: see
/// the `layout_solve` benchmark's 100-pane case, which exercises the heap fallback.
const STACK_CAP: usize = 8;

/// A small buffer that stays inline on the stack for up to `N` items and only allocates on the
/// heap past that. `solve` uses this for its scratch buffers (pane sizes, the flexible-pane
/// index/weight/cap list, and the largest-remainder distribution pass) so that the common case of
/// a handful of panes per split (called several times per frame by multi-panel UIs) does not
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
    /// Panics if the buffer is the `Stack` variant and already holds `N` items: callers must
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
            Constraint::Fixed(_) | Constraint::Percent(_) | Constraint::Ratio(_, _) => {}
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
                // `shares[k]` is an integer share of `remainder` (a `u16` widened to `u32`), so it
                // can never exceed `remainder` itself and fits back in a `u16`.
                #[allow(clippy::cast_possible_truncation)]
                let share = shares[k] as u16;
                let grown = sizes[i].saturating_add(share);
                sizes[i] = cap.map_or(grown, |max| grown.min(max));
            }
        }
    }

    sizes
}

/// Const-generic sibling of [`solve`]: same algorithm, but sized to `N` at compile time, so
/// every scratch buffer is a plain `[T; N]` on the stack and there is no [`SmallBuf`] heap
/// fallback to worry about, whatever `N` is. Used by [`split_v_n`]/[`split_h_n`] and their
/// `_flex`/`_spaced` siblings.
fn solve_n<const N: usize>(total: u16, constraints: &[Constraint; N]) -> [u16; N] {
    let mut sizes = [0u16; N];
    for (i, c) in constraints.iter().enumerate() {
        sizes[i] = c.base(total);
    }

    // Clamp the fixed/percent sum so it never exceeds the axis, same as `solve`.
    let mut used: u16 = 0;
    for size in &mut sizes {
        let room = total.saturating_sub(used);
        *size = (*size).min(room);
        used += *size;
    }

    // Same largest-remainder distribution as `solve`, but into fixed-size `[T; N]` scratch
    // (at most `N` panes can be flexible, so `N` is always enough room).
    let mut flexible: [(usize, u16, Option<u16>); N] = [(0, 0, None); N];
    let mut flex_len = 0usize;
    for (i, c) in constraints.iter().enumerate() {
        match c {
            Constraint::Fill(weight) => {
                flexible[flex_len] = (i, *weight, None);
                flex_len += 1;
            }
            Constraint::Min(_) => {
                flexible[flex_len] = (i, 1, None);
                flex_len += 1;
            }
            Constraint::Max(cap) => {
                flexible[flex_len] = (i, 1, Some(*cap));
                flex_len += 1;
            }
            Constraint::Fixed(_) | Constraint::Percent(_) | Constraint::Ratio(_, _) => {}
        }
    }
    if flex_len > 0 {
        let remainder = total.saturating_sub(used);
        let total_weight: u32 = flexible[..flex_len]
            .iter()
            .map(|&(_, w, _)| u32::from(w))
            .sum();
        if let Some(total_weight) = std::num::NonZeroU32::new(total_weight) {
            let mut shares = [0u32; N];
            let mut fracs = [0u32; N];
            let mut floor_sum: u32 = 0;
            for (k, &(_, weight, _)) in flexible[..flex_len].iter().enumerate() {
                let product = u32::from(remainder) * u32::from(weight);
                let share = product / total_weight;
                fracs[k] = product % total_weight;
                shares[k] = share;
                floor_sum += share;
            }
            let mut leftover = u32::from(remainder).saturating_sub(floor_sum);
            let mut order = [0usize; N];
            for (idx, slot) in order[..flex_len].iter_mut().enumerate() {
                *slot = idx;
            }
            order[..flex_len].sort_by(|&a, &b| fracs[b].cmp(&fracs[a]).then(a.cmp(&b)));
            for &idx in &order[..flex_len] {
                if leftover == 0 {
                    break;
                }
                shares[idx] += 1;
                leftover -= 1;
            }
            for (k, &(i, _, cap)) in flexible[..flex_len].iter().enumerate() {
                // `shares[k]` is an integer share of `remainder` (a `u16` widened to `u32`), so
                // it can never exceed `remainder` itself and fits back in a `u16`.
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
/// Never panics: a degenerate `area` (zero height, zero width, or both) resolves every
/// constraint to a zero-height pane via [`saturating_sub`](u16::saturating_sub) arithmetic
/// rather than under/overflowing, and an empty `constraints` slice simply returns an empty
/// `Vec`.
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

/// Split `area` into stacked rows top-to-bottom, like [`split_v`], but sized to a compile-time
/// pane count `N`: takes `[Constraint; N]` and returns `[Rect; N]` instead of allocating a `Vec`.
///
/// The array length ties the constraint count to the return type, so a caller that destructures
/// the result (`let [header, body, footer] = split_v_n(area, [..]);`) gets a compile error if it
/// adds or removes a constraint without updating the destructuring pattern, rather than a
/// silently out-of-sync index into a `Vec`. Never allocates, for any `N`.
///
/// Never panics, for the same reason as [`split_v`].
///
/// # Examples
///
/// ```
/// use retroglyph_core::Rect;
/// use retroglyph_widgets::{Constraint, split_v_n};
///
/// let area = Rect::new(0, 0, 20, 10);
/// let [header, body, footer] =
///     split_v_n(area, [Constraint::Fixed(1), Constraint::Fill(1), Constraint::Fixed(1)]);
/// assert_eq!((header.height(), body.height(), footer.height()), (1, 8, 1));
/// ```
#[must_use]
pub fn split_v_n<const N: usize>(area: Rect, constraints: [Constraint; N]) -> [Rect; N] {
    let sizes = solve_n(area.height(), &constraints);
    let mut y = area.top();
    std::array::from_fn(|i| {
        let h = sizes[i];
        let rect = Rect::new(area.left(), y, area.width(), h);
        y = y.saturating_add(h);
        rect
    })
}

/// Split `area` into columns left-to-right.
///
/// Returns one [`Rect`] per constraint; empty panes (zero width) are still
/// returned so indices line up with `constraints`.
///
/// Never panics, for the same reason as [`split_v`]: a degenerate `area` resolves every
/// constraint to a zero-width pane instead of under/overflowing, and an empty `constraints`
/// slice returns an empty `Vec`.
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

/// Split `area` into columns left-to-right, like [`split_h`], but sized to a compile-time pane
/// count `N`: takes `[Constraint; N]` and returns `[Rect; N]` instead of allocating a `Vec`.
///
/// See [`split_v_n`] for why the array-sized signature is worth it over indexing a `Vec`. Never
/// allocates, for any `N`; never panics, for the same reason as [`split_h`].
///
/// # Examples
///
/// ```
/// use retroglyph_core::Rect;
/// use retroglyph_widgets::{Constraint, split_h_n};
///
/// let area = Rect::new(0, 0, 100, 5);
/// let [left, right] = split_h_n(area, [Constraint::Percent(30), Constraint::Fill(1)]);
/// assert_eq!((left.width(), right.width()), (30, 70));
/// ```
#[must_use]
pub fn split_h_n<const N: usize>(area: Rect, constraints: [Constraint; N]) -> [Rect; N] {
    let sizes = solve_n(area.width(), &constraints);
    let mut x = area.left();
    std::array::from_fn(|i| {
        let w = sizes[i];
        let rect = Rect::new(x, area.top(), w, area.height());
        x = x.saturating_add(w);
        rect
    })
}

/// Interleaves a `Constraint::Fixed(spacing)` gap between every pair of adjacent `constraints`.
///
/// `[c0, c1, c2]` with `spacing` becomes `[c0, Fixed(spacing), c1, Fixed(spacing), c2]`: the
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
/// [`split_h`], then discarding the gap panes, but the caller only ever sees the content panes,
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

/// Split `area` into columns left-to-right, like [`split_h_n`], but with a fixed `spacing`-cell
/// gap carved out between every adjacent pair of panes, like [`split_h_spaced`].
///
/// Reserves `spacing * (N - 1)` cells up front (equivalent to [`split_h_spaced`]'s constraint
/// interleaving, without needing a `2 * N - 1`-sized scratch array) and solves the remaining
/// panes against what's left, so [`Fill`](Constraint::Fill)/[`Percent`](Constraint::Percent)
/// panes share only the space after every gap is reserved, same as [`split_h_spaced`]. No-op
/// (falls back to [`split_h_n`]) with fewer than two panes or zero spacing. Never allocates.
///
/// # Examples
///
/// ```
/// use retroglyph_core::Rect;
/// use retroglyph_widgets::{Constraint, split_h_n_spaced};
///
/// let area = Rect::new(0, 0, 59, 6);
/// let [a, b, c] = split_h_n_spaced(area, [Constraint::Fill(1); 3], 1);
/// assert_eq!((a.width(), b.width(), c.width()), (19, 19, 19));
/// assert_eq!(b.left(), a.right() + 1);
/// ```
#[must_use]
pub fn split_h_n_spaced<const N: usize>(
    area: Rect,
    constraints: [Constraint; N],
    spacing: u16,
) -> [Rect; N] {
    if spacing == 0 || N < 2 {
        return split_h_n(area, constraints);
    }
    // `N` is the number of panes in one layout split, nowhere near `u16::MAX` in any realistic
    // UI, and `N >= 2` here so `N as u16 - 1` cannot underflow.
    #[allow(clippy::cast_possible_truncation)]
    let total_spacing = spacing.saturating_mul(N as u16 - 1);
    let sizes = solve_n(area.width().saturating_sub(total_spacing), &constraints);
    let mut x = area.left();
    std::array::from_fn(|i| {
        let w = sizes[i];
        let rect = Rect::new(x, area.top(), w, area.height());
        x = x.saturating_add(w).saturating_add(spacing);
        rect
    })
}

/// Split `area` into stacked rows top-to-bottom, like [`split_v_n`], but with a fixed
/// `spacing`-cell gap carved out between every adjacent pair of panes, like [`split_v_spaced`].
///
/// See [`split_h_n_spaced`] for the full behavior; this is the same operation along the vertical
/// axis. Never allocates.
#[must_use]
pub fn split_v_n_spaced<const N: usize>(
    area: Rect,
    constraints: [Constraint; N],
    spacing: u16,
) -> [Rect; N] {
    if spacing == 0 || N < 2 {
        return split_v_n(area, constraints);
    }
    // `N` is the number of panes in one layout split, nowhere near `u16::MAX` in any realistic
    // UI, and `N >= 2` here so `N as u16 - 1` cannot underflow.
    #[allow(clippy::cast_possible_truncation)]
    let total_spacing = spacing.saturating_mul(N as u16 - 1);
    let sizes = solve_n(area.height().saturating_sub(total_spacing), &constraints);
    let mut y = area.top();
    std::array::from_fn(|i| {
        let h = sizes[i];
        let rect = Rect::new(area.left(), y, area.width(), h);
        y = y.saturating_add(h).saturating_add(spacing);
        rect
    })
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
#[non_exhaustive]
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
///
/// Returns a [`SmallBuf`], not a `Vec`, so the common small-split case does not pay for a heap
/// allocation here either (same rationale as `solve`'s scratch buffers).
fn place(total: u16, sizes: &[u16], flex: Flex) -> SmallBuf<u16, STACK_CAP> {
    let content: u16 = sizes.iter().fold(0u16, |a, &b| a.saturating_add(b));
    let slack = total.saturating_sub(content);
    let n = sizes.len();
    let mut offsets: SmallBuf<u16, STACK_CAP> = SmallBuf::with_capacity(n);

    let packed_from = |start: u16, offsets: &mut SmallBuf<u16, STACK_CAP>| {
        let mut pos = start;
        for &s in sizes {
            offsets.push(pos);
            pos = pos.saturating_add(s);
        }
    };

    match flex {
        Flex::End => packed_from(slack, &mut offsets),
        Flex::Center => packed_from(slack / 2, &mut offsets),
        Flex::SpaceBetween if n > 1 => {
            // `n` is the number of panes in one layout split, nowhere near `u16::MAX` in any
            // realistic UI.
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
        Flex::Start | Flex::SpaceBetween => packed_from(0, &mut offsets),
        Flex::SpaceAround => {
            // `n` is the number of panes in one layout split, nowhere near `u16::MAX` in any
            // realistic UI.
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

/// Const-generic sibling of [`place`], used by [`split_v_n_flex`]/[`split_h_n_flex`]. Same
/// algorithm, but into a fixed-size `[u16; N]` so it never allocates, for any `N`.
fn place_n<const N: usize>(total: u16, sizes: &[u16; N], flex: Flex) -> [u16; N] {
    let content: u16 = sizes.iter().fold(0u16, |a, &b| a.saturating_add(b));
    let slack = total.saturating_sub(content);
    let mut offsets = [0u16; N];

    let packed_from = |start: u16, offsets: &mut [u16; N]| {
        let mut pos = start;
        for (o, &s) in offsets.iter_mut().zip(sizes.iter()) {
            *o = pos;
            pos = pos.saturating_add(s);
        }
    };

    match flex {
        Flex::End => packed_from(slack, &mut offsets),
        Flex::Center => packed_from(slack / 2, &mut offsets),
        Flex::SpaceBetween if N > 1 => {
            // `N` is the number of panes in one layout split, nowhere near `u16::MAX` in any
            // realistic UI.
            #[allow(clippy::cast_possible_truncation)]
            let gaps = N as u16 - 1;
            let gap = slack / gaps;
            let mut extra = slack % gaps;
            let mut pos = 0;
            for (i, &s) in sizes.iter().enumerate() {
                offsets[i] = pos;
                pos = pos.saturating_add(s);
                if i + 1 < N {
                    pos = pos.saturating_add(gap + u16::from(extra > 0));
                    extra = extra.saturating_sub(1);
                }
            }
        }
        Flex::Start | Flex::SpaceBetween => packed_from(0, &mut offsets),
        Flex::SpaceAround => {
            // `N` is the number of panes in one layout split, nowhere near `u16::MAX` in any
            // realistic UI.
            #[allow(clippy::cast_possible_truncation)]
            let gaps = N as u16 + 1;
            let unit = slack / gaps;
            let mut extra = slack % gaps;
            let mut pos = unit + u16::from(extra > 0);
            extra = extra.saturating_sub(u16::from(extra > 0));
            for (i, &s) in sizes.iter().enumerate() {
                offsets[i] = pos;
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
///
/// Never panics, for the same reason as [`split_v`]: every offset is computed with
/// [`saturating_add`](u16::saturating_add)/[`saturating_sub`](u16::saturating_sub).
#[must_use]
pub fn split_v_flex(area: Rect, constraints: &[Constraint], flex: Flex) -> Vec<Rect> {
    let sizes = solve(area.height(), constraints);
    let offsets = place(area.height(), &sizes, flex);
    offsets
        .iter()
        .copied()
        .zip(sizes.iter().copied())
        .map(|(y, h)| Rect::new(area.left(), area.top().saturating_add(y), area.width(), h))
        .collect()
}

/// Split `area` into stacked rows top-to-bottom, like [`split_v_n`], but with explicit control
/// over how leftover space is placed via [`Flex`], like [`split_v_flex`].
///
/// Never allocates, for any `N`; never panics, for the same reason as [`split_v_flex`].
#[must_use]
pub fn split_v_n_flex<const N: usize>(
    area: Rect,
    constraints: [Constraint; N],
    flex: Flex,
) -> [Rect; N] {
    let sizes = solve_n(area.height(), &constraints);
    let offsets = place_n(area.height(), &sizes, flex);
    std::array::from_fn(|i| {
        Rect::new(
            area.left(),
            area.top().saturating_add(offsets[i]),
            area.width(),
            sizes[i],
        )
    })
}

/// Split `area` into columns left-to-right, like [`split_h`], but with
/// explicit control over how leftover space is placed via [`Flex`].
///
/// Never panics, for the same reason as [`split_h`]: every offset is computed with
/// [`saturating_add`](u16::saturating_add)/[`saturating_sub`](u16::saturating_sub).
#[must_use]
pub fn split_h_flex(area: Rect, constraints: &[Constraint], flex: Flex) -> Vec<Rect> {
    let sizes = solve(area.width(), constraints);
    let offsets = place(area.width(), &sizes, flex);
    offsets
        .iter()
        .copied()
        .zip(sizes.iter().copied())
        .map(|(x, w)| Rect::new(area.left().saturating_add(x), area.top(), w, area.height()))
        .collect()
}

/// Split `area` into columns left-to-right, like [`split_h_n`], but with explicit control over
/// how leftover space is placed via [`Flex`], like [`split_h_flex`].
///
/// Never allocates, for any `N`; never panics, for the same reason as [`split_h_flex`].
#[must_use]
pub fn split_h_n_flex<const N: usize>(
    area: Rect,
    constraints: [Constraint; N],
    flex: Flex,
) -> [Rect; N] {
    let sizes = solve_n(area.width(), &constraints);
    let offsets = place_n(area.width(), &sizes, flex);
    std::array::from_fn(|i| {
        Rect::new(
            area.left().saturating_add(offsets[i]),
            area.top(),
            sizes[i],
            area.height(),
        )
    })
}

/// Compute a `width`×`height` [`Rect`] centered within `screen`.
///
/// `width`/`height` are clamped down to `screen`'s own dimensions if larger,
/// so the result never extends past `screen`'s edges: a modal, dialog, or
/// tooltip box built from this is always fully on-screen, even on a
/// terminal too small to fit the box's requested size. Pure layout math: no
/// drawing, no `Terminal`. Pairs with `panel`/`modal` in `retroglyph-widgets`
/// (the `draw` module) for a centered, bordered box.
///
/// Never panics: the clamp and centering offsets are computed with saturating arithmetic, so a
/// zero-size `screen`, `width`, or `height` resolves to a zero-size or edge-pinned rect instead
/// of under/overflowing.
#[must_use]
pub fn centered_rect(screen: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(screen.width());
    let height = height.min(screen.height());
    let x = screen.left() + (screen.width() - width) / 2;
    let y = screen.top() + (screen.height() - height) / 2;
    Rect::new(x, y, width, height)
}

/// Which side of an anchor rect a panel prefers to open on, for [`anchored_rect`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// Open above the anchor (panel's bottom edge touches the anchor's top edge).
    Above,
    /// Open below the anchor (panel's top edge touches the anchor's bottom edge). The usual
    /// choice for a dropdown under a menu label or a field.
    Below,
    /// Open to the left of the anchor (panel's right edge touches the anchor's left edge).
    Left,
    /// Open to the right of the anchor (panel's left edge touches the anchor's right edge).
    Right,
}

impl Side {
    /// The side to fall back to when this side doesn't have room, per [`anchored_rect`].
    const fn opposite(self) -> Self {
        match self {
            Self::Above => Self::Below,
            Self::Below => Self::Above,
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

/// Place a `size` panel adjacent to `anchor`, preferring `side`, flipping to the opposite side
/// when there isn't room, and clamped to stay within `bounds`.
///
/// `side` decides which edge of `anchor` the panel opens from: [`Side::Below`]/[`Side::Above`]
/// place the panel's left edge at `anchor`'s left edge and stack it vertically off `anchor`'s
/// bottom/top edge; [`Side::Right`]/[`Side::Left`] place the panel's top edge at `anchor`'s top
/// edge and lay it out horizontally off `anchor`'s right/left edge. If the preferred side doesn't
/// have enough room within `bounds` (the panel's far edge would fall outside `bounds` on that
/// axis) but the opposite side does, the panel opens on the opposite side instead; if neither
/// side has room, the preferred side is kept and clamped like the fitting case. Once a side is
/// chosen, the panel is clamped along the perpendicular axis so it never runs past `bounds`'
/// edges: this is the three-line clamp a hand-rolled dropdown would otherwise repeat
/// (`x.min(bounds.right() - width).max(bounds.left())`), applied to whichever axis `side` didn't
/// already pin.
///
/// `size` is clamped down to `bounds`' own dimensions if larger, so the result is always fully
/// within `bounds`, the same guarantee [`centered_rect`] makes for a centered box.
///
/// Pure layout math: no drawing, no `Terminal`. Callers still own sizing (deciding `size` from
/// content, with a floor/ceiling) and overflow (scrolling when content is taller than the
/// resulting rect); this only answers where the rect goes.
///
/// Never panics: every offset is computed with saturating arithmetic, so a degenerate `anchor`,
/// `size`, or `bounds` (zero width/height, or `anchor` outside `bounds`) resolves to a clamped,
/// zero-size-or-larger rect instead of under/overflowing.
///
/// # Examples
///
/// ```
/// use retroglyph_core::{Rect, Size};
/// use retroglyph_widgets::{Side, anchored_rect};
///
/// let bounds = Rect::new(0, 0, 40, 20);
/// let anchor = Rect::new(5, 5, 10, 1); // e.g. a menu label
/// let rect = anchored_rect(anchor, Size::new(12, 4), Side::Below, bounds);
/// assert_eq!(rect, Rect::new(5, 6, 12, 4));
/// ```
#[must_use]
pub fn anchored_rect(anchor: Rect, size: Size, preferred: Side, bounds: Rect) -> Rect {
    let width = size.width().min(bounds.width());
    let height = size.height().min(bounds.height());

    // `checked_sub` (not `saturating_sub`): a saturated 0 would make an anchor too close to
    // `bounds`' start edge for `height`/`width` look like it fits with room to spare.
    let fits = |candidate: Side| match candidate {
        Side::Above => anchor
            .top()
            .checked_sub(height)
            .is_some_and(|t| t >= bounds.top()),
        Side::Below => anchor.bottom().saturating_add(height) <= bounds.bottom(),
        Side::Left => anchor
            .left()
            .checked_sub(width)
            .is_some_and(|l| l >= bounds.left()),
        Side::Right => anchor.right().saturating_add(width) <= bounds.right(),
    };
    let resolved = if fits(preferred) || !fits(preferred.opposite()) {
        preferred
    } else {
        preferred.opposite()
    };

    let (x, y) = match resolved {
        Side::Above => {
            let x = anchor
                .left()
                .min(bounds.right().saturating_sub(width))
                .max(bounds.left());
            let y = anchor.top().saturating_sub(height).max(bounds.top());
            (x, y)
        }
        Side::Below => {
            let x = anchor
                .left()
                .min(bounds.right().saturating_sub(width))
                .max(bounds.left());
            let y = anchor
                .bottom()
                .min(bounds.bottom().saturating_sub(height))
                .max(bounds.top());
            (x, y)
        }
        Side::Left => {
            let x = anchor.left().saturating_sub(width).max(bounds.left());
            let y = anchor
                .top()
                .min(bounds.bottom().saturating_sub(height))
                .max(bounds.top());
            (x, y)
        }
        Side::Right => {
            let x = anchor
                .right()
                .min(bounds.right().saturating_sub(width))
                .max(bounds.left());
            let y = anchor
                .top()
                .min(bounds.bottom().saturating_sub(height))
                .max(bounds.top());
            (x, y)
        }
    };

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
    fn horizontal_ratio_and_fill() {
        let area = Rect::new(0, 0, 100, 5);
        let panes = split_h(area, &[Constraint::Ratio(3, 10), Constraint::Fill(1)]);
        assert_eq!(panes[0].width(), 30);
        assert_eq!(panes[1].width(), 70);
    }

    #[test]
    fn ratio_zero_denominator_resolves_to_zero() {
        let area = Rect::new(0, 0, 100, 5);
        let panes = split_h(area, &[Constraint::Ratio(1, 0), Constraint::Fill(1)]);
        assert_eq!(panes[0].width(), 0);
        assert_eq!(panes[1].width(), 100);
    }

    #[test]
    fn ratio_over_one_clamps_to_total() {
        let area = Rect::new(0, 0, 100, 5);
        let panes = split_h(area, &[Constraint::Ratio(3, 2)]);
        assert_eq!(panes[0].width(), 100);
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
        // ahead of Fill(3), not to the first pane in the slice.
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

    #[test]
    fn anchored_rect_opens_below_when_preferred() {
        let bounds = Rect::new(0, 0, 40, 20);
        let anchor = Rect::new(5, 5, 10, 1);
        let r = anchored_rect(anchor, Size::new(12, 4), Side::Below, bounds);
        assert_eq!(r, Rect::new(5, 6, 12, 4));
    }

    #[test]
    fn anchored_rect_opens_above_when_preferred() {
        let bounds = Rect::new(0, 0, 40, 20);
        let anchor = Rect::new(5, 10, 10, 1);
        let r = anchored_rect(anchor, Size::new(12, 4), Side::Above, bounds);
        assert_eq!(r, Rect::new(5, 6, 12, 4));
    }

    #[test]
    fn anchored_rect_flips_below_to_above_when_there_is_no_room_below() {
        let bounds = Rect::new(0, 0, 40, 20);
        // Anchor near the bottom: no room for a 4-tall panel below, but there is above.
        let anchor = Rect::new(5, 18, 10, 1);
        let r = anchored_rect(anchor, Size::new(12, 4), Side::Below, bounds);
        assert_eq!(r, Rect::new(5, 14, 12, 4));
    }

    #[test]
    fn anchored_rect_flips_above_to_below_when_there_is_no_room_above() {
        let bounds = Rect::new(0, 0, 40, 20);
        // Anchor near the top: no room for a 4-tall panel above, but there is below.
        let anchor = Rect::new(5, 1, 10, 1);
        let r = anchored_rect(anchor, Size::new(12, 4), Side::Above, bounds);
        assert_eq!(r, Rect::new(5, 2, 12, 4));
    }

    #[test]
    fn anchored_rect_keeps_preferred_side_when_neither_side_has_room() {
        let bounds = Rect::new(0, 0, 40, 3);
        let anchor = Rect::new(5, 1, 10, 1);
        // Neither above nor below fits the panel (bounds is only 3 rows tall); the panel's
        // height is itself first clamped down to bounds.height() (3), stays on the preferred
        // Below side, and its clamped-height rect is then pulled up to fit inside bounds.
        let r = anchored_rect(anchor, Size::new(12, 4), Side::Below, bounds);
        assert_eq!(r, Rect::new(5, 0, 12, 3));
    }

    #[test]
    fn anchored_rect_clamps_to_the_right_bounds_edge() {
        let bounds = Rect::new(0, 0, 20, 20);
        // Anchor near the right edge: a 12-wide panel starting at anchor.left() (15) would run
        // past bounds.right() (20), so it's pulled left to stay inside.
        let anchor = Rect::new(15, 5, 4, 1);
        let r = anchored_rect(anchor, Size::new(12, 4), Side::Below, bounds);
        assert_eq!(r, Rect::new(8, 6, 12, 4));
    }

    #[test]
    fn anchored_rect_clamps_to_the_left_bounds_edge() {
        // A non-origin bounds so an anchor can sit to the left of bounds.left() itself.
        let bounds = Rect::new(5, 0, 20, 20);
        let anchor = Rect::new(2, 5, 2, 1);
        let r = anchored_rect(anchor, Size::new(12, 4), Side::Below, bounds);
        assert_eq!(r, Rect::new(5, 6, 12, 4));
    }

    #[test]
    fn anchored_rect_opens_to_the_right_and_clamps_vertically() {
        let bounds = Rect::new(0, 0, 40, 10);
        // Anchor near the bottom: a 6-tall panel starting at anchor.top() would run past
        // bounds.bottom(), so it's pulled up to stay inside.
        let anchor = Rect::new(5, 8, 6, 1);
        let r = anchored_rect(anchor, Size::new(8, 6), Side::Right, bounds);
        assert_eq!(r, Rect::new(11, 4, 8, 6));
    }

    #[test]
    fn anchored_rect_opens_to_the_left() {
        let bounds = Rect::new(0, 0, 40, 10);
        let anchor = Rect::new(20, 2, 6, 1);
        let r = anchored_rect(anchor, Size::new(8, 4), Side::Left, bounds);
        assert_eq!(r, Rect::new(12, 2, 8, 4));
    }

    #[test]
    fn anchored_rect_clamps_size_down_to_bounds() {
        let bounds = Rect::new(0, 0, 10, 10);
        let anchor = Rect::new(2, 2, 2, 1);
        let r = anchored_rect(anchor, Size::new(100, 100), Side::Below, bounds);
        assert_eq!(r.width(), 10);
        assert_eq!(r.height(), 10);
        assert!(r.left() >= bounds.left() && r.right() <= bounds.right());
        assert!(r.top() >= bounds.top() && r.bottom() <= bounds.bottom());
    }

    #[test]
    fn anchored_rect_handles_a_zero_size_bounds() {
        let bounds = Rect::new(3, 3, 0, 0);
        let anchor = Rect::new(3, 3, 0, 0);
        let r = anchored_rect(anchor, Size::new(5, 5), Side::Below, bounds);
        assert_eq!(r, Rect::new(3, 3, 0, 0));
    }

    /// `solve`'s internal `SmallBuf` scratch buffers stay on the stack for up to `STACK_CAP`
    /// (8) items and fall back to the heap past that; this covers a constraint count past the
    /// cap (all-`Fixed`, so `sizes` alone crosses into the heap path) and asserts the result is
    /// identical in shape to what an all-`Vec` implementation would produce: every pane keeps its
    /// requested size and the total exactly fills the area.
    #[test]
    fn split_beyond_stack_cap_matches_small_case_behavior() {
        let panes = 20; // > STACK_CAP, and far below u16::MAX
        #[allow(clippy::cast_possible_truncation)]
        let panes_u16 = panes as u16;
        let area = Rect::new(0, 0, panes_u16, 1);
        let constraints = vec![Constraint::Fixed(1); panes];
        let widths: Vec<u16> = split_h(area, &constraints)
            .iter()
            .map(Rect::width)
            .collect();
        assert_eq!(widths, vec![1u16; panes]);
        assert_eq!(widths.iter().sum::<u16>(), panes_u16);
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

    #[test]
    fn split_v_n_matches_split_v() {
        let area = Rect::new(0, 0, 20, 10);
        let constraints = [
            Constraint::Fixed(1),
            Constraint::Fill(1),
            Constraint::Fixed(1),
        ];
        let vec_panes = split_v(area, &constraints);
        let [a, b, c] = split_v_n(area, constraints);
        assert_eq!(vec_panes, vec![a, b, c]);
    }

    #[test]
    fn split_h_n_matches_split_h() {
        let area = Rect::new(0, 0, 100, 5);
        let constraints = [Constraint::Percent(30), Constraint::Fill(1)];
        let vec_panes = split_h(area, &constraints);
        let [left, right] = split_h_n(area, constraints);
        assert_eq!(vec_panes, vec![left, right]);
    }

    #[test]
    fn split_v_n_destructures_by_compile_time_count() {
        let area = Rect::new(0, 0, 12, 12);
        let [header, body, footer] = split_v_n(
            area,
            [
                Constraint::Fixed(2),
                Constraint::Fill(1),
                Constraint::Fixed(2),
            ],
        );
        assert_eq!(header.height(), 2);
        assert_eq!(body.height(), 8);
        assert_eq!(footer.height(), 2);
        assert_eq!(header.top(), 0);
        assert_eq!(body.top(), 2);
        assert_eq!(footer.top(), 10);
    }

    #[test]
    fn split_h_n_flex_matches_split_h_flex() {
        let area = Rect::new(0, 0, 10, 1);
        let constraints = [Constraint::Fixed(2), Constraint::Fixed(2)];
        let vec_panes = split_h_flex(area, &constraints, Flex::SpaceBetween);
        let [a, b] = split_h_n_flex(area, constraints, Flex::SpaceBetween);
        assert_eq!(vec_panes, vec![a, b]);
    }

    #[test]
    fn split_v_n_flex_matches_split_v_flex() {
        let area = Rect::new(0, 0, 10, 10);
        let constraints = [Constraint::Fixed(2), Constraint::Fixed(2)];
        let vec_panes = split_v_flex(area, &constraints, Flex::End);
        let [a, b] = split_v_n_flex(area, constraints, Flex::End);
        assert_eq!(vec_panes, vec![a, b]);
    }

    #[test]
    fn split_h_n_spaced_matches_split_h_spaced() {
        let area = Rect::new(0, 0, 59, 6);
        let constraints = [Constraint::Fill(1); 3];
        let vec_panes = split_h_spaced(area, &constraints, 1);
        let [a, b, c] = split_h_n_spaced(area, constraints, 1);
        assert_eq!(vec_panes, vec![a, b, c]);
    }

    #[test]
    fn split_v_n_spaced_matches_split_v_spaced() {
        let area = Rect::new(0, 0, 6, 59);
        let constraints = [Constraint::Fill(1); 3];
        let vec_panes = split_v_spaced(area, &constraints, 1);
        let [a, b, c] = split_v_n_spaced(area, constraints, 1);
        assert_eq!(vec_panes, vec![a, b, c]);
    }

    #[test]
    fn split_h_n_spaced_falls_back_with_one_pane_or_no_spacing() {
        let area = Rect::new(0, 0, 10, 1);
        assert_eq!(
            split_h_n_spaced(area, [Constraint::Fill(1)], 1),
            [split_h_n(area, [Constraint::Fill(1)])[0]]
        );
        let constraints = [Constraint::Fill(1), Constraint::Fill(1)];
        assert_eq!(
            split_h_n_spaced(area, constraints, 0),
            split_h_n(area, constraints)
        );
    }

    /// `solve_n` never falls back to the heap, unlike `solve`'s `SmallBuf`; this covers a pane
    /// count past `STACK_CAP` to confirm that holds true and produces the same result `solve`
    /// would.
    #[test]
    fn split_h_n_beyond_stack_cap_matches_split_h() {
        let area = Rect::new(0, 0, 20, 1);
        let constraints = [Constraint::Fixed(1); 20];
        let vec_panes = split_h(area, &constraints);
        let arr_panes = split_h_n(area, constraints);
        assert_eq!(vec_panes, arr_panes.to_vec());
    }
}
