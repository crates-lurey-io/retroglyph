//! Playable crossterm demo of the `rg` library.

use rg::Terminal;
use rg::backend::Crossterm;
use rg::color::{AnsiColor, Color};
use rg::event::{Event, KeyCode};

fn main() -> Result<(), std::io::Error> {
    let backend = Crossterm::new()?;
    let mut term = Terminal::new(backend);

    let mut player_x = 5;
    let mut player_y = 5;

    loop {
        term.clear();

        // 1. Draw room boundary
        term.fg(Color::Ansi(AnsiColor::White));
        for x in 2..32 {
            term.put(x, 2, '─');
            term.put(x, 11, '─');
        }
        for y in 2..12 {
            term.put(2, y, '│');
            term.put(31, y, '│');
        }
        term.put(2, 2, '┌');
        term.put(31, 2, '┐');
        term.put(2, 11, '└');
        term.put(31, 11, '┘');

        // 2. Draw Enemy (Red D)
        term.fg(Color::Ansi(AnsiColor::Red));
        term.put(15, 8, 'D');

        // 3. Draw Player (@ - Green)
        term.fg(Color::Ansi(AnsiColor::Green));
        term.put(player_x, player_y, '@');
        term.reset_style();

        // 4. Draw UI status line
        term.print(2, 0, "rg Library — Crossterm Interactive Demo");
        term.print(2, 13, "HP: 100 | Use Arrow Keys to move, Q or ESC to Quit");

        // Present double-buffered frame
        term.present();

        // Wait for next input event
        let event = term.read();
        if let Event::Key(key_event) = event {
            match key_event.code {
                KeyCode::Up | KeyCode::Char('w') => {
                    if player_y > 3 {
                        player_y -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('s') => {
                    if player_y < 10 {
                        player_y += 1;
                    }
                }
                KeyCode::Left | KeyCode::Char('a') => {
                    if player_x > 3 {
                        player_x -= 1;
                    }
                }
                KeyCode::Right | KeyCode::Char('d') => {
                    if player_x < 30 {
                        player_x += 1;
                    }
                }
                KeyCode::Escape | KeyCode::Char('q') => {
                    break;
                }
                _ => {}
            }
        }
    }

    Ok(())
}
