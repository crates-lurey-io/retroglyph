//! 06: Layers
//!
//! Multi-layer compositing: [`Terminal::layer`] selects which layer subsequent draw calls
//! write to. A background fill lives on layer 0; a single glyph that steps one cell to the
//! right each tick lives on layer 1. Layer 1's untouched cells stay the default, empty
//! [`Tile`](retroglyph_core::Tile), which is transparent -- so the layer-0 fill shows through
//! everywhere the moving glyph currently isn't, proving both z-order (layer 1 draws over
//! layer 0) and transparency (an empty tile on a higher layer never occludes a lower one).
//! [`Terminal::present`] composites every allocated layer into one frame on every backend,
//! not just pixel ones, so this looks identical everywhere.
//!
//! ```sh
//! cargo run --example 06_layers --features crossterm
//! cargo run --example 06_layers --features software
//! cargo run --example 06_layers  # headless fallback, prints a few frames to stdout
//! ```
//!
//! The glyph advances automatically, one column every 1/[`STEP_INTERVAL_HZ`] of a second
//! of real elapsed time (not once per raw `tick` call -- see
//! [`FrameClock`](retroglyph_core::FrameClock)'s doc comment on why: a crossterm binary's
//! event loop is an unthrottled spin, so counting raw ticks would blow through the whole
//! track in microseconds instead of animating visibly), and parks at the end of its track
//! (rather than looping forever) so the frame eventually settles into a stable,
//! reproducible state; `q` or `Escape` quits, or close the window.

use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::{AnsiColor, Backend, Color, Frame, FrameClock, Style, Terminal};
use retroglyph_examples::Example;

/// Width of the row the layer-1 glyph travels across before wrapping.
const TRACK_WIDTH: u16 = 48;

/// How often the glyph advances one column, in real elapsed time. Matches
/// [`retroglyph_examples::HEADLESS_FRAME_DELTA`] (100ms) so the headless snapshot's
/// frame-by-frame progression (driven by that fixed synthetic delta) advances by
/// exactly one column per call, same as before this example switched from a raw
/// per-tick counter to a wall-clock-paced one.
const STEP_INTERVAL_HZ: u32 = 10;

/// State for the layers example: a fixed-timestep accumulator gating how many
/// [`STEP_INTERVAL_HZ`]-spaced steps the layer-1 glyph's column has advanced.
pub struct Layers {
    clock: FrameClock,
    ticks: u32,
}

impl Default for Layers {
    fn default() -> Self {
        Self {
            clock: FrameClock::new(STEP_INTERVAL_HZ),
            ticks: 0,
        }
    }
}

impl Layers {
    /// Drains pending input, returning `false` if the user asked to quit.
    #[allow(clippy::needless_pass_by_ref_mut, clippy::unused_self)]
    fn handle_events<B: Backend>(&mut self, term: &mut Terminal<B>) -> bool {
        for event in term.drain_events() {
            match event {
                Event::Key(key) if matches!(key.code, KeyCode::Char('q') | KeyCode::Escape) => {
                    return false;
                }
                Event::Close => return false,
                _ => {}
            }
        }
        true
    }

    /// Draws this frame and presents it: a layer-0 background fill, then a single
    /// layer-1 glyph at a column derived from `self.ticks`.
    fn draw<B: Backend>(&self, term: &mut Terminal<B>) {
        term.print(1, 1, "Layer 0: background fill. Layer 1: moving glyph.");
        term.print(1, 2, "q / Escape quits.");

        // A non-space glyph (rather than a colored blank) so the layer-0 fill is
        // visible in the plain-text headless snapshot too, not just the PNG/SVG
        // ones -- without it, z-order/transparency would only be provable on the
        // two color-aware backends.
        let bg_style = Style::new()
            .fg(Color::Ansi(AnsiColor::BrightBlack))
            .bg(Color::Ansi(AnsiColor::Blue));
        term.layer(0);
        for x in 0..TRACK_WIDTH {
            term.put_styled(1 + x, 10, '.', bg_style);
        }

        // Parks at the last column instead of wrapping: an animation that loops
        // forever never settles into a single frame a screenshot/capture can pin,
        // so a fixed end state is what makes this example's snapshots reproducible.
        let step = self.ticks.min(u32::from(TRACK_WIDTH) - 1);
        let glyph_x = 1 + u16::try_from(step).expect("step is bounded by TRACK_WIDTH");
        if step == u32::from(TRACK_WIDTH) - 1 {
            term.print(1, 12, "(parked at track end)");
        }
        let glyph_style = Style::new()
            .fg(Color::Ansi(AnsiColor::BrightYellow))
            .bg(Color::Default);
        term.layer(1);
        term.put_styled(glyph_x, 10, '@', glyph_style);

        term.present().ok();
    }
}

impl Example for Layers {
    const NAME: &'static str = "06_layers";

    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>, frame: &Frame) -> bool {
        if !self.handle_events(term) {
            return false;
        }
        self.draw(term);
        // Real-time-paced, not once per raw tick: `frame.delta` is the actual elapsed
        // wall time since the previous call, correct on every backend (including an
        // unthrottled crossterm spin loop, where many ticks can fire between two
        // `FrameClock::tick` steps -- `clock.tick()`'s `while` drains however many
        // STEP_INTERVAL_HZ-sized steps are actually due, zero most of the time).
        self.clock.advance(frame.delta);
        while self.clock.tick() {
            self.ticks = self.ticks.saturating_add(1);
        }
        true
    }
}

retroglyph_examples::example_main!(Layers);
