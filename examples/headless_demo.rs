//! Example headless demo.

use retroglyph::Terminal;
use retroglyph::backend::Headless;
use retroglyph::color::{AnsiColor, Color};
use retroglyph::event::{Event, KeyCode, KeyEvent, KeyModifiers};

fn draw_scene(term: &mut Terminal<Headless>, player_x: u16, player_y: u16) {
    // Draw room
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
    term.put(player_x, player_y, '@');

    // Draw enemies
    term.fg(Color::Ansi(AnsiColor::Red));
    term.put(10, 8, 'g');
    term.put(20, 10, 'D');
    term.reset_style();

    // Draw status
    term.print(1, 1, "HP: 100  Level: 1");
}

fn main() {
    let backend = Headless::new(40, 15);
    let mut term = Terminal::new(backend);

    let mut player_x = 5;
    let player_y = 5;

    // Initial render
    draw_scene(&mut term, player_x, player_y);
    term.present().expect("present failed");

    println!("--- Frame 1 ---");
    println!("{}", term.backend().grid());

    // Event Injection
    term.backend_mut().push_event(Event::Key(KeyEvent {
        code: KeyCode::Right,
        modifiers: KeyModifiers::NONE,
    }));

    // Event Processing
    let event = term.read();
    if let Event::Key(key_event) = event
        && key_event.code == KeyCode::Right
    {
        player_x += 1;
    }

    // Render with updated state
    draw_scene(&mut term, player_x, player_y);
    term.present().expect("present failed");

    println!("--- Frame 2 (After player moved right) ---");
    println!("{}", term.backend().grid());
}
