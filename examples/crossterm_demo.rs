//! Playable crossterm demo of the `rg` library.

mod util;

use rg::backend::Crossterm;
use rg::Terminal;

fn main() -> Result<(), std::io::Error> {
    let backend = Crossterm::new()?;
    let mut term = Terminal::new(backend);

    let mut player = (5u16, 5u16);

    while util::game::tick(&mut term, &mut player) {}
    Ok(())
}
