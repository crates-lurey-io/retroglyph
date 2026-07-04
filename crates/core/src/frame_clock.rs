//! Fixed-timestep accumulator.
//!
//! `FrameClock` decouples logic updates (a stable, fixed rate) from rendering
//! (as fast as the display allows). It is a *pure accumulator*: it never reads a
//! clock itself. The driver supplies elapsed wall time via
//! [`Frame::dt`](crate::Frame), which keeps `FrameClock` `no_std`-clean and
//! platform-agnostic (including wasm, where there is no `std::time::Instant`).
//!
//! # Example
//!
//! ```
//! use core::time::Duration;
//! use retroglyph_core::FrameClock;
//!
//! let mut clock = FrameClock::new(100); // 100 logic updates per second (10 ms)
//!
//! // Once per rendered frame, feed the elapsed time then drain pending steps:
//! clock.advance(Duration::from_millis(35));
//! let mut steps = 0;
//! while clock.tick() {
//!     steps += 1; // run one fixed logic update
//! }
//! assert_eq!(steps, 3); // 35 ms at 100 Hz = 3 whole steps (5 ms remainder)
//! ```

use core::time::Duration;

/// A fixed-timestep accumulator.
///
/// Feed elapsed wall time with [`advance`](Self::advance), then call
/// [`tick`](Self::tick) in a loop to drain whole logic steps. Use
/// [`alpha`](Self::alpha) to interpolate rendering between logic frames.
#[derive(Debug, Clone)]
pub struct FrameClock {
    step: Duration,
    accumulator: Duration,
    max_accumulate: Duration,
}

impl FrameClock {
    /// Create an accumulator targeting `hz` logic updates per second.
    ///
    /// Catch-up is capped at five steps per frame to avoid a "spiral of death"
    /// when logic temporarily runs slower than real time.
    ///
    /// # Panics
    ///
    /// Panics if `hz` is zero.
    #[must_use]
    pub fn new(hz: u32) -> Self {
        assert!(hz > 0, "FrameClock hz must be non-zero");
        let step = Duration::from_secs_f64(1.0 / f64::from(hz));
        Self {
            step,
            accumulator: Duration::ZERO,
            max_accumulate: step * 5,
        }
    }

    /// The fixed timestep duration.
    #[must_use]
    pub const fn step(&self) -> Duration {
        self.step
    }

    /// The fixed timestep duration in seconds.
    #[must_use]
    pub const fn dt_secs(&self) -> f64 {
        self.step.as_secs_f64()
    }

    /// Add elapsed wall time to the accumulator, clamped to the catch-up cap.
    ///
    /// Call once per rendered frame with [`Frame::dt`](crate::Frame).
    pub fn advance(&mut self, dt: Duration) {
        self.accumulator = (self.accumulator + dt).min(self.max_accumulate);
    }

    /// Consume one fixed step if enough time has accumulated.
    ///
    /// Returns `true` when a logic step is due (and deducts it). Call in a loop
    /// until it returns `false`, then render:
    ///
    /// ```
    /// # use core::time::Duration;
    /// # use retroglyph_core::FrameClock;
    /// # let mut clock = FrameClock::new(60);
    /// clock.advance(Duration::from_millis(16));
    /// while clock.tick() {
    ///     // one fixed logic update
    /// }
    /// ```
    #[must_use]
    pub fn tick(&mut self) -> bool {
        if self.accumulator >= self.step {
            self.accumulator -= self.step;
            true
        } else {
            false
        }
    }

    /// Fraction of the next step already accumulated, in `0.0..1.0`.
    ///
    /// Multiply by the delta between the previous and current state to render an
    /// interpolated position between fixed logic frames.
    #[must_use]
    pub fn alpha(&self) -> f64 {
        self.accumulator.as_secs_f64() / self.step.as_secs_f64()
    }

    /// Reset the accumulator. Call after a pause to avoid a burst of catch-up
    /// steps on the next frame.
    pub const fn reset(&mut self) {
        self.accumulator = Duration::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drains_expected_steps() {
        let mut clock = FrameClock::new(100); // 10 ms per step
        clock.advance(Duration::from_millis(35));
        let mut steps = 0;
        while clock.tick() {
            steps += 1;
        }
        assert_eq!(steps, 3);
        // 5 ms of remainder carries over as alpha.
        assert!((clock.alpha() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn caps_catch_up() {
        let mut clock = FrameClock::new(60);
        // A huge stall must not produce unbounded steps.
        clock.advance(Duration::from_secs(10));
        let mut steps = 0;
        while clock.tick() {
            steps += 1;
        }
        assert_eq!(steps, 5); // clamped to max_accumulate (5 steps)
    }

    #[test]
    fn reset_clears_accumulator() {
        let mut clock = FrameClock::new(60);
        clock.advance(Duration::from_millis(100));
        clock.reset();
        assert!(!clock.tick());
    }
}
