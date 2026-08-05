//! [`StatBar`]: a labeled `current`/`max` stat bar.
use core::fmt::Write as _;

use retroglyph_core::color::{Color, Style};

use super::{Widget, bar};
use crate::Surface;
use crate::Theme;

/// A labeled stat bar: `label`, a bar filling `current / max` of the
/// remaining width colored by [`super::Meter`], and a trailing
/// `"current/max"` readout.
///
/// Only the first row of `area` is used. Same layout and coloring as
/// [`super::Gauge`], but for integer `current`/`max` pairs (health, mana,
/// stamina) with a literal readout instead of a percentage: `"45/100"`
/// reads as a stat, not a load. `max == 0` renders as an empty, unfilled bar
/// with a `"0/0"` readout rather than a special-cased blank output. If
/// `current` exceeds `max` (e.g. a temporarily buffed stat), the bar fill
/// still caps at 100%, but the readout shows the true, uncapped numbers
/// (`"120/100"`) so the overflow stays visible in text. `label_style` defaults to
/// [`Theme::DARK`]'s `dim` role (as if [`StatBar::theme`] had been called); set it with
/// [`StatBar::label_style`]. The fill (and readout) color defaults to [`super::Meter`]'s
/// green→yellow→red load ramp, which reads backwards for a health/mana/stamina stat (full
/// renders as danger, not safe); override it with [`StatBar::fill_color`].
///
/// # Examples
///
/// ```
/// use retroglyph_core::grid::{Grid, Rect};
/// use retroglyph_ui::{StatBar, Surface, Widget};
///
/// let area = Rect::new(0, 0, 20, 1);
/// let mut grid = Grid::new(20, 1);
/// StatBar::new("HP", 45, 100).render(&mut Surface::new(&mut grid, area, 0));
/// ```
#[derive(Clone, Copy, Debug)]
pub struct StatBar<'a> {
    label: &'a str,
    current: u32,
    max: u32,
    label_style: Style,
    fill_color: fn(f32) -> Color,
}

impl<'a> StatBar<'a> {
    /// A stat bar for `label`, reading `current` out of `max`, with `label_style` styled from
    /// [`Theme::DARK`] (as if [`StatBar::theme`] had been called).
    #[must_use]
    pub fn new(label: &'a str, current: u32, max: u32) -> Self {
        Self {
            label,
            current,
            max,
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
    /// green→yellow→red load ramp to `fill_color`, called with `current / max` clamped to
    /// `0.0..=1.0`.
    ///
    /// The default ramp reads backwards for a descending stat like health: full renders as
    /// danger (red), not safe. Supply a ramp that fits instead, e.g.
    /// `|r| Color::lerp(Color::RED, Color::GREEN, r)`. A plain `fn` pointer rather than a
    /// boxed closure, so `StatBar` stays `Copy`; a non-capturing closure like the one above
    /// coerces to it automatically.
    #[must_use]
    pub const fn fill_color(mut self, fill_color: fn(f32) -> Color) -> Self {
        self.fill_color = fill_color;
        self
    }

    /// Sets `label_style` to `theme.dim` on `theme.panel_bg`, the same mapping (and the same
    /// "assumes it's drawn on `theme.panel_bg`" caveat) as [`super::Gauge::theme`]; see its
    /// doc comment for the full explanation, including why the bar's own load-colored fill stays
    /// outside `theme`'s role palette, unless overridden separately with [`StatBar::fill_color`].
    ///
    /// Call before any manual [`StatBar::label_style`] override you want to keep.
    #[must_use]
    pub fn theme(self, theme: Theme) -> Self {
        self.theme_on(theme, theme.panel_bg)
    }

    /// Same as [`StatBar::theme`], but `label_style` is drawn on `bg` instead of
    /// `theme.panel_bg`: for a stat bar drawn directly on a backdrop other than a themed
    /// [`super::Panel`]/[`super::Modal`]'s fill. [`StatBar::theme`] is exactly
    /// `theme_on(theme, theme.panel_bg)`.
    #[must_use]
    pub fn theme_on(mut self, theme: Theme, bg: Color) -> Self {
        self.label_style = Style::new().fg(theme.dim).bg(bg);
        self
    }
}

impl Widget for StatBar<'_> {
    fn render(&self, surface: &mut Surface<'_>) {
        let ratio = if self.max == 0 {
            0.0
        } else {
            // `current`/`max` are gauge readouts (health, ammo, ...) for on-screen display; values
            // near `u32::MAX` losing mantissa bits below f32's 2^23 threshold has no visible
            // effect on the rendered ratio.
            #[allow(clippy::cast_precision_loss)]
            {
                self.current as f32 / self.max as f32
            }
        };
        // `"4294967295/4294967295"` (two `u32::MAX`s) is the longest possible output: 21 bytes.
        let mut readout = bar::ReadoutBuf::<24>::new();
        let _ = write!(readout, "{}/{}", self.current, self.max);
        bar::render(
            surface,
            self.label,
            self.label_style,
            ratio,
            readout.as_str(),
            self.fill_color,
        );
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::grid::{Grid, Pos, Rect};

    use super::*;

    #[test]
    fn zero_max_renders_an_empty_bar_and_zero_zero_readout() {
        // 1-char label "H" makes the bar's starting column predictable: it
        // begins right after "H" plus a one-column gap, i.e. at column 2.
        let area = Rect::new(0, 0, 20, 1);
        let mut grid = Grid::new(20, 1);
        StatBar::new("H", 0, 0).render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(2, 0)].glyph(), '░'); // empty bar cell
        assert_eq!(grid[Pos::new(19, 0)].glyph(), '0'); // last char of "0/0"
    }

    #[test]
    fn normal_case_fills_proportionally_and_shows_current_over_max() {
        let area = Rect::new(0, 0, 20, 1);
        let mut grid = Grid::new(20, 1);
        StatBar::new("H", 45, 100).render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(2, 0)].glyph(), '█'); // bar starts filled
        assert_eq!(grid[Pos::new(19, 0)].glyph(), '0'); // last char of "45/100"
    }

    #[test]
    fn over_max_caps_the_bar_but_shows_true_numbers_in_the_readout() {
        let area = Rect::new(0, 0, 20, 1);
        let mut grid = Grid::new(20, 1);
        StatBar::new("H", 150, 100).render(&mut Surface::new(&mut grid, area, 0));

        // Bar's last cell before the gap+readout is fully filled (clamped
        // to 100%), but the readout still reads the true "150/100".
        assert_eq!(grid[Pos::new(11, 0)].glyph(), '█');
        assert_eq!(grid[Pos::new(19, 0)].glyph(), '0'); // last char of "150/100"
    }

    #[test]
    fn label_style_is_configurable() {
        use retroglyph_core::color::Color;

        let area = Rect::new(0, 0, 20, 1);
        let mut grid = Grid::new(20, 1);
        StatBar::new("H", 45, 100)
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
        StatBar::new("H", 100, 100)
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
        StatBar::new("H", 45, 100)
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
        let area = Rect::new(0, 0, 20, 1);
        let mut grid = Grid::new(20, 1);
        StatBar::new("H", 45, 100)
            .theme_on(Theme::DARK, Color::Default)
            .render(&mut Surface::new(&mut grid, area, 0));

        assert_eq!(grid[Pos::new(0, 0)].style().foreground(), Theme::DARK.dim);
        assert_eq!(grid[Pos::new(0, 0)].style().background(), Color::Default);
    }
}
