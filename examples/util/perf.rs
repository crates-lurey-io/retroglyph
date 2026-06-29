//! Frame-time tracking and on-screen diagnostic overlay.
//!
//! [`PerfOverlay`] maintains a rolling ring buffer of recent frame durations.
//! Call [`PerfOverlay::begin_frame`] at the top of your tick function and
//! call [`PerfOverlay::draw`] to render a compact stats bar into the terminal.
//!
//! The raw frame-time data is also accessible via [`PerfOverlay::samples`]
//! for external analysis (CSV export, benchmark assertions, etc.).
#![allow(dead_code)]

use retroglyph::color::Color;
use retroglyph::style::Style;
use retroglyph::{Backend, Terminal};

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

// On WASM `std::time::Instant` is unavailable; delegate to `performance.now()`
// (sub-millisecond precision) so frame times are real measurements.
#[cfg(target_arch = "wasm32")]
mod wasm_time {
    use wasm_bindgen::prelude::wasm_bindgen;

    #[wasm_bindgen(inline_js = "export function perf_now() { return performance.now(); }")]
    extern "C" {
        fn perf_now() -> f64;
    }

    #[derive(Clone, Copy)]
    pub struct Instant {
        ms: f64,
    }
    impl Instant {
        pub fn now() -> Self {
            Self { ms: perf_now() }
        }
        pub fn elapsed(self) -> std::time::Duration {
            let delta_ms = (perf_now() - self.ms).max(0.0);
            std::time::Duration::from_secs_f64(delta_ms / 1_000.0)
        }
    }
}
#[cfg(target_arch = "wasm32")]
use wasm_time::Instant;

/// Number of frames kept in the rolling window.
const WINDOW: usize = 64;

/// Frame-timing stats over the current window.
#[derive(Debug, Clone, Copy)]
pub struct FrameStats {
    /// Mean frame time in milliseconds.
    pub avg_ms: f64,
    /// Shortest frame in the window, in milliseconds.
    pub min_ms: f64,
    /// Longest frame in the window, in milliseconds.
    pub max_ms: f64,
    /// Frames per second derived from the mean frame time.
    pub fps: f64,
    /// Number of samples in the window (0..=WINDOW).
    pub count: usize,
}

/// Rolling frame-time tracker and optional HUD overlay.
///
/// # Usage
///
/// ```ignore
/// let mut perf = PerfOverlay::new();
///
/// fn tick(term: &mut Terminal<impl Backend>, state: &mut State) -> bool {
///     perf.begin_frame();
///     // ... game update and draw ...
///     perf.draw(term);   // optional
///     term.present().unwrap();
///     true
/// }
/// ```
pub struct PerfOverlay {
    /// Ring buffer of frame durations in microseconds (avoids float storage).
    samples: [u64; WINDOW],
    /// Write head into the ring buffer.
    head: usize,
    /// How many valid samples are in the buffer (saturates at WINDOW).
    count: usize,
    /// Timestamp recorded at `begin_frame`.
    frame_start: Instant,
    /// Whether [`begin_frame`] has been called at least once.
    started: bool,
    /// Whether to show the overlay at all.
    pub visible: bool,
}

impl PerfOverlay {
    /// Create a new overlay with an empty window. Visible by default.
    pub fn new() -> Self {
        Self {
            samples: [0; WINDOW],
            head: 0,
            count: 0,
            frame_start: Instant::now(),
            started: false,
            visible: true,
        }
    }

    /// Record the start of a frame. Call at the very top of your tick fn.
    ///
    /// On the first call this just records a baseline timestamp. On every
    /// subsequent call it records the elapsed time since the *previous*
    /// `begin_frame` — the true inter-frame wall time, including vsync waits
    /// that happen outside the tick function.
    pub fn begin_frame(&mut self) {
        if self.started {
            // Elapsed since last begin_frame = true inter-frame interval.
            let us = u64::try_from(self.frame_start.elapsed().as_micros()).unwrap_or(u64::MAX);
            self.samples[self.head] = us;
            self.head = (self.head + 1) % WINDOW;
            if self.count < WINDOW {
                self.count += 1;
            }
        }
        self.frame_start = Instant::now();
        self.started = true;
    }

    /// Compute stats over the current window. Returns `None` if no samples
    /// have been recorded yet.
    pub fn stats(&self) -> Option<FrameStats> {
        if self.count == 0 {
            return None;
        }
        let mut sum = 0u64;
        let mut lo = u64::MAX;
        let mut hi = 0u64;
        for i in 0..self.count {
            let v = self.samples[i];
            sum += v;
            if v < lo {
                lo = v;
            }
            if v > hi {
                hi = v;
            }
        }
        #[allow(clippy::cast_precision_loss)]
        let mean_us = sum as f64 / self.count as f64;
        let avg_ms = mean_us / 1_000.0;
        Some(FrameStats {
            avg_ms,
            #[allow(clippy::cast_precision_loss)]
            min_ms: lo as f64 / 1_000.0,
            #[allow(clippy::cast_precision_loss)]
            max_ms: hi as f64 / 1_000.0,
            fps: if mean_us > 0.0 {
                1_000_000.0 / mean_us
            } else {
                0.0
            },
            count: self.count,
        })
    }

    /// Raw frame-time samples in microseconds, oldest-first.
    ///
    /// Useful for writing CSV output or asserting on percentile budgets in
    /// benchmark harnesses.
    pub fn samples_us(&self) -> impl Iterator<Item = u64> + '_ {
        // The ring buffer may wrap; yield oldest-first by rotating.
        let start = if self.count < WINDOW {
            0
        } else {
            self.head // head points at the oldest slot when full
        };
        (0..self.count).map(move |i| self.samples[(start + i) % WINDOW])
    }

    /// Dump all samples as a CSV line to stdout: `frame_us,frame_us,...\n`.
    ///
    /// Pipe this to a file for offline analysis or Criterion flamegraphs.
    pub fn dump_csv(&self) {
        let parts: Vec<String> = self.samples_us().map(|v| v.to_string()).collect();
        println!("{}", parts.join(","));
    }

    /// Draw a compact one-line stats bar at `(x, y)`.
    ///
    /// Format: `FPS  60.1  avg  16.6ms  min  14.2ms  max  22.1ms`
    ///
    /// Does nothing if `self.visible` is false or there are no samples yet.
    pub fn draw<B: Backend>(&self, term: &mut Terminal<B>, x: u16, y: u16) {
        if !self.visible {
            return;
        }
        let Some(s) = self.stats() else { return };

        let text = format!(
            " FPS {:5.1}  avg {:6.2}ms  min {:6.2}ms  max {:6.2}ms ",
            s.fps, s.avg_ms, s.min_ms, s.max_ms,
        );

        let style = Style::new()
            .fg(Color::Rgb {
                r: 180,
                g: 220,
                b: 180,
            })
            .bg(Color::Rgb {
                r: 20,
                g: 20,
                b: 30,
            });

        for (i, ch) in text.chars().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            term.put_styled(x + i as u16, y, ch, style);
        }
    }
}

impl Default for PerfOverlay {
    fn default() -> Self {
        Self::new()
    }
}
