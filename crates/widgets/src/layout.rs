//! Constraint-based `Rect` splitter for multi-panel UIs.
//!
//! Splits a [`Rect`](retroglyph_core::Rect) into stacked rows ([`split_v`]) or
//! side-by-side columns ([`split_h`]) according to a slice of [`Constraint`]s.
//!
//! The solver sums the [`Fixed`](Constraint::Fixed) and [`Percent`](Constraint::Percent)
//! amounts, then distributes whatever remains equally across the
//! [`Fill`](Constraint::Fill) panes. Sizes are clamped so the panes never spill
//! past `area`. There is no min/max, spacing, or flex weight.
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
}

impl Constraint {
    /// Resolve this constraint's base size against `total` axis length.
    /// [`Fill`](Self::Fill) resolves to zero here; it is filled in later.
    fn base(self, total: u16) -> u16 {
        match self {
            Self::Fixed(n) => n.min(total),
            Self::Percent(p) => {
                let p = u32::from(p.min(100));
                #[allow(clippy::cast_possible_truncation)]
                {
                    (u32::from(total) * p / 100) as u16
                }
            }
            Self::Fill => 0,
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

    // Distribute the remainder equally across the Fill panes.
    let fill_indices: Vec<usize> = constraints
        .iter()
        .enumerate()
        .filter(|(_, c)| matches!(c, Constraint::Fill))
        .map(|(i, _)| i)
        .collect();
    if !fill_indices.is_empty() {
        let remainder = total.saturating_sub(used);
        #[allow(clippy::cast_possible_truncation)]
        let each = remainder / fill_indices.len() as u16;
        #[allow(clippy::cast_possible_truncation)]
        let mut extra = remainder % fill_indices.len() as u16;
        for &i in &fill_indices {
            sizes[i] = each + u16::from(extra > 0);
            extra = extra.saturating_sub(1);
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
}
