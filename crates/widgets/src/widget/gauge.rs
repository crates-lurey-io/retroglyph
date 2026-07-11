//! [`Gauge`]: a labeled, load-colored progress bar.
use retroglyph_core::{Backend, Rect, Style, Terminal};

use super::{Widget, bar};

/// A labeled gauge: a `label`, then a bar filling `ratio` (0.0-1.0) of the
/// remaining width, colored by [`super::Meter`], with a trailing percentage.
///
/// Only the first row of `area` is used. Generalizes
/// [`ProgressBar`](super::ProgressBar) with a load-colored fill and inline
/// label/readout. For a `current`/`max` integer stat (health, mana) rather
/// than a `0.0..=1.0` load ratio, see [`super::StatBar`]. `label_style`
/// defaults to a neutral gray-blue; set it with [`Gauge::label_style`].
#[derive(Clone, Copy, Debug)]
pub struct Gauge<'a> {
    label: &'a str,
    ratio: f32,
    label_style: Style,
}

impl<'a> Gauge<'a> {
    /// A gauge for `label`, filled to `ratio` (0.0-1.0).
    #[must_use]
    pub fn new(label: &'a str, ratio: f32) -> Self {
        Self {
            label,
            ratio,
            label_style: bar::default_label_style(),
        }
    }

    /// Set the label's style.
    #[must_use]
    pub const fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }
}

impl<B: Backend> Widget<B> for Gauge<'_> {
    fn render(self, area: Rect, term: &mut Terminal<B>) {
        let ratio = self.ratio.clamp(0.0, 1.0);
        let pct = format!("{:>3}%", (ratio * 100.0).round() as i32);
        bar::render(term, area, self.label, self.label_style, ratio, &pct);
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::Headless;

    use super::*;

    #[test]
    fn label_bar_and_percentage_readout() {
        let area = Rect::new(0, 0, 20, 1);
        let mut term = Terminal::new(Headless::new(20, 1));
        Gauge::new("H", 0.5).render(area, &mut term);

        assert_eq!(term.grid().get(2, 0).glyph(), '█'); // bar starts filled
        assert_eq!(term.grid().get(19, 0).glyph(), '%'); // "XX%"-style readout
    }

    #[test]
    fn label_style_is_configurable() {
        use retroglyph_core::Color;

        let area = Rect::new(0, 0, 20, 1);
        let mut term = Terminal::new(Headless::new(20, 1));
        Gauge::new("H", 0.5)
            .label_style(Style::new().fg(Color::WHITE))
            .render(area, &mut term);

        assert_eq!(term.grid().get(0, 0).style().foreground(), Color::WHITE);
    }
}
