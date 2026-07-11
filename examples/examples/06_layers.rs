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
//! The glyph advances automatically and parks at the end of its track (rather than
//! looping forever) so the frame settles into a stable, reproducible state; `q` or
//! `Escape` quits, or close the window.

use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::{AnsiColor, Backend, Color, Style, Terminal};
use retroglyph_examples::Example;

/// Width of the row the layer-1 glyph travels across before wrapping.
const TRACK_WIDTH: u16 = 48;

/// State for the layers example: how many ticks have elapsed, which drives the layer-1
/// glyph's column.
#[derive(Default)]
pub struct Layers {
    ticks: u32,
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

    fn tick<B: Backend>(&mut self, term: &mut Terminal<B>, _frame: &retroglyph_core::Frame) -> bool {
        if !self.handle_events(term) {
            return false;
        }
        self.draw(term);
        self.ticks += 1;
        true
    }
}

retroglyph_examples::example_main!(Layers);
