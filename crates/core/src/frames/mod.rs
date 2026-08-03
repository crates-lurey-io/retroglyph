//! [`FrameClock`] and [`FrameStats`], the two accumulators behind the `App`/`Frame` game loop.
//!
//! Both are pure accumulators fed by [`Frame::delta`](crate::Frame): neither reads a clock
//! itself, they only consume wall time the driver hands them, which keeps both `no_std`-clean and
//! platform-agnostic (including wasm, where there is no `std::time::Instant`). Split across
//! `clock.rs`/`stats.rs` (this file just re-exports), one file per concept: [`FrameClock`]
//! decouples fixed-rate logic updates from rendering, [`FrameStats`] is a rolling window of frame
//! durations for a live perf/FPS overlay.

mod clock;
mod stats;

pub use clock::FrameClock;
pub use stats::FrameStats;
