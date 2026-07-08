//! 11: Time & `Sparkline` -- animating from `Frame::delta`, not from input
//!
//! Every example so far only changed what was on screen in response to a key press or click --
//! nothing has moved on its own. This example's new concept is [`Frame::delta`]: the elapsed time
//! since the last frame, accumulated across frames to drive a widget's state instead of input
//! ever touching it. [`Sparkline`] (a single-row bar chart, new here) just draws whatever samples
//! it's handed -- it has no notion of time itself.
//!
//! The samples themselves are a seeded random walk, not a sine wave -- a clean `sin()` produces
//! the same perfectly smooth hump every cycle, which reads as an animation demo rather than a
//! plausible metric. Real sparklines (`btop`, `htop`, Grafana single-stat panels, etc.) chart
//! noisy, bursty, roughly-uncorrelated data: CPU load, requests/sec, network throughput. Each
//! tick below takes one small random step from the previous sample and clamps to `0.0..=1.0`,
//! the same technique the bigger `dashboard` example (`crates/examples/examples/dashboard.rs`)
//! uses for its CPU/network sparklines -- see that file's `walk`/`burst` helpers for the fuller
//! version (per-core jitter, occasional traffic bursts) this is a minimal, dependency-free
//! stand-in for.
//!
//! `update` is called once per render, and nothing on any backend caps how often that happens --
//! `Crossterm`'s loop, in particular, has no throttle at all, so pushing one sample per `update`
//! call would make the sparkline's sample density track the terminal's redraw rate instead of
//! real time (the same reason system monitors like `btop` sample on a fixed interval -- 2 seconds
//! by default -- rather than once per UI redraw). [`FrameClock`] is what fixes that: a
//! fixed-rate accumulator that turns the *variable* `frame.delta` into a *steady* stream of
//! logic ticks, one new sparkline sample per tick, regardless of how fast or slow `update` itself
//! is actually being called.
//!
//! Headless makes this concept visible without a display at all: rerun with `RG_HEADLESS_FRAMES`
//! set higher than the default 3 and the printed frames show the wave actually advancing, frame
//! over frame, with no input ever injected -- see `run_headless`'s own doc comment.
//!
//! ```sh
//! RG_HEADLESS_FRAMES=30 cargo run --example 11_time_and_sparkline                # Headless, 30 frames
//! cargo run --example 11_time_and_sparkline --features crossterm                 # Terminal
//! cargo run --example 11_time_and_sparkline --features default-font              # Desktop window
//! cargo run --example 11_time_and_sparkline --features default-font --target wasm32-unknown-unknown  # WASM
//! ```
//!
//! Press any key (Terminal/Desktop) to quit.

use std::collections::VecDeque;
use std::time::Duration;

use retroglyph_core::grid::Rect;
use retroglyph_core::{App, Backend, Flow, Frame, FrameClock, Terminal};
use retroglyph_gallery::{any_key_pressed_or_window_closed, rg_gallery_run};
use retroglyph_widgets::widget::{Sparkline, Widget};

/// Samples kept around for the sparkline to draw -- bounded so a long-running window/terminal
/// session doesn't grow this forever; `Sparkline` only ever draws the most recent `area.width()`
/// of them anyway; see its own doc comment.
const MAX_SAMPLES: usize = 200;

/// New sparkline sample 10 times a second -- fast enough to look smooth, slow enough that the
/// visible 40-column window covers several real seconds of history instead of a sliver of one.
const SAMPLE_HZ: u32 = 10;

/// Maximum magnitude of one random-walk step -- small enough that consecutive bars stay visually
/// connected (a real metric doesn't teleport), large enough that 200 steps cover most of the
/// `0.0..=1.0` range instead of just jittering around the start value.
const STEP_MAGNITUDE: f32 = 0.12;

struct TimeAndSparkline {
    /// Simulated time, advanced in fixed [`SAMPLE_HZ`] steps by `clock` -- only used for the
    /// on-screen `elapsed:` readout, not to derive the sample itself.
    elapsed: Duration,
    clock: FrameClock,
    samples: VecDeque<f32>,
    /// Last sample pushed, so each new one is a small step from it rather than an independent
    /// draw -- that's what makes it a *walk* instead of just noise.
    last: f32,
    rng: Lcg,
}

/// Minimal 64-bit linear congruential generator (Knuth's multiplicative constants) -- not
/// suitable for cryptography or statistical sampling, but plenty for jittering a demo sparkline
/// without pulling in a `rand` dependency just for this one example.
struct Lcg(u64);

impl Lcg {
    const fn new(seed: u64) -> Self {
        Self(if seed == 0 { 1 } else { seed })
    }

    /// Uniform `f32` in `-0.5..=0.5`, i.e. a signed step direction and magnitude in one draw.
    fn signed_unit(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        #[allow(clippy::cast_precision_loss)]
        let unit = ((self.0 >> 33) % 10_000) as f32 / 10_000.0;
        unit - 0.5
    }
}

impl<B: Backend> App<B> for TimeAndSparkline {
    fn update(&mut self, term: &mut Terminal<B>, frame: &Frame) -> Flow {
        // The only state mutation in this whole example that isn't gated on input: advance the
        // clock and push zero or more new samples, regardless of what (if anything) was pressed.
        // Zero or more, not exactly one: a slow redraw (or `RG_HEADLESS_FRAMES`' 50ms synthetic
        // step) can owe more than one tick's worth of samples at once, while a fast one owes none
        // this call -- that's the point of decoupling sample rate from render rate.
        self.clock.advance(frame.delta);
        while self.clock.tick() {
            self.elapsed += self.clock.step();
            self.last = self
                .rng
                .signed_unit()
                .mul_add(STEP_MAGNITUDE, self.last)
                .clamp(0.0, 1.0);
            if self.samples.len() == MAX_SAMPLES {
                self.samples.pop_front();
            }
            self.samples.push_back(self.last);
        }

        term.print(0, 0, "11: Time & Sparkline");
        term.print(
            0,
            2,
            &format!("elapsed: {:6.1}s", self.elapsed.as_secs_f32()),
        );

        let area = Rect::new(0, 4, 40, 1);
        Sparkline::new(self.samples.make_contiguous()).render(area, term);

        term.present().expect("present failed");

        if any_key_pressed_or_window_closed(term) {
            Flow::Exit
        } else {
            Flow::Continue
        }
    }
}

rg_gallery_run!(
    TimeAndSparkline {
        elapsed: Duration::ZERO,
        clock: FrameClock::new(SAMPLE_HZ),
        samples: VecDeque::new(),
        last: 0.5,
        rng: Lcg::new(0x00C0_FFEE),
    },
    "11: Time & Sparkline",
    40,
    8
);
