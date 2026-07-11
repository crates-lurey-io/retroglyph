//! [`Tween`]: a finite, retargetable transition between two `f32` values.

use super::easing::Easing;
use core::time::Duration;

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

#[cfg(test)]
#[allow(clippy::float_cmp)] // exact float equality is intentional throughout: every value
// under test here is produced by simple, exactly-representable arithmetic (0.0, 1.0, halves),
// not an accumulated or transcendental result where an epsilon comparison would be appropriate.
mod tests {
    use super::*;

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
}
