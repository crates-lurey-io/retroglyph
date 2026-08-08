//! Tutorial chapter 1: hello
//!
//! The smallest complete retroglyph program: open a backend, draw `@` in the middle of the
//! grid, and quit cleanly on `q`/`Escape` or the window's close button. Chapter 2 gives this
//! same `@` a position that moves; chapter 3 puts it on a map. See `docs/src/tutorial/01-hello.md`
//! for the walkthrough this file backs.
//!
//! ```sh
//! cargo run --example 01_hello --features crossterm
//! cargo run --example 01_hello --features software
//! cargo run --example 01_hello  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Keys: `q` or `Escape` quits.

use retroglyph::backend::Backend;
use retroglyph::color::Style;
use retroglyph::event::{Event, KeyCode};
use retroglyph::terminal::Terminal;
use retroglyph_examples::Example;

// ANCHOR: state
/// State for the hello example: none needed yet. `@` is drawn at a fixed spot; chapter 2 adds
/// a position here and moves it.
#[derive(Default)]
pub struct Hello;
// ANCHOR_END: state

impl Hello {
    /// Drains pending input, returning `false` if the user asked to quit (`q`/`Escape`, or the
    /// window's close button).
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

    // ANCHOR: draw
    /// Draws this frame (the driver presents).
    #[allow(clippy::unused_self)]
    fn draw<B: Backend>(&self, term: &mut Terminal<B>) {
        let mut surface = term.surface();
        let (x, y) = (surface.width() / 2, surface.height() / 2);
        surface.put((x, y), '@', Style::default());
    }
    // ANCHOR_END: draw
}

impl Example for Hello {
    const NAME: &'static str = "01_hello";

    fn tick<B: Backend>(
        &mut self,
        term: &mut Terminal<B>,
        _frame: &retroglyph::app::Frame,
    ) -> bool {
        if !self.handle_events(term) {
            return false;
        }
        self.draw(term);
        true
    }
}

retroglyph_examples::example_main!(Hello);
