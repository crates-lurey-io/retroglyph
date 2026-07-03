//! Fixed-timestep accumulator for example game loops.
//!
//! The software backend's event loop fires the tick closure as fast as the
//! display will allow. A fixed timestep accumulator decouples *logic* updates
//! (which run at a stable, predictable rate) from *rendering* (which runs as
//! fast as possible, interpolating between logic frames).
//!
//! # Basic usage
//!
//! ```ignore
//! let mut step = FixedStep::new(60); // 60 logic updates per second
//!
//! fn tick(term: &mut Terminal<impl Backend>, state: &mut State) -> bool {
//!     // Run zero or more logic steps to catch up to wall time.
//!     while step.update() {
//!         state.update(step.dt());
//!     }
//!     // Render once, optionally with interpolation via step.alpha().
//!     state.draw(term, step.alpha());
//!     term.present().unwrap();
//!     true
//! }
//! ```
#![allow(dead_code)]

#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};

#[cfg(target_arch = "wasm32")]
mod wasm_time {
    pub use std::time::Duration;
    #[derive(Clone, Copy)]
    pub struct Instant;
    impl Instant {
        pub fn now() -> Self {
            Self
        }
        pub fn elapsed(self) -> Duration {
            Duration::ZERO
        }
    }
    impl std::ops::Sub for Instant {
        type Output = Duration;
        fn sub(self, _: Self) -> Duration {
            Duration::ZERO
        }
    }
}
#[cfg(target_arch = "wasm32")]
use wasm_time::{Duration, Instant};

/// Measures wall-clock time between frames for feeding a [`FrameClock`].
///
/// Wraps the platform split the demos need: real elapsed time via [`Instant`]
/// on native targets, a fixed 16 ms assumption on `wasm32` (where there is no
/// `SystemTime`/`Instant`). This is the tiny helper referenced by the dashboard
/// demo plan; it exists because `rg_run!`'s `tick` signature hides `Frame.dt`.
///
/// [`FrameClock`]: retroglyph_core::FrameClock
pub struct Stopwatch {
    last: Instant,
}

impl Default for Stopwatch {
    fn default() -> Self {
        Self::new()
    }
}

impl Stopwatch {
    /// Start the stopwatch at the current instant.
    #[must_use]
    pub fn new() -> Self {
        Self {
            last: Instant::now(),
        }
    }

    /// Return the elapsed time since the previous `lap` (or construction) and
    /// reset the mark. On `wasm32` this always returns a fixed 16 ms tick.
    pub fn lap(&mut self) -> Duration {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let now = Instant::now();
            let dt = now.duration_since(self.last);
            self.last = now;
            dt
        }
        #[cfg(target_arch = "wasm32")]
        {
            Duration::from_millis(16)
        }
    }
}

/// Fixed-timestep accumulator.
///
/// Each call to [`FixedStep::update`] advances the internal clock by the
/// elapsed wall time, then returns `true` once per pending logic step. Keep
/// calling it in a loop until it returns `false`, then render.
pub struct FixedStep {
    /// Fixed duration of one logic tick.
    step: Duration,
    /// Wall-clock time at the last `update` call (or construction).
    last: Instant,
    /// Accumulated time not yet consumed by logic ticks.
    accumulator: Duration,
    /// Fraction of `step` consumed by the current render frame (0..1).
    /// Use for linear interpolation between logic frames.
    alpha: f64,
    /// Maximum time to accumulate in one render frame. Prevents the
    /// "spiral of death" when logic is temporarily slower than real time.
    max_accumulate: Duration,
}

impl FixedStep {
    /// Create a new accumulator targeting `hz` logic updates per second.
    pub fn new(hz: u32) -> Self {
        let step = Duration::from_secs_f64(1.0 / f64::from(hz));
        Self {
            step,
            last: Instant::now(),
            accumulator: Duration::ZERO,
            alpha: 0.0,
            // Cap at 5 frames worth of catch-up to prevent spiral of death.
            max_accumulate: step * 5,
        }
    }

    /// Fixed timestep duration (seconds).
    pub const fn dt(&self) -> f64 {
        self.step.as_secs_f64()
    }

    /// Interpolation fraction for the current render frame (0.0..1.0).
    ///
    /// Multiply by the difference between the previous and current state
    /// to get a smoothly interpolated position for rendering.
    pub const fn alpha(&self) -> f64 {
        self.alpha
    }

    /// Advance by wall time and return `true` if a logic step is pending.
    ///
    /// Call this in a loop at the top of your render tick:
    ///
    /// ```ignore
    /// while step.update() {
    ///     state.update(step.dt());
    /// }
    /// ```
    ///
    /// On the first call per render frame this measures elapsed time since
    /// the last call and adds it to the accumulator. Subsequent calls in the
    /// same loop iteration just consume the accumulator without re-measuring.
    pub fn update(&mut self) -> bool {
        // Only measure wall time on the first call per render frame (when the
        // accumulator is zero or was just exhausted).
        if self.accumulator == Duration::ZERO {
            let now = Instant::now();
            #[cfg(not(target_arch = "wasm32"))]
            let delta = now.duration_since(self.last).min(self.max_accumulate);
            #[cfg(target_arch = "wasm32")]
            let delta = Duration::ZERO;
            self.last = now;
            self.accumulator += delta;
        }

        if self.accumulator >= self.step {
            self.accumulator -= self.step;
            self.alpha = self.accumulator.as_secs_f64() / self.step.as_secs_f64();
            true
        } else {
            self.alpha = self.accumulator.as_secs_f64() / self.step.as_secs_f64();
            // Reset so next render frame re-measures wall time.
            // (accumulator is already < step, leave it so partial time carries over.)
            false
        }
    }

    /// Reset the accumulator and clock. Call when re-entering the game loop
    /// after a pause to avoid a burst of catch-up logic steps.
    pub fn reset(&mut self) {
        self.last = Instant::now();
        self.accumulator = Duration::ZERO;
        self.alpha = 0.0;
    }
}
