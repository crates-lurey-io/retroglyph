//! [`oscillate`]: a continuous periodic wave with no start or end.

use core::time::Duration;

/// A continuous sine wave sampled at `elapsed`, completing one full cycle every `period`, mapped
/// from its natural `-1.0..=1.0` range to `0.0..=1.0`.
///
/// For a finite transition that starts, runs once, and stops, see [`Tween`](super::Tween)
/// instead -- this is for motion with no start or end (a pulsing indicator, a breathing effect):
/// keep accumulating `elapsed` every frame and re-sample.
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
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

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
