//! [`Easing`]: normalized curves reshaping linear progress into eased motion.

/// A normalized easing curve: reshapes a linear progress fraction (`0.0..=1.0`) into an eased
/// one, the same named curves as CSS transitions and <https://easings.net>.
///
/// [`Linear`](Self::Linear) is the default. The `In` variants start slow, `Out` variants end
/// slow, and `InOut` variants do both (matching the usual naming convention: "In" describes the
/// *start* of the motion, not a direction).
///
/// [`EaseOutElastic`](Self::EaseOutElastic) and [`EaseOutBounce`](Self::EaseOutBounce) are the
/// only curves in their families: both are used for a settle/overshoot effect at the *end* of a
/// motion, and the in/in-out variants (the same shape mirrored to the start) are uncommon enough
/// in practice that this curated set omits them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Easing {
    /// Constant speed: `t` unchanged.
    #[default]
    Linear,
    /// Starts slow, accelerates, quadratically.
    EaseInQuad,
    /// Starts fast, decelerates, quadratically.
    EaseOutQuad,
    /// Slow -> fast -> slow, quadratically.
    EaseInOutQuad,
    /// Starts slow, accelerates, cubically -- a stronger version of [`EaseInQuad`](Self::EaseInQuad).
    EaseInCubic,
    /// Starts fast, decelerates, cubically -- a stronger version of [`EaseOutQuad`](Self::EaseOutQuad).
    EaseOutCubic,
    /// Slow -> fast -> slow, cubically -- a stronger version of [`EaseInOutQuad`](Self::EaseInOutQuad).
    EaseInOutCubic,
    /// A gentle sine-shaped start.
    EaseInSine,
    /// A gentle sine-shaped end.
    EaseOutSine,
    /// A gentle sine-shaped start and end.
    EaseInOutSine,
    /// Springs past the target and oscillates back before settling, going outside `0.0..=1.0`
    /// for part of the curve.
    EaseOutElastic,
    /// Bounces (like a dropped ball) to a stop at the target.
    EaseOutBounce,
}

impl Easing {
    /// Applies this curve to `t` (clamped to `0.0..=1.0` first), returning the eased fraction.
    #[must_use]
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::EaseInQuad => t * t,
            // Not `f32::mul_add`: it's a std-only inherent method, not in `core`. `libm::fmaf`
            // is the no_std-safe equivalent (and this crate already depends on libm for the
            // trig curves below).
            Self::EaseOutQuad => libm::fmaf(t, -t, 2.0 * t),
            Self::EaseInOutQuad => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    let u = libm::fmaf(-2.0, t, 2.0);
                    1.0 - u * u / 2.0
                }
            }
            Self::EaseInCubic => t * t * t,
            Self::EaseOutCubic => {
                let u = 1.0 - t;
                libm::fmaf(u * u, -u, 1.0)
            }
            Self::EaseInOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    let u = libm::fmaf(-2.0, t, 2.0);
                    1.0 - u * u * u / 2.0
                }
            }
            Self::EaseInSine => 1.0 - libm::cosf(t * core::f32::consts::FRAC_PI_2),
            Self::EaseOutSine => libm::sinf(t * core::f32::consts::FRAC_PI_2),
            Self::EaseInOutSine => -(libm::cosf(core::f32::consts::PI * t) - 1.0) / 2.0,
            Self::EaseOutElastic => ease_out_elastic(t),
            Self::EaseOutBounce => ease_out_bounce(t),
        }
    }
}

/// `t * (10 * t - 10.75) * (2 pi / 3)`'s sine, decayed by `2^(-10t)` -- see
/// <https://easings.net/#easeOutElastic>.
fn ease_out_elastic(t: f32) -> f32 {
    const C4: f32 = 2.0 * core::f32::consts::PI / 3.0;

    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }
    libm::fmaf(
        libm::powf(2.0, -10.0 * t),
        libm::sinf(libm::fmaf(10.0, t, -0.75) * C4),
        1.0,
    )
}

/// Four piecewise quadratic segments, each bouncing to a smaller peak -- see
/// <https://easings.net/#easeOutBounce>.
fn ease_out_bounce(t: f32) -> f32 {
    const N1: f32 = 7.5625;
    const D1: f32 = 2.75;
    if t < 1.0 / D1 {
        N1 * t * t
    } else if t < 2.0 / D1 {
        let t = t - 1.5 / D1;
        libm::fmaf(N1 * t, t, 0.75)
    } else if t < 2.5 / D1 {
        let t = t - 2.25 / D1;
        libm::fmaf(N1 * t, t, 0.9375)
    } else {
        let t = t - 2.625 / D1;
        libm::fmaf(N1 * t, t, 0.984_375)
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // exact float equality is intentional throughout: every value
// under test here is produced by simple, exactly-representable arithmetic (0.0, 1.0, halves),
// not an accumulated or transcendental result where an epsilon comparison would be appropriate.
mod tests {
    use super::*;

    #[test]
    fn linear_is_identity() {
        assert_eq!(Easing::Linear.apply(0.0), 0.0);
        assert_eq!(Easing::Linear.apply(0.5), 0.5);
        assert_eq!(Easing::Linear.apply(1.0), 1.0);
    }

    #[test]
    fn every_curve_starts_at_0_and_ends_at_1() {
        for easing in [
            Easing::Linear,
            Easing::EaseInQuad,
            Easing::EaseOutQuad,
            Easing::EaseInOutQuad,
            Easing::EaseInCubic,
            Easing::EaseOutCubic,
            Easing::EaseInOutCubic,
            Easing::EaseInSine,
            Easing::EaseOutSine,
            Easing::EaseInOutSine,
            Easing::EaseOutElastic,
            Easing::EaseOutBounce,
        ] {
            assert!(
                (easing.apply(0.0) - 0.0).abs() < 1e-5,
                "{easing:?} should start at 0"
            );
            assert!(
                (easing.apply(1.0) - 1.0).abs() < 1e-5,
                "{easing:?} should end at 1"
            );
        }
    }

    #[test]
    fn ease_in_quad_starts_slower_than_linear() {
        // "In" curves front-load less motion than linear during the first half.
        assert!(Easing::EaseInQuad.apply(0.25) < 0.25);
    }

    #[test]
    fn ease_out_quad_starts_faster_than_linear() {
        assert!(Easing::EaseOutQuad.apply(0.25) > 0.25);
    }

    #[test]
    fn out_of_range_input_is_clamped() {
        assert_eq!(Easing::Linear.apply(-1.0), 0.0);
        assert_eq!(Easing::Linear.apply(2.0), 1.0);
    }

    #[test]
    #[allow(clippy::cast_precision_loss)] // i in 0..100 is always exactly representable in f32
    fn elastic_overshoots_past_the_target() {
        // The defining feature of an elastic curve: some t produces a value outside 0..=1.
        let overshoots = (0..100)
            .map(|i| Easing::EaseOutElastic.apply(i as f32 / 100.0))
            .any(|v| !(0.0..=1.0).contains(&v));
        assert!(overshoots);
    }
}
