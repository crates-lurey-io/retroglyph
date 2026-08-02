//! [`Sparkline`]: a single-row bar chart of recent samples.
use retroglyph_core::Style;
use retroglyph_core::symbols::bar::NINE_LEVELS;

use super::{Meter, Widget};
use crate::Surface;

/// A single-row sparkline of `samples`, scaled to the sample max, using the
/// eight vertical block glyphs `▁▂▃▄▅▆▇█`.
///
/// The most recent samples are right-aligned so the graph scrolls left as
/// new data arrives. By default, bar height *and color* track each sample's
/// fraction of the max via [`Meter`] (a green-to-red load ramp); call
/// [`Sparkline::style`] to draw every bar in one fixed color instead, height
/// only, the right choice once the color channel would otherwise imply
/// something the data doesn't mean (e.g. a frame-time graph, where "tallest
/// bar in view" isn't the same thing as "bad": [`super::PerfOverlay`] does
/// this). Only the first row of `area` is drawn.
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
/// Sparkline::new(&samples).render(&mut Surface::new(&mut grid, area, 0));
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Sparkline<'a> {
    samples: &'a [f32],
    style: Option<Style>,
}

impl<'a> Sparkline<'a> {
    /// A sparkline of `samples`, colored by [`Meter`] (green-to-red load ramp) unless overridden
    /// with [`style`](Self::style).
    #[must_use]
    pub const fn new(samples: &'a [f32]) -> Self {
        Self {
            samples,
            style: None,
        }
    }

    /// Draws every bar in `style`'s foreground color instead of the default [`Meter`] ramp.
    /// Height still tracks each sample's fraction of the max; only the color stops varying.
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = Some(style);
        self
    }
}

impl Widget for Sparkline<'_> {
    fn render(&self, surface: &mut Surface<'_>) {
        let width = usize::from(surface.width());
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

        for i in 0..width {
            // `i` ranges over `0..width`, itself widened from this surface's own `u16` width, so
            // narrowing it back is always exact.
            #[allow(clippy::cast_possible_truncation)]
            let x = i as u16;
            if i < pad {
                surface.put((x, 0), ' ', Style::new());
                continue;
            }
            let ratio = (recent[i - pad] / max).clamp(0.0, 1.0);
            // `ratio` is clamped to `0.0..=1.0`, so the rounded level always lands in `0..=8`.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let level = (ratio * 8.0).round() as usize;
            let style = self
                .style
                .unwrap_or_else(|| Style::new().fg(Meter::new(ratio).color()));
            surface.put((x, 0), NINE_LEVELS[level.min(8)], style);
        }
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::{Grid, Pos, Rect};

    use super::*;

    #[test]
    fn right_aligns_recent_samples_and_pads_the_rest() {
        let area = Rect::new(0, 0, 5, 1);
        let mut grid = Grid::new(5, 1);
        Sparkline::new(&[1.0, 2.0]).render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
        assert_eq!(grid[Pos::new(2, 0)].glyph(), ' ');
        assert_eq!(grid[Pos::new(3, 0)].glyph(), NINE_LEVELS[4]); // 1.0 / 2.0 -> half
        assert_eq!(grid[Pos::new(4, 0)].glyph(), NINE_LEVELS[8]); // 2.0 / 2.0 -> full
    }

    #[test]
    fn empty_samples_is_a_no_op_beyond_blank_padding() {
        let area = Rect::new(0, 0, 3, 1);
        let mut grid = Grid::new(3, 1);
        Sparkline::new(&[]).render(&mut Surface::new(&mut grid, area, 0));
        for x in 0..3 {
            assert_eq!(grid[Pos::new(x, 0)].glyph(), ' ');
        }
    }

    #[test]
    fn style_overrides_the_default_meter_ramp_with_one_fixed_color() {
        use retroglyph_core::Color;

        let area = Rect::new(0, 0, 3, 1);
        let mut grid = Grid::new(3, 1);
        let accent = Style::new().fg(Color::Rgb {
            r: 90,
            g: 170,
            b: 250,
        });
        Sparkline::new(&[1.0, 4.0, 2.0])
            .style(accent)
            .render(&mut Surface::new(&mut grid, area, 0));

        // Height still tracks the ratio (the low, high, mid samples land on different block
        // levels)...
        assert_ne!(grid[Pos::new(0, 0)].glyph(), grid[Pos::new(1, 0)].glyph());
        // ...but every bar shares the one fixed color, not a ramp that would color the tallest
        // bar (here, `4.0`, the max) differently from the shortest.
        for x in 0..3 {
            assert_eq!(grid[Pos::new(x, 0)].style(), accent);
        }
    }
}
