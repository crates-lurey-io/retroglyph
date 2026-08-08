//! [`Sparkline`]: a single-row bar chart of recent samples.
use retroglyph_core::color::{Color, Style};
use retroglyph_core::symbols::bar::NINE_LEVELS;

use super::{Meter, Widget};
use crate::Surface;

/// A single-row sparkline of `samples`, scaled to the sample max, using the
/// eight vertical block glyphs `▁▂▃▄▅▆▇█` by default (see [`Sparkline::glyphs`] to pick a
/// different ramp).
///
/// The most recent samples are right-aligned so the graph scrolls left as
/// new data arrives. By default, bar height *and color* track each sample's
/// fraction of the max via [`Meter`] (a green-to-red load ramp). Two ways to override the color:
/// [`Sparkline::style`] draws every bar in one fixed color instead, height only, the right choice
/// once the color channel would otherwise imply something the data doesn't mean (e.g. a
/// frame-time graph, where "tallest bar in view" isn't the same thing as "bad":
/// [`super::PerfOverlay`] does this); [`Sparkline::fill_color`] instead swaps [`Meter`]'s ramp for
/// a caller-supplied one, e.g. for a bar where high isn't bad (matching
/// [`super::Gauge::fill_color`]/[`super::StatBar::fill_color`]). If both are set, `style` wins,
/// since it also carries a background color that a color-only ramp can't express. Only the first
/// row of `area` is drawn.
///
/// Unlike [`super::BoxBorder`], [`super::Gauge`], [`super::StatBar`],
/// [`super::Table`], and [`super::Button`], `Sparkline` has no `theme()`/
/// `theme_on()` pair: [`Meter`] already gives its bars a semantic
/// green-to-red color, and a fixed [`Sparkline::style`] override has no
/// single [`Theme`](crate::Theme) role to map onto either.
///
/// # Examples
///
/// ```
/// use retroglyph_core::grid::{Grid, Rect};
/// use retroglyph_ui::{Surface, Sparkline, Widget};
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
    fill_color: Option<fn(f32) -> Color>,
    glyphs: &'a [char],
}

impl<'a> Sparkline<'a> {
    /// A sparkline of `samples`, colored by [`Meter`] (green-to-red load ramp) unless overridden
    /// with [`style`](Self::style) or [`fill_color`](Self::fill_color), drawn with the eighth-block
    /// glyph ramp [`NINE_LEVELS`](retroglyph_core::symbols::bar::NINE_LEVELS) unless overridden
    /// with [`glyphs`](Self::glyphs).
    #[must_use]
    pub const fn new(samples: &'a [f32]) -> Self {
        Self {
            samples,
            style: None,
            fill_color: None,
            glyphs: &NINE_LEVELS,
        }
    }

    /// Draws every bar in `style`'s foreground color instead of the default [`Meter`] ramp.
    /// Height still tracks each sample's fraction of the max; only the color stops varying.
    ///
    /// Takes priority over [`Sparkline::fill_color`] if both are set.
    #[must_use]
    pub const fn style(mut self, style: Style) -> Self {
        self.style = Some(style);
        self
    }

    /// Overrides each bar's color from [`Meter`]'s default green→yellow→red load ramp to
    /// `fill_color`, called with the bar's clamped `0.0..=1.0` fraction of the sample max. Height
    /// still tracks that same fraction.
    ///
    /// Matches [`super::Gauge::fill_color`]/[`super::StatBar::fill_color`]'s shape: for a bar
    /// where high isn't bad (e.g. a health-derived sparkline, where full should read safe, not
    /// danger), supply a ramp that fits, e.g. `|r| Color::lerp(Color::RED, Color::GREEN, r)`. A
    /// plain `fn` pointer rather than a boxed closure, so `Sparkline` stays `Copy`; a
    /// non-capturing closure like the one above coerces to it automatically.
    ///
    /// Ignored if [`Sparkline::style`] is also set: `style` also carries a background color that
    /// a color-only ramp can't express, so it wins.
    #[must_use]
    pub const fn fill_color(mut self, fill_color: fn(f32) -> Color) -> Self {
        self.fill_color = Some(fill_color);
        self
    }

    /// Overrides the glyph ramp from the default eighth blocks
    /// ([`NINE_LEVELS`](retroglyph_core::symbols::bar::NINE_LEVELS), `' '▁▂▃▄▅▆▇█`) to `glyphs`,
    /// indexed from empty (`glyphs[0]`) to full (`glyphs[glyphs.len() - 1]`).
    ///
    /// Six of the eight eighth-block glyphs are outside CP437: a pixel backend that resolves
    /// glyphs through a CP437 table draws its fallback solid block for all of them, and a
    /// tileset/sprite backend can't tint an arbitrary glyph by [`Style::fg`]. On either backend,
    /// pass [`SHADE_LEVELS`](retroglyph_core::symbols::bar::SHADE_LEVELS) (`' '░▒▓█`, all within
    /// CP437) for a ramp that still renders five distinguishable levels instead of collapsing to
    /// one block. `glyphs` must be non-empty; an empty slice falls back to a single space.
    #[must_use]
    pub const fn glyphs(mut self, glyphs: &'a [char]) -> Self {
        self.glyphs = glyphs;
        self
    }
}

impl Widget for Sparkline<'_> {
    fn render(&self, surface: &mut Surface<'_>) {
        let width = usize::from(surface.width());
        if width == 0 {
            return;
        }
        // Floor the max away from zero so an all-zero (or empty) sample window scales to a flat
        // row of blanks instead of dividing by zero (`sample / 0.0` is NaN, which would break the
        // clamp and the level cast below). 1e-6 is small enough to never perturb a real max.
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

        // An empty `glyphs` override has no "full" glyph to fall back to, so treat it as a single
        // blank level rather than panicking on the `glyphs.len() - 1` top index below.
        let glyphs: &[char] = if self.glyphs.is_empty() {
            &[' ']
        } else {
            self.glyphs
        };
        let top = glyphs.len() - 1;

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
            // `glyphs` is indexed `0..=top` (empty to full), so a clamped ratio maps to a level by
            // scaling to the top index and rounding. `.min(top)` is belt-and-suspenders against a
            // rounding result of exactly `top` (it never exceeds it, given the clamp above).
            // `top` is a glyph-ramp length, always tiny relative to `f32`'s 24-bit mantissa.
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            let level = retroglyph_core::math::round(ratio * top as f32) as usize;
            let color = self
                .fill_color
                .map_or_else(|| Meter::new(ratio).color(), |fill_color| fill_color(ratio));
            let style = self.style.unwrap_or_else(|| Style::new().fg(color));
            surface.put((x, 0), glyphs[level.min(top)], style);
        }
    }
}

#[cfg(test)]
mod tests {
    use retroglyph_core::grid::{Grid, Pos, Rect};

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
        use retroglyph_core::color::Color;

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

    #[test]
    fn fill_color_overrides_the_default_meter_ramp() {
        use retroglyph_core::color::Color;

        fn white_to_red(ratio: f32) -> Color {
            Color::lerp(Color::WHITE, Color::RED, ratio)
        }

        let area = Rect::new(0, 0, 3, 1);
        let mut grid = Grid::new(3, 1);
        Sparkline::new(&[1.0, 4.0, 2.0])
            .fill_color(white_to_red)
            .render(&mut Surface::new(&mut grid, area, 0));

        // Full ratio (the `4.0` max): the default ramp would also be red-ish here, so assert
        // against the custom ramp's own output rather than a color literal that might coincide.
        assert_eq!(grid[Pos::new(1, 0)].style().foreground(), white_to_red(1.0));
    }

    #[test]
    fn style_wins_over_fill_color_when_both_are_set() {
        use retroglyph_core::color::Color;

        fn white_to_red(ratio: f32) -> Color {
            Color::lerp(Color::WHITE, Color::RED, ratio)
        }
        let accent = Style::new().fg(Color::Rgb {
            r: 90,
            g: 170,
            b: 250,
        });

        let area = Rect::new(0, 0, 3, 1);
        let mut grid = Grid::new(3, 1);
        Sparkline::new(&[1.0, 4.0, 2.0])
            .fill_color(white_to_red)
            .style(accent)
            .render(&mut Surface::new(&mut grid, area, 0));

        for x in 0..3 {
            assert_eq!(grid[Pos::new(x, 0)].style(), accent);
        }
    }

    #[test]
    fn glyphs_overrides_the_default_eighth_block_ramp() {
        use retroglyph_core::symbols::bar::SHADE_LEVELS;

        let area = Rect::new(0, 0, 3, 1);
        let mut grid = Grid::new(3, 1);
        Sparkline::new(&[1.0, 4.0, 2.0])
            .glyphs(&SHADE_LEVELS)
            .render(&mut Surface::new(&mut grid, area, 0));

        // `4.0`'s ratio is 1.0 (the max), so it maps to `SHADE_LEVELS`' top index, `FULL`; none of
        // the rendered glyphs fall outside the CP437-safe ramp.
        assert_eq!(grid[Pos::new(1, 0)].glyph(), SHADE_LEVELS[4]);
        for x in 0..3 {
            assert!(SHADE_LEVELS.contains(&grid[Pos::new(x, 0)].glyph()));
        }
    }

    #[test]
    fn empty_glyphs_falls_back_to_a_single_blank_level() {
        let area = Rect::new(0, 0, 3, 1);
        let mut grid = Grid::new(3, 1);
        Sparkline::new(&[1.0, 4.0, 2.0])
            .glyphs(&[])
            .render(&mut Surface::new(&mut grid, area, 0));

        for x in 0..3 {
            assert_eq!(grid[Pos::new(x, 0)].glyph(), ' ');
        }
    }
}
