//! Time-driven value animation: [`Easing`] curves, a stateful, retargetable [`Tween`], and a
//! periodic [`oscillate`] helper.
//!
//! [`FrameClock`](crate::FrameClock) answers "how many fixed logic steps has this frame's elapsed
//! time earned"; this module answers a different question -- "what's this one `f32` value right
//! now, partway through animating from A to B" (or, for [`oscillate`], partway through an
//! ongoing wave with no start or end). Two tools for two different shapes of motion:
//!
//! - [`Tween`] -- a finite transition from one value to another over a fixed duration, reshaped
//!   by an [`Easing`] curve. Use it for things that start, run once, and stop: a fade-in, a
//!   value settling toward a new target.
//! - [`oscillate`] -- a continuous periodic wave with no start or end. Use it for things that
//!   just keep going: a pulsing indicator, a breathing effect, the demo signal in gallery example
//!   11.
//!
//! Both follow the same explicit, app-owned state convention as
//! [`ListState`](https://docs.rs/retroglyph-widgets/latest/retroglyph_widgets/struct.ListState.html)
//! and [`Interaction`](https://docs.rs/retroglyph-widgets/latest/retroglyph_widgets/struct.Interaction.html):
//! a plain struct the caller constructs and stores itself, updated once per frame with
//! [`Frame::delta`](crate::Frame::delta), rather than a hidden global animation manager keyed by
//! an id the way egui's `Context::animate_value` works. One [`Tween`] animates one `f32`; an app
//! with several needs several `Tween`s, the same as it needs several `ListState`s for several
//! lists.

use core::time::Duration;

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

/// A retargetable animation from one `f32` value to another over a fixed duration, reshaped by
/// an [`Easing`] curve.
///
/// ```
/// use core::time::Duration;
/// use retroglyph_core::{Easing, Tween};
///
/// let mut fade = Tween::new(0.0, 1.0)
///     .duration(Duration::from_millis(200))
///     .easing(Easing::EaseOutCubic);
///
/// fade.update(Duration::from_millis(100)); // halfway through, by elapsed time
/// assert!(fade.value() > 0.5); // EaseOutCubic front-loads motion, so it's already past halfway
/// assert!(!fade.is_finished());
///
/// fade.update(Duration::from_millis(100)); // now fully elapsed
/// assert_eq!(fade.value(), 1.0);
/// assert!(fade.is_finished());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tween {
    from: f32,
    to: f32,
    elapsed: Duration,
    duration: Duration,
    easing: Easing,
}

impl Tween {
    /// [`duration`](Self::duration)'s default if never overridden: 200ms, a typical UI
    /// micro-interaction length -- noticeable, but not sluggish.
    pub const DEFAULT_DURATION: Duration = Duration::from_millis(200);

    /// A new tween animating from `from` to `to` over [`DEFAULT_DURATION`](Self::DEFAULT_DURATION)
    /// with [`Easing::Linear`]. Chain [`duration`](Self::duration)/[`easing`](Self::easing) to
    /// override either, then call [`update`](Self::update) once per frame.
    #[must_use]
    pub const fn new(from: f32, to: f32) -> Self {
        Self {
            from,
            to,
            elapsed: Duration::ZERO,
            duration: Self::DEFAULT_DURATION,
            easing: Easing::Linear,
        }
    }

    /// Overrides the total duration of the animation.
    #[must_use]
    pub const fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Overrides the easing curve.
    #[must_use]
    pub const fn easing(mut self, easing: Easing) -> Self {
        self.easing = easing;
        self
    }

    /// Advances the animation by `dt` -- call once per frame with
    /// [`Frame::delta`](crate::Frame::delta). Clamped to `duration`: calling this after the
    /// animation has already finished is a no-op, not an overshoot into negative "time left."
    pub fn update(&mut self, dt: Duration) {
        self.elapsed = (self.elapsed + dt).min(self.duration);
    }

    /// Linear progress through the animation: `0.0` at the start, `1.0` once
    /// [`is_finished`](Self::is_finished). Doesn't have the easing curve applied yet -- see
    /// [`value`](Self::value) for that.
    #[must_use]
    pub fn progress(&self) -> f32 {
        if self.duration.is_zero() {
            return 1.0;
        }
        self.elapsed.as_secs_f32() / self.duration.as_secs_f32()
    }

    /// The current animated value: [`progress`](Self::progress) run through this tween's
    /// [`Easing`] curve, then used to interpolate between `from` and `to`.
    #[must_use]
    pub fn value(&self) -> f32 {
        let t = self.easing.apply(self.progress());
        libm::fmaf(self.to - self.from, t, self.from)
    }

    /// `true` once [`update`](Self::update) has accumulated at least `duration` of elapsed time.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.elapsed >= self.duration
    }

    /// Redirects the animation toward a new target, smoothly: the current
    /// [`value`](Self::value) becomes the new start, elapsed time resets to zero, and `target`
    /// becomes the new end. `duration`/`easing` are unchanged.
    ///
    /// Calling this repeatedly -- e.g. once every time a pointer re-enters or leaves a hover
    /// rect, faster than any single fade finishes -- never causes a visible snap to some earlier
    /// value: each retarget starts from wherever the animation actually is *right now*, not from
    /// its original `from`.
    pub fn retarget(&mut self, target: f32) {
        self.from = self.value();
        self.to = target;
        self.elapsed = Duration::ZERO;
    }
}

/// A continuous sine wave sampled at `elapsed`, completing one full cycle every `period`, mapped
/// from its natural `-1.0..=1.0` range to `0.0..=1.0`.
///
/// For a finite transition that starts, runs once, and stops, see [`Tween`] instead -- this is
/// for motion with no start or end (a pulsing indicator, a breathing effect): keep accumulating
/// `elapsed` every frame and re-sample.
///
/// ```
/// use core::time::Duration;
/// use retroglyph_core::oscillate;
///
/// let period = Duration::from_secs(2);
/// assert_eq!(oscillate(Duration::ZERO, period), 0.5); // sin(0) == 0, remapped to the midpoint
/// assert!((oscillate(Duration::from_millis(1500), period) - 0.0).abs() < 1e-6); // 3/4 through
/// ```
#[must_use]
pub fn oscillate(elapsed: Duration, period: Duration) -> f32 {
    if period.is_zero() {
        return 0.5;
    }
    let cycles = elapsed.as_secs_f32() / period.as_secs_f32(); // unbounded -- doesn't wrap itself
    let radians = cycles * 2.0 * core::f32::consts::PI;
    libm::fmaf(0.5, libm::sinf(radians), 0.5)
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // exact float equality is intentional throughout: every value
// under test here is produced by simple, exactly-representable arithmetic (0.0, 1.0, halves),
// not an accumulated or transcendental result where an epsilon comparison would be appropriate.
mod tests {
    use super::*;

    // ── Easing ──────────────────────────────────────────────────────────────────

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

    // ── Tween ───────────────────────────────────────────────────────────────────

    #[test]
    fn starts_at_from_and_ends_at_to() {
        let mut tween = Tween::new(10.0, 20.0).duration(Duration::from_millis(100));
        assert_eq!(tween.value(), 10.0);
        assert!(!tween.is_finished());

        tween.update(Duration::from_millis(100));
        assert_eq!(tween.value(), 20.0);
        assert!(tween.is_finished());
    }

    #[test]
    fn update_past_duration_clamps_instead_of_overshooting() {
        let mut tween = Tween::new(0.0, 1.0).duration(Duration::from_millis(100));
        tween.update(Duration::from_millis(500)); // way more than the duration
        assert_eq!(tween.value(), 1.0);
        assert!(tween.is_finished());

        tween.update(Duration::from_millis(500)); // finished tweens stay finished
        assert!(tween.is_finished());
        assert_eq!(tween.value(), 1.0);
    }

    #[test]
    fn easing_reshapes_the_midpoint() {
        let mut linear = Tween::new(0.0, 1.0).duration(Duration::from_millis(100));
        let mut eased = Tween::new(0.0, 1.0)
            .duration(Duration::from_millis(100))
            .easing(Easing::EaseInQuad);

        linear.update(Duration::from_millis(50));
        eased.update(Duration::from_millis(50));

        assert_eq!(linear.value(), 0.5);
        assert!(eased.value() < linear.value()); // EaseInQuad front-loads less motion
    }

    #[test]
    fn zero_duration_finishes_immediately() {
        let tween = Tween::new(0.0, 5.0).duration(Duration::ZERO);
        assert!(tween.is_finished());
        assert_eq!(tween.value(), 5.0);
    }

    #[test]
    fn retarget_starts_from_the_current_value_not_the_original_from() {
        let mut tween = Tween::new(0.0, 10.0).duration(Duration::from_millis(100));
        tween.update(Duration::from_millis(50)); // halfway: value() == 5.0
        assert_eq!(tween.value(), 5.0);

        tween.retarget(20.0);
        // No snap: retargeting mid-flight starts from wherever the tween already was.
        assert_eq!(tween.value(), 5.0);
        assert!(!tween.is_finished());

        tween.update(Duration::from_millis(100));
        assert_eq!(tween.value(), 20.0);
    }

    #[test]
    fn repeated_retargets_never_snap() {
        let mut tween = Tween::new(0.0, 1.0).duration(Duration::from_millis(100));
        tween.update(Duration::from_millis(30));
        let before = tween.value();
        tween.retarget(0.0);
        assert_eq!(tween.value(), before);

        tween.update(Duration::from_millis(10));
        let before = tween.value();
        tween.retarget(1.0);
        assert_eq!(tween.value(), before);
    }

    // ── oscillate ───────────────────────────────────────────────────────────────

    #[test]
    fn oscillate_at_zero_elapsed_is_the_midpoint() {
        // sin(0) == 0, remapped from -1..=1 to 0..=1 -> 0.5.
        assert_eq!(oscillate(Duration::ZERO, Duration::from_secs(1)), 0.5);
    }

    #[test]
    fn oscillate_completes_a_full_cycle_after_one_period() {
        let period = Duration::from_secs(4);
        let start = oscillate(Duration::ZERO, period);
        let one_cycle_later = oscillate(period, period);
        assert!((start - one_cycle_later).abs() < 1e-5);
    }

    #[test]
    fn oscillate_stays_within_0_and_1() {
        let period = Duration::from_secs(1);
        for ms in 0..2000u64 {
            let v = oscillate(Duration::from_millis(ms), period);
            assert!((0.0..=1.0).contains(&v), "{v} out of range at {ms}ms");
        }
    }

    #[test]
    fn zero_period_is_a_defined_constant_not_a_panic() {
        assert_eq!(oscillate(Duration::from_secs(1), Duration::ZERO), 0.5);
    }
}
