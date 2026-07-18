//! [`Gauge`]: a labeled, load-colored progress bar.
use retroglyph_core::{Backend, Rect, Style, Terminal};

use super::{Widget, bar};
use crate::Theme;

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

    /// Sets `label_style` to `theme.dim` on `theme.panel_bg` -- the same de-emphasized role
    /// `09_widgets_dashboard` already uses for the plain-text label next to this gauge's
    /// sparkline. The bar's own fill stays load-colored via [`super::Meter`] regardless of
    /// `theme`, matching every other gauge/meter-backed widget here (see [`super::Sparkline`]'s
    /// doc comment for why that coloring is deliberately not part of the [`Theme`] role palette).
    ///
    /// `label_style` sets an explicit background rather than leaving it at [`Style::new()`]'s
    /// default: an unset background isn't "transparent" once a real backend draws it (a bare
    /// `Color::Default` cell paints as solid black behind the glyph -- see
    /// `retroglyph-software`'s `DEFAULT_BG`), so this widget assumes it's drawn on
    /// `theme.panel_bg`, true when composed with a themed [`super::Panel`]/[`super::Modal`].
    /// Drawing this gauge directly on the raw screen background instead needs a manual
    /// `.label_style(...)` override afterwards.
    ///
    /// Call before any manual [`Gauge::label_style`] override you want to keep.
    #[must_use]
    pub fn theme(mut self, theme: Theme) -> Self {
        self.label_style = Style::new().fg(theme.dim).bg(theme.panel_bg);
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

    #[test]
    fn theme_maps_dim_role_onto_label_style() {
        let area = Rect::new(0, 0, 20, 1);
        let mut term = Terminal::new(Headless::new(20, 1));
        Gauge::new("H", 0.5)
            .theme(Theme::DARK)
            .render(area, &mut term);

        assert_eq!(term.grid().get(0, 0).style().foreground(), Theme::DARK.dim);
        assert_eq!(
            term.grid().get(0, 0).style().background(),
            Theme::DARK.panel_bg
        );
    }
}
