use core::time::Duration;

/// A continuous sine wave sampled at `elapsed`, completing one full cycle every `period`, mapped
/// from its natural `-1.0..=1.0` range to `0.0..=1.0`.
///
/// For a finite transition that starts, runs once, and stops, see [`Tween`](super::Tween)
/// instead. This is for motion with no start or end (a pulsing indicator, a breathing effect):
/// keep accumulating `elapsed` every frame and re-sample.
///
/// Equivalent to [`oscillate_with_phase`](crate::animate::oscillate_with_phase) with `phase` `0.0`. Two callers driving independent
/// pulses (say, a row of status dots) that both call plain `oscillate` with the same `elapsed`
/// and `period` will always be perfectly in sync; use [`oscillate_with_phase`](crate::animate::oscillate_with_phase) to stagger them.
///
/// ```
/// use core::time::Duration;
/// use retroglyph_ui::animate::oscillate;
///
/// let period = Duration::from_secs(2);
/// assert_eq!(oscillate(Duration::ZERO, period), 0.5); // sin(0) == 0, remapped to the midpoint
/// assert!((oscillate(Duration::from_millis(1500), period) - 0.0).abs() < 1e-6); // 3/4 through
/// ```
#[must_use]
pub fn oscillate(elapsed: Duration, period: Duration) -> f32 {
    oscillate_with_phase(elapsed, period, 0.0)
}

/// [`oscillate`](crate::animate::oscillate), offset by `phase` full cycles.
///
/// `phase` shifts where in the cycle sampling starts: `0.0` matches plain [`oscillate`](crate::animate::oscillate), `0.25`
/// starts a quarter-cycle ahead, and so on. Only `phase`'s fractional part matters (a whole
/// number of cycles is no offset at all), and negative values are fine. This is what lets
/// several callers share one clock's `elapsed` and `period` while still pulsing out of sync with
/// each other, e.g. a row of status dots each given a different `phase`.
///
/// ```
/// use core::time::Duration;
/// use retroglyph_ui::animate::oscillate_with_phase;
///
/// let period = Duration::from_secs(2);
/// // A quarter cycle ahead of `elapsed = 0` is the same as `elapsed = period / 4` with no phase.
/// let a = oscillate_with_phase(Duration::ZERO, period, 0.25);
/// let b = oscillate_with_phase(Duration::from_millis(500), period, 0.0);
/// assert!((a - b).abs() < 1e-6);
/// ```
#[must_use]
pub fn oscillate_with_phase(elapsed: Duration, period: Duration, phase: f32) -> f32 {
    if period.is_zero() {
        return 0.5;
    }
    let cycles = elapsed.as_secs_f32() / period.as_secs_f32() + phase; // unbounded; doesn't wrap
    let radians = cycles * 2.0 * core::f32::consts::PI;
    retroglyph_core::math::mul_add(0.5, retroglyph_core::math::sin(radians), 0.5)
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

    #[test]
    fn zero_phase_matches_plain_oscillate() {
        let period = Duration::from_secs(1);
        for ms in 0..2000u64 {
            let elapsed = Duration::from_millis(ms);
            assert_eq!(
                oscillate_with_phase(elapsed, period, 0.0),
                oscillate(elapsed, period)
            );
        }
    }

    #[test]
    fn phase_offset_shifts_the_sampled_point_in_the_cycle() {
        let period = Duration::from_secs(2);
        let a = oscillate_with_phase(Duration::ZERO, period, 0.25);
        let b = oscillate_with_phase(Duration::from_millis(500), period, 0.0);
        assert!((a - b).abs() < 1e-6);
    }

    #[test]
    fn phase_offset_by_a_whole_number_of_cycles_is_no_offset() {
        let period = Duration::from_secs(1);
        let elapsed = Duration::from_millis(137);
        let a = oscillate_with_phase(elapsed, period, 0.0);
        let b = oscillate_with_phase(elapsed, period, 3.0);
        assert!((a - b).abs() < 1e-5);
    }

    #[test]
    fn negative_phase_is_defined() {
        let period = Duration::from_secs(2);
        let a = oscillate_with_phase(Duration::from_millis(500), period, -0.25);
        let b = oscillate_with_phase(Duration::ZERO, period, 0.0);
        assert!((a - b).abs() < 1e-6);
    }

    #[test]
    fn phase_offset_stays_within_0_and_1() {
        let period = Duration::from_secs(1);
        for ms in 0..2000u64 {
            let v = oscillate_with_phase(Duration::from_millis(ms), period, 0.37);
            assert!((0.0..=1.0).contains(&v), "{v} out of range at {ms}ms");
        }
    }

    #[test]
    fn zero_period_with_phase_is_a_defined_constant_not_a_panic() {
        assert_eq!(
            oscillate_with_phase(Duration::from_secs(1), Duration::ZERO, 0.25),
            0.5
        );
    }
}
