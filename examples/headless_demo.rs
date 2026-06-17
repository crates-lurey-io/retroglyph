//! Example headless demo.

use rg::backend::Headless;
use rg::color::{AnsiColor, Color};
use rg::Terminal;

fn main() {
    let backend = Headless::new(40, 15);
    let mut term = Terminal::new(backend);

    // Draw room
    term.clear();
    term.fg(Color::Ansi(AnsiColor::White));
    for x in 0..40 {
        term.put(x, 0, '─');
        term.put(x, 14, '─');
    }
    for y in 0..15 {
        term.put(0, y, '│');
        term.put(39, y, '│');
    }
    term.put(0, 0, '┌');
    term.put(39, 0, '┐');
    term.put(0, 14, '└');
    term.put(39, 14, '┘');

    // Draw player
    term.fg(Color::Ansi(AnsiColor::Green));
    term.put(5, 5, '@');
    term.reset_style();

    // Draw status
    term.print(1, 1, "HP: 100  Level: 1");

    term.present();

    println!("{}", term.backend().grid());

    // Demonstrate present again
    term.present();
}
