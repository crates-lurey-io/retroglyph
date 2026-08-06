//! Rolling frame-time statistics for a live perf/FPS overlay.
//!
//! [`FrameStats`](crate::frames::FrameStats) is a fixed-size ring buffer fed one sample per frame via
//! [`record`](crate::frames::FrameStats::record), decoupled from any renderer or backend the same way
//! [`FrameClock`](crate::frames::FrameClock) is: it only consumes wall time handed to it
//! (typically [`Frame::delta`](crate::app::Frame)), never reads a clock itself, so it stays
//! `no_std`-clean and platform-agnostic (including wasm, where there is no `std::time::Instant`).
//!
//! # Example
//!
//! ```
//! use core::time::Duration;
//! use retroglyph_core::frames::FrameStats;
//!
//! let mut stats: FrameStats = FrameStats::new(); // 120-frame window by default
//! for _ in 0..30 {
//!     stats.record(Duration::from_millis(16));
//! }
//! assert!((stats.fps() - 62.5).abs() < 0.5);
//! assert!((stats.avg().as_millis() as i64 - 16).abs() <= 1);
//! ```

use core::time::Duration;

/// A fixed-size ring buffer of recent per-frame durations, plus the min/max/average/fps readouts
/// derived from it.
///
/// `N` bounds memory and how far back the window looks; the default, 120 samples (about two
/// seconds at 60fps), is a reasonable window for a live overlay. Every reducer
/// ([`avg`](Self::avg), [`min`](Self::min), [`max`](Self::max), [`fps`](Self::fps)) is computed
/// on demand from the current window rather than maintained incrementally, since a live overlay
/// only calls them once per rendered frame, not once per sample.
///
/// Readouts are [`Duration`], not a pre-chosen unit: a caller formats with whatever precision it
/// needs (`as_millis()`, `as_secs_f32()`, ...) rather than being handed milliseconds it then has
/// to convert back if it wants something else. [`current`](Self::current) is the single most
/// recent sample, unsmoothed: pair it with the windowed [`min`](Self::min)/[`max`](Self::max) for
/// a "current, min, max" readout, and [`samples`](Self::samples) for a frame-time graph (feed it,
/// converted to milliseconds, straight into
/// [`Sparkline`](https://docs.rs/retroglyph-ui/latest/retroglyph_ui/struct.Sparkline.html)).
#[derive(Debug, Clone)]
pub struct FrameStats<const N: usize = 120> {
    /// Ring buffer of frame durations.
    samples: [Duration; N],
    /// Number of valid entries in `samples` (`0..=N`); reaches `N` once the ring has wrapped.
    len: usize,
    /// Index the next [`record`](Self::record) call writes to.
    head: usize,
    /// Total frames ever recorded, uncapped by `N`.
    frames: u64,
}

impl<const N: usize> Default for FrameStats<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> FrameStats<N> {
    /// An empty window; every readout is [`Duration::ZERO`] (or `0.0` for [`fps`](Self::fps))
    /// until the first [`record`](Self::record).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            samples: [Duration::ZERO; N],
            len: 0,
            head: 0,
            frames: 0,
        }
    }

    /// Records one frame's wall-clock duration.
    ///
    /// Call once per rendered frame with [`Frame::delta`](crate::app::Frame::delta). A no-op if
    /// `N` is `0` (frame count still advances) rather than panicking: a zero-capacity window is
    /// a degenerate but harmless configuration, not a caller error worth crashing over.
    pub fn record(&mut self, delta: Duration) {
        self.frames = self.frames.wrapping_add(1);
        if N == 0 {
            return;
        }
        self.samples[self.head] = delta;
        self.head = (self.head + 1) % N;
        self.len = (self.len + 1).min(N);
    }

    /// Total frames ever recorded via [`record`](Self::record), uncapped by `N`.
    #[must_use]
    pub const fn frame_count(&self) -> u64 {
        self.frames
    }

    /// The window's starting index into `samples` (oldest sample first): `0` while the ring
    /// hasn't wrapped yet, `head` (the slot about to be overwritten next) once it has.
    const fn start(&self) -> usize {
        if self.len < N { 0 } else { self.head }
    }

    /// Recorded frame durations, oldest first (so the most recent frame is last): the order a
    /// sparkline/histogram wants so new data enters on the right.
    #[must_use]
    pub fn samples(&self) -> impl ExactSizeIterator<Item = Duration> + '_ {
        let start = self.start();
        let len = self.len;
        // `N` is `0` only in the degenerate case handled by `record`'s early return, in which
        // case `len` is always `0` too and this range never indexes `samples`.
        let modulus = if N == 0 { 1 } else { N };
        (0..len).map(move |i| self.samples[(start + i) % modulus])
    }

    /// The most recently recorded frame's duration, unsmoothed. [`Duration::ZERO`] before the
    /// first [`record`](Self::record).
    #[must_use]
    pub const fn current(&self) -> Duration {
        if self.len == 0 || N == 0 {
            return Duration::ZERO;
        }
        self.samples[(self.head + N - 1) % N]
    }

    /// The average frame duration over the current window. [`Duration::ZERO`] before the first
    /// [`record`](Self::record).
    #[must_use]
    pub fn avg(&self) -> Duration {
        if self.len == 0 {
            return Duration::ZERO;
        }
        // `len` is at most `N`, a small fixed window (hundreds of samples at most), so it always
        // fits a `u32` divisor.
        #[allow(clippy::cast_possible_truncation)]
        let len = self.len as u32;
        self.samples().sum::<Duration>() / len
    }

    /// The fastest (shortest) frame in the current window. [`Duration::ZERO`] before the first
    /// [`record`](Self::record).
    #[must_use]
    pub fn min(&self) -> Duration {
        self.samples().min().unwrap_or_default()
    }

    /// The slowest (longest) frame in the current window. [`Duration::ZERO`] before the first
    /// [`record`](Self::record).
    #[must_use]
    pub fn max(&self) -> Duration {
        self.samples().max().unwrap_or_default()
    }

    /// Frames per second, derived from [`avg`](Self::avg) over the current window. `0.0` before
    /// the first [`record`](Self::record) (rather than dividing by zero).
    #[must_use]
    pub fn fps(&self) -> f32 {
        let avg = self.avg().as_secs_f32();
        if avg <= 0.0 { 0.0 } else { 1.0 / avg }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settled<const N: usize>(millis: u64, frames: usize) -> FrameStats<N> {
        let mut stats = FrameStats::new();
        for _ in 0..frames {
            stats.record(Duration::from_millis(millis));
        }
        stats
    }

    #[test]
    fn empty_stats_read_as_zero() {
        let stats = FrameStats::<8>::new();
        assert_eq!(stats.frame_count(), 0);
        assert_eq!(stats.current(), Duration::ZERO);
        assert_eq!(stats.avg(), Duration::ZERO);
        assert_eq!(stats.min(), Duration::ZERO);
        assert_eq!(stats.max(), Duration::ZERO);
        assert!((stats.fps() - 0.0).abs() < f32::EPSILON);
        assert_eq!(stats.samples().count(), 0);
    }

    #[test]
    fn steady_frames_report_consistent_stats() {
        let stats = settled::<8>(16, 5);
        assert_eq!(stats.frame_count(), 5);
        assert_eq!(stats.current(), Duration::from_millis(16));
        assert_eq!(stats.avg(), Duration::from_millis(16));
        assert_eq!(stats.min(), Duration::from_millis(16));
        assert_eq!(stats.max(), Duration::from_millis(16));
        assert!((stats.fps() - 62.5).abs() < 0.1);
        assert_eq!(stats.samples().count(), 5);
    }

    #[test]
    fn ring_buffer_wraps_and_drops_the_oldest_sample() {
        use alloc::vec::Vec;

        let mut stats = FrameStats::<3>::new();
        for ms in [10, 20, 30, 40] {
            stats.record(Duration::from_millis(ms));
        }
        // Capacity 3, 4 samples recorded: the oldest (10ms) fell off the window.
        assert_eq!(stats.frame_count(), 4);
        let samples: Vec<Duration> = stats.samples().collect();
        assert_eq!(
            samples,
            [10, 20, 30, 40][1..]
                .iter()
                .map(|&ms| Duration::from_millis(ms))
                .collect::<Vec<_>>()
        );
        assert_eq!(stats.current(), Duration::from_millis(40));
        assert_eq!(stats.min(), Duration::from_millis(20));
        assert_eq!(stats.max(), Duration::from_millis(40));
    }

    #[test]
    fn min_and_max_track_the_extremes_of_a_varying_window() {
        let mut stats = FrameStats::<8>::new();
        for ms in [16, 16, 40, 16, 8, 16] {
            stats.record(Duration::from_millis(ms));
        }
        assert_eq!(stats.min(), Duration::from_millis(8));
        assert_eq!(stats.max(), Duration::from_millis(40));
        assert_eq!(stats.current(), Duration::from_millis(16));
    }

    #[test]
    fn zero_capacity_window_still_counts_frames_without_panicking() {
        let mut stats = FrameStats::<0>::new();
        stats.record(Duration::from_millis(16));
        stats.record(Duration::from_millis(16));
        assert_eq!(stats.frame_count(), 2);
        assert_eq!(stats.avg(), Duration::ZERO);
        assert_eq!(stats.samples().count(), 0);
    }

    #[test]
    fn default_matches_new() {
        let stats: FrameStats<4> = FrameStats::default();
        assert_eq!(stats.frame_count(), 0);
    }

    /// The whole point of returning [`Duration`] instead of a pre-chosen unit (milliseconds, as
    /// an earlier revision of this API did): sub-millisecond precision survives untouched.
    /// Rounding to whole milliseconds internally, the way an `f32`-milliseconds readout would
    /// tend to invite, would make this fail.
    #[test]
    fn sub_millisecond_precision_is_not_rounded_away() {
        let mut stats = FrameStats::<4>::new();
        stats.record(Duration::from_micros(1500)); // 1.5ms, not representable as whole ms
        assert_eq!(stats.current(), Duration::from_micros(1500));
        assert_eq!(stats.avg(), Duration::from_micros(1500));
        assert_eq!(stats.min(), Duration::from_micros(1500));
        assert_eq!(stats.max(), Duration::from_micros(1500));
    }

    #[test]
    fn fps_reflects_a_varying_not_just_steady_window() {
        let mut stats = FrameStats::<4>::new();
        // Two 10ms frames and two 30ms frames: average 20ms -> 50fps, not the steady-state
        // 1000/10=100 or 1000/30=33 either extreme would give.
        for ms in [10, 30, 10, 30] {
            stats.record(Duration::from_millis(ms));
        }
        assert!((stats.fps() - 50.0).abs() < 0.1, "fps={}", stats.fps());
    }
}
