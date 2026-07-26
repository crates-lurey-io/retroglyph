//! [`Sparkline`]: a single-row bar chart of recent samples.
use retroglyph_core::{Rect, Style};

use super::{Meter, Widget};
use crate::Surface;

/// Vertical block glyphs from empty to full, indexed 0..=8.
const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// A single-row sparkline of `samples`, scaled to the sample max, using the
/// eight vertical block glyphs `▁▂▃▄▅▆▇█`.
///
/// The most recent samples are right-aligned so the graph scrolls left as
/// new data arrives. Bar height (and color) tracks each sample's fraction
/// of the max via [`Meter`]. Only the first row of `area` is drawn.
///
/// # Examples
///
/// ```
/// use retroglyph_core::{Grid, Rect};
/// use retroglyph_widgets::{Surface, Sparkline, Widget};
///
/// let samples = [1.0, 3.0, 2.0, 4.0, 1.5];
/// let mut grid = Grid::new(10, 1);
/// let area = Rect::new(0, 0, 10, 1);
/// Sparkline::new(&samples).render(area, &mut Surface::new(&mut grid, area, 0));
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Sparkline<'a> {
    samples: &'a [f32],
}

impl<'a> Sparkline<'a> {
    /// A sparkline of `samples`.
    #[must_use]
    pub const fn new(samples: &'a [f32]) -> Self {
        Self { samples }
    }
}

impl Widget for Sparkline<'_> {
    fn render(&self, area: Rect, surface: &mut Surface<'_>) {
        let width = area.width_usize();
        if width == 0 {
            return;
        }
        let max = self
            .samples
            .iter()
            .copied()
            .fold(0.0_f32, f32::max)
            .max(1e-6);

        // Take the last `width` samples so the graph is right-aligned.
        let start = self.samples.len().saturating_sub(width);
        let recent = &self.samples[start..];
        let pad = width - recent.len();

        let y = area.top();
        for i in 0..width {
            // `i` ranges over `0..width`, and `width` is `area.width_usize()`, itself widened
            // from `area`'s own `u16` width, so narrowing it back is always exact.
            #[allow(clippy::cast_possible_truncation)]
            let x = area.left() + i as u16;
            if i < pad {
                surface.put((x, y), ' ', Style::new());
                continue;
            }
            let ratio = (recent[i - pad] / max).clamp(0.0, 1.0);
            // `ratio` is clamped to `0.0..=1.0`, so the rounded level always lands in `0..=8`.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let level = (ratio * 8.0).round() as usize;
            surface.put(
                (x, y),
                BLOCKS[level.min(8)],
                Style::new().fg(Meter::new(ratio).color()),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::{Grid, Pos};

    use super::*;

    #[test]
    fn right_aligns_recent_samples_and_pads_the_rest() {
        let area = Rect::new(0, 0, 5, 1);
        let mut grid = Grid::new(5, 1);
        Sparkline::new(&[1.0, 2.0]).render(area, &mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
        assert_eq!(grid[Pos::new(2, 0)].glyph(), ' ');
        assert_eq!(grid[Pos::new(3, 0)].glyph(), BLOCKS[4]); // 1.0 / 2.0 -> half
        assert_eq!(grid[Pos::new(4, 0)].glyph(), BLOCKS[8]); // 2.0 / 2.0 -> full
    }

    #[test]
    fn empty_samples_is_a_no_op_beyond_blank_padding() {
        let area = Rect::new(0, 0, 3, 1);
        let mut grid = Grid::new(3, 1);
        Sparkline::new(&[]).render(area, &mut Surface::new(&mut grid, area, 0));
        for x in 0..3 {
            assert_eq!(grid[Pos::new(x, 0)].glyph(), ' ');
        }
    }
}
