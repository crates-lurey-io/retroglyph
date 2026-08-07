//! Tutorial chapter 2: input
//!
//! Chapter 1's fixed `@` gets a position and moves with the arrow keys, clamped to the edges of
//! the grid (there's no map yet -- chapter 3 adds walls and real collision). See
//! `docs/src/tutorial/02-input.md` for the walkthrough this file backs.
//!
//! ```sh
//! cargo run --example 02_input --features crossterm
//! cargo run --example 02_input --features software
//! cargo run --example 02_input  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Keys: arrow keys move. `q` or `Escape` quits.

use retroglyph_core::backend::Backend;
use retroglyph_core::color::Style;
use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::terminal::Terminal;
use retroglyph_examples::Example;

// ANCHOR: state
/// State for the input example: the player's position, in cells.
pub struct Input {
    x: u16,
    y: u16,
}

impl Default for Input {
    fn default() -> Self {
        // Centered on the 50x25 grid every backend in this crate uses by default; chapter 3's
        // `init` override centers on the real backend size instead (see its own doc comment).
        Self { x: 25, y: 12 }
    }
}
// ANCHOR_END: state

impl Input {
    // ANCHOR: movement
    /// Moves the player by one cell in `(dx, dy)`, clamped to `width`x`height` so it never
    /// leaves the grid.
    fn try_move(&mut self, width: u16, height: u16, dx: i32, dy: i32) {
        let nx = i32::from(self.x) + dx;
        let ny = i32::from(self.y) + dy;
        if let Ok(nx) = u16::try_from(nx)
            && nx < width
        {
            self.x = nx;
        }
        if let Ok(ny) = u16::try_from(ny)
            && ny < height
        {
            self.y = ny;
        }
    }

    /// Drains pending input: moves on an arrow key, returns `false` if the user asked to quit.
    fn handle_events<B: Backend>(&mut self, term: &mut Terminal<B>) -> bool {
        let size = term.size();
        let (width, height) = (size.width, size.height);
        for event in term.drain_events() {
            match event {
                Event::Key(key) => match key.code {
                    KeyCode::Char('q') | KeyCode::Escape => return false,
                    KeyCode::Left => self.try_move(width, height, -1, 0),
                    KeyCode::Right => self.try_move(width, height, 1, 0),
                    KeyCode::Up => self.try_move(width, height, 0, -1),
                    KeyCode::Down => self.try_move(width, height, 0, 1),
                    _ => {}
                },
                Event::Close => return false,
                _ => {}
            }
        }
        true
    }
    // ANCHOR_END: movement

    /// Draws this frame (the driver presents).
    fn draw<B: Backend>(&self, term: &mut Terminal<B>) {
        let mut surface = term.surface();
        surface.put((self.x, self.y), '@', Style::default());
    }
}

impl Example for Input {
    const NAME: &'static str = "02_input";

    fn tick<B: Backend>(
        &mut self,
        term: &mut Terminal<B>,
        _frame: &retroglyph_core::app::Frame,
    ) -> bool {
        if !self.handle_events(term) {
            return false;
        }
        self.draw(term);
        true
    }
}

retroglyph_examples::example_main!(Input);
