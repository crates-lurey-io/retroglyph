//! [`Meter`]: a load ratio mapped to a green→yellow→red color.
use retroglyph_core::Color;

/// A load ratio in `0.0..=1.0`, mapped to a green→yellow→red color ramp.
///
/// Low load is green, mid load yellow, high load red. Values outside the
/// range are clamped. Delegates to [`Color::lerp`] (backed by `gem`) rather
/// than hand-rolling RGB interpolation.
///
/// Not a drawing widget -- there's no [`Terminal`](retroglyph_core::Terminal)
/// involved, just a ratio-to-color mapping -- but kept as its own small
/// struct rather than a free function so [`Gauge`](super::Gauge),
/// [`StatBar`](super::StatBar), and [`Sparkline`](super::Sparkline) share
/// one place that owns the ramp.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Meter {
    ratio: f32,
}

impl Meter {
    const GREEN: Color = Color::Rgb {
        r: 80,
        g: 200,
        b: 120,
    };
    const YELLOW: Color = Color::Rgb {
        r: 220,
        g: 200,
        b: 90,
    };
    const RED: Color = Color::Rgb {
        r: 220,
        g: 90,
        b: 90,
    };

    /// A meter reading `ratio` (clamped to `0.0..=1.0` when colored).
    #[must_use]
    pub const fn new(ratio: f32) -> Self {
        Self { ratio }
    }

    /// The ramped color for this meter's ratio.
    #[must_use]
    pub fn color(self) -> Color {
        let t = self.ratio.clamp(0.0, 1.0);
        if t < 0.5 {
            Color::lerp(Self::GREEN, Self::YELLOW, t * 2.0)
        } else {
            Color::lerp(Self::YELLOW, Self::RED, (t - 0.5) * 2.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_load_is_green() {
        assert_eq!(Meter::new(0.0).color(), Meter::GREEN);
    }

    #[test]
    fn mid_load_is_yellow() {
        assert_eq!(Meter::new(0.5).color(), Meter::YELLOW);
    }

    #[test]
    fn high_load_is_red() {
        assert_eq!(Meter::new(1.0).color(), Meter::RED);
    }

    #[test]
    fn out_of_range_ratios_are_clamped() {
        assert_eq!(Meter::new(-1.0).color(), Meter::GREEN);
        assert_eq!(Meter::new(2.0).color(), Meter::RED);
    }
}
