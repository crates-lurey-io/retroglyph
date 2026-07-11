//! [`Sparkline`]: a single-row bar chart of recent samples.
use retroglyph_core::{Backend, Rect, Style, Terminal};

use super::{Meter, Widget};

/// Vertical block glyphs from empty to full, indexed 0..=8.
const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// A single-row sparkline of `samples`, scaled to the sample max, using the
/// eight vertical block glyphs `▁▂▃▄▅▆▇█`.
///
/// The most recent samples are right-aligned so the graph scrolls left as
/// new data arrives. Bar height (and color) tracks each sample's fraction
/// of the max via [`Meter`]. Only the first row of `area` is drawn.
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

impl<B: Backend> Widget<B> for Sparkline<'_> {
    fn render(self, area: Rect, term: &mut Terminal<B>) {
        let width = area.width_usize();
        if width == 0 {
            return;
        }
        let y = area.top();
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
            let x = area.left() + i as u16;
            if i < pad {
                term.put_styled(x, y, ' ', Style::new());
                continue;
            }
            let ratio = (recent[i - pad] / max).clamp(0.0, 1.0);
            let level = (ratio * 8.0).round() as usize;
            term.put_styled(
                x,
                y,
                BLOCKS[level.min(8)],
                Style::new().fg(Meter::new(ratio).color()),
            );
        }
        term.reset_style();
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::Headless;

    use super::*;

    #[test]
    fn right_aligns_recent_samples_and_pads_the_rest() {
        let area = Rect::new(0, 0, 5, 1);
        let mut term = Terminal::new(Headless::new(5, 1));
        Sparkline::new(&[1.0, 2.0]).render(area, &mut term);

        assert_eq!(term.grid().get(0, 0).glyph(), ' ');
        assert_eq!(term.grid().get(2, 0).glyph(), ' ');
        assert_eq!(term.grid().get(3, 0).glyph(), BLOCKS[4]); // 1.0 / 2.0 -> half
        assert_eq!(term.grid().get(4, 0).glyph(), BLOCKS[8]); // 2.0 / 2.0 -> full
    }

    #[test]
    fn empty_samples_is_a_no_op_beyond_blank_padding() {
        let area = Rect::new(0, 0, 3, 1);
        let mut term = Terminal::new(Headless::new(3, 1));
        Sparkline::new(&[]).render(area, &mut term);
        for x in 0..3 {
            assert_eq!(term.grid().get(x, 0).glyph(), ' ');
        }
    }
}
