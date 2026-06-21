//! Playable crossterm demo of the `rg` library.

mod util;

use retroglyph::backend::Crossterm;
use retroglyph::{Pos, Terminal};

fn main() -> Result<(), std::io::Error> {
    let backend = Crossterm::new()?;
    let mut term = Terminal::new(backend);

    let mut player = Pos::new(5, 5);

    while util::game::tick(&mut term, &mut player) {}
    Ok(())
}
