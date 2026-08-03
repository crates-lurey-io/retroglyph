//! [`Gauge`]: a labeled, load-colored progress bar.
use core::fmt::Write as _;

use retroglyph_core::{Color, Style};

use super::{Widget, bar};
use crate::Surface;
use crate::Theme;

/// A labeled gauge: a `label`, then a bar filling `ratio` (0.0-1.0) of the
/// remaining width, colored by [`super::Meter`], with a trailing percentage.
///
/// Only the first row of `area` is used. Generalizes
/// [`ProgressBar`](super::ProgressBar) with a load-colored fill and inline
/// label/readout. For a `current`/`max` integer stat (health, mana) rather
/// than a `0.0..=1.0` load ratio, see [`super::StatBar`]. `label_style` defaults to
/// [`Theme::DARK`]'s `dim` role (as if [`Gauge::theme`] had been called); set it with
/// [`Gauge::label_style`]. The fill (and readout) color defaults to [`super::Meter`]'s
/// green→yellow→red load ramp; override it with [`Gauge::fill_color`] for a gauge that isn't
/// load-shaped (e.g. a health bar, where full should read as safe, not danger).
///
/// # Examples
///
/// ```
/// use retroglyph_core::{Grid, Rect};
/// use retroglyph_widgets::{Gauge, Surface, Widget};
///
/// let area = Rect::new(0, 0, 20, 1);
/// let mut grid = Grid::new(20, 1);
/// Gauge::new("CPU", 0.75).render(&mut Surface::new(&mut grid, area, 0));
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Gauge<'a> {
    label: &'a str,
    ratio: f32,
    label_style: Style,
    fill_color: fn(f32) -> Color,
}

impl<'a> Gauge<'a> {
    /// A gauge for `label`, filled to `ratio` (0.0-1.0), with `label_style` styled from
    /// [`Theme::DARK`] (as if [`Gauge::theme`] had been called).
    #[must_use]
    pub fn new(label: &'a str, ratio: f32) -> Self {
        Self {
            label,
            ratio,
            label_style: Style::new(),
            fill_color: bar::meter_fill_color,
        }
        .theme(Theme::DARK)
    }

    /// Set the label's style.
    #[must_use]
    pub const fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }

    /// Overrides the bar's fill (and readout text) color from [`super::Meter`]'s default
    /// green→yellow→red load ramp to `fill_color`, called with the clamped `0.0..=1.0` ratio.
    ///
    /// For gauges that aren't load-shaped, where the default ramp would read backwards (a full
    /// health bar reading as danger rather than safe): supply a ramp that fits, e.g.
    /// `|r| Color::lerp(Color::RED, Color::GREEN, r)`. A plain `fn` pointer rather than a
    /// boxed closure, so `Gauge` stays `Copy`; a non-capturing closure like the one above
    /// coerces to it automatically.
    #[must_use]
    pub const fn fill_color(mut self, fill_color: fn(f32) -> Color) -> Self {
        self.fill_color = fill_color;
        self
    }

    /// Sets `label_style` to `theme.dim` on `theme.panel_bg`: the same de-emphasized role
    /// `09_widgets_dashboard` already uses for the plain-text label next to this gauge's
    /// sparkline. The bar's own fill stays load-colored via [`super::Meter`] regardless of
    /// `theme` (matching every other gauge/meter-backed widget here; see [`super::Sparkline`]'s
    /// doc comment for why that coloring is not part of the [`Theme`] role palette), unless
    /// overridden separately with [`Gauge::fill_color`].
    ///
    /// `label_style` sets an explicit background rather than leaving it at [`Style::new()`]'s
    /// default: an unset background isn't "transparent" once a real backend draws it (a bare
    /// `Color::Default` cell paints as solid black behind the glyph; see
    /// `retroglyph-software`'s `DEFAULT_BG`), so this widget assumes it's drawn on
    /// `theme.panel_bg`, true when composed with a themed [`super::Panel`]/[`super::Modal`].
    /// Drawing this gauge directly on the raw screen background instead needs a manual
    /// `.label_style(...)` override afterwards.
    ///
    /// Call before any manual [`Gauge::label_style`] override you want to keep.
    #[must_use]
    pub fn theme(self, theme: Theme) -> Self {
        self.theme_on(theme, theme.panel_bg)
    }

    /// Same as [`Gauge::theme`], but `label_style` is drawn on `bg` instead of `theme.panel_bg`,
    /// for a gauge drawn directly on a backdrop other than a themed [`super::Panel`]/
    /// [`super::Modal`]'s fill. [`Gauge::theme`] is exactly `theme_on(theme, theme.panel_bg)`.
    #[must_use]
    pub fn theme_on(mut self, theme: Theme, bg: Color) -> Self {
        self.label_style = Style::new().fg(theme.dim).bg(bg);
        self
    }
}

impl Widget for Gauge<'_> {
    fn render(&self, surface: &mut Surface<'_>) {
        let ratio = self.ratio.clamp(0.0, 1.0);
        // "100%" is the longest possible output: 4 bytes.
        let mut pct = bar::ReadoutBuf::<4>::new();
        // `ratio` is clamped to `0.0..=1.0` above, so the rounded percentage always lands in
        // `0..=100`, well within `i32`'s range.
        #[allow(clippy::cast_possible_truncation)]
        let pct_value = retroglyph_core::math::round(ratio * 100.0) as i32;
        let _ = write!(pct, "{pct_value:>3}%");
        bar::render(
            surface,
            self.label,
            self.label_style,
            ratio,
            pct.as_str(),
            self.fill_color,
        );
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::{Grid, Pos, Rect};

    use super::*;

    #[test]
    fn label_bar_and_percentage_readout() {
        let area = Rect::new(0, 0, 20, 1);
        let mut grid = Grid::new(20, 1);
        Gauge::new("H", 0.5).render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(2, 0)].glyph(), '█'); // bar starts filled
        assert_eq!(grid[Pos::new(19, 0)].glyph(), '%'); // "XX%"-style readout
    }

    #[test]
    fn label_style_is_configurable() {
        use retroglyph_core::Color;

        let area = Rect::new(0, 0, 20, 1);
        let mut grid = Grid::new(20, 1);
        Gauge::new("H", 0.5)
            .label_style(Style::new().fg(Color::WHITE))
            .render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 0)].style().foreground(), Color::WHITE);
    }

    #[test]
    fn fill_color_overrides_the_default_meter_ramp() {
        fn white_to_red(ratio: f32) -> Color {
            Color::lerp(Color::WHITE, Color::RED, ratio)
        }

        let area = Rect::new(0, 0, 20, 1);
        let mut grid = Grid::new(20, 1);
        Gauge::new("H", 1.0)
            .fill_color(white_to_red)
            .render(&mut Surface::new(&mut grid, area, 0));

        // Full ratio: the default ramp would also be red-ish here, so assert against the
        // custom ramp's own output rather than a color literal that might coincide.
        assert_eq!(grid[Pos::new(2, 0)].style().foreground(), white_to_red(1.0));
    }

    #[test]
    fn theme_maps_dim_role_onto_label_style() {
        let area = Rect::new(0, 0, 20, 1);
        let mut grid = Grid::new(20, 1);
        Gauge::new("H", 0.5)
            .theme(Theme::DARK)
            .render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 0)].style().foreground(), Theme::DARK.dim);
        assert_eq!(
            grid[Pos::new(0, 0)].style().background(),
            Theme::DARK.panel_bg
        );
    }

    #[test]
    fn theme_on_uses_the_given_backdrop_instead_of_panel_bg() {
        use retroglyph_core::Color;

        let area = Rect::new(0, 0, 20, 1);
        let mut grid = Grid::new(20, 1);
        Gauge::new("H", 0.5)
            .theme_on(Theme::DARK, Color::Default)
            .render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 0)].style().foreground(), Theme::DARK.dim);
        assert_eq!(grid[Pos::new(0, 0)].style().background(), Color::Default);
    }
}
