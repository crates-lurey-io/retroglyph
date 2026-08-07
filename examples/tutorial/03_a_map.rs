//! Tutorial chapter 3: a map
//!
//! Chapter 2's free-roaming `@` gets a level: a fixed-size room built with [`Grid::from_charmap`]
//! and blitted to the screen every frame, with walls (`#`) blocking movement instead of only the
//! screen edge. See `docs/src/tutorial/03-a-map.md` for the walkthrough this file backs.
//!
//! ```sh
//! cargo run --example 03_a_map --features crossterm
//! cargo run --example 03_a_map --features software
//! cargo run --example 03_a_map  # headless fallback, prints a few frames to stdout
//! ```
//!
//! Keys: arrow keys move. `q` or `Escape` quits.

use retroglyph_core::backend::Backend;
use retroglyph_core::color::Style;
use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::grid::Grid;
use retroglyph_core::terminal::Terminal;
use retroglyph_core::tile::Tile;
use retroglyph_examples::Example;

// ANCHOR: map
/// The level: a single walled room. `#` is a wall, everything else (just `.` here) is floor.
const LEVEL: &str = "\
##############
#............#
#............#
#............#
#............#
#............#
##############";

/// Builds the level's [`Grid`]: one [`Tile`] per character, walls and floor each with their own
/// style so they read apart from the player drawn on top of them.
fn build_level() -> Grid {
    let wall = Style::default();
    let floor = Style::default();
    Grid::from_charmap(LEVEL, |c| match c {
        '#' => Tile::new('#', wall),
        _ => Tile::new('.', floor),
    })
}
// ANCHOR_END: map

/// State for the map example: the level (static, blitted unchanged every frame) and the
/// player's position.
pub struct AMap {
    level: Grid,
    x: u16,
    y: u16,
}

impl Default for AMap {
    fn default() -> Self {
        Self {
            level: build_level(),
            x: 2,
            y: 2,
        }
    }
}

impl AMap {
    // ANCHOR: movement
    /// Whether `(x, y)` is a wall, or outside the level entirely (out of bounds is treated the
    /// same as a wall: nothing to walk onto there).
    fn is_wall(&self, x: i32, y: i32) -> bool {
        let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else {
            return true;
        };
        self.level
            .tile(0, (x, y))
            .is_none_or(|tile| tile.glyph() == '#')
    }

    /// Moves the player by one cell in `(dx, dy)`, a no-op if the destination is a wall.
    fn try_move(&mut self, dx: i32, dy: i32) {
        let (nx, ny) = (i32::from(self.x) + dx, i32::from(self.y) + dy);
        if self.is_wall(nx, ny) {
            return;
        }
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        {
            self.x = nx as u16;
            self.y = ny as u16;
        }
    }

    /// Drains pending input: moves on an arrow key, returns `false` if the user asked to quit.
    fn handle_events<B: Backend>(&mut self, term: &mut Terminal<B>) -> bool {
        for event in term.drain_events() {
            match event {
                Event::Key(key) => match key.code {
                    KeyCode::Char('q') | KeyCode::Escape => return false,
                    KeyCode::Left => self.try_move(-1, 0),
                    KeyCode::Right => self.try_move(1, 0),
                    KeyCode::Up => self.try_move(0, -1),
                    KeyCode::Down => self.try_move(0, 1),
                    _ => {}
                },
                Event::Close => return false,
                _ => {}
            }
        }
        true
    }
    // ANCHOR_END: movement

    /// Draws this frame (the driver presents): the level first, then the player on top.
    fn draw<B: Backend>(&self, term: &mut Terminal<B>) {
        let mut surface = term.surface();
        surface.blit(&self.level, 0, 0);
        surface.put((self.x, self.y), '@', Style::default());
    }
}

impl Example for AMap {
    const NAME: &'static str = "03_a_map";

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

retroglyph_examples::example_main!(AMap);
