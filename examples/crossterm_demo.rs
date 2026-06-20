//! Playable crossterm demo of the `rg` library.

use rg::Terminal;
use rg::backend::Crossterm;
use rg::color::{AnsiColor, Color};
use rg::event::{Event, KeyCode};

// Fixed room size in world space. The terminal clips on small windows rather
// than resizing the room, so positions are stable across resize events.
const ROOM_LEFT: u16 = 2;
const ROOM_TOP: u16 = 2;
const ROOM_RIGHT: u16 = 40;
const ROOM_BOTTOM: u16 = 20;

fn main() -> Result<(), std::io::Error> {
    let backend = Crossterm::new()?;
    let mut term = Terminal::new(backend);

    let mut player_x: u16 = 5;
    let mut player_y: u16 = 5;

    loop {
        let size = term.size();

        term.clear();

        // 1. Draw room boundary (box-drawing characters)
        term.fg(Color::Ansi(AnsiColor::White));
        for x in ROOM_LEFT + 1..ROOM_RIGHT {
            term.put(x, ROOM_TOP, '─');
            term.put(x, ROOM_BOTTOM, '─');
        }
        for y in ROOM_TOP + 1..ROOM_BOTTOM {
            term.put(ROOM_LEFT, y, '│');
            term.put(ROOM_RIGHT, y, '│');
        }
        term.put(ROOM_LEFT, ROOM_TOP, '┌');
        term.put(ROOM_RIGHT, ROOM_TOP, '┐');
        term.put(ROOM_LEFT, ROOM_BOTTOM, '└');
        term.put(ROOM_RIGHT, ROOM_BOTTOM, '┘');

        // 2. Draw Enemy (Red D) — fixed center of the room
        term.fg(Color::Ansi(AnsiColor::Red));
        term.put(
            u16::midpoint(ROOM_LEFT, ROOM_RIGHT),
            u16::midpoint(ROOM_TOP, ROOM_BOTTOM),
            'D',
        );

        // 3. Draw Player (@ - Green)
        term.fg(Color::Ansi(AnsiColor::Green));
        term.put(player_x, player_y, '@');
        term.reset_style();

        // 4. Draw UI status line
        term.print(2, 0, "rg Library — Crossterm Interactive Demo");
        term.print(
            2,
            size.height.saturating_sub(1),
            "HP: 100 | Arrow Keys to move, Q or ESC to Quit",
        );

        // Present double-buffered frame
        term.present();

        // Wait for next input event (Resize is auto-applied by the terminal)
        let event = term.read();
        if let Event::Key(key_event) = event {
            match key_event.code {
                KeyCode::Up | KeyCode::Char('w') => {
                    if player_y > ROOM_TOP + 1 {
                        player_y -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('s') => {
                    if player_y < ROOM_BOTTOM - 1 {
                        player_y += 1;
                    }
                }
                KeyCode::Left | KeyCode::Char('a') => {
                    if player_x > ROOM_LEFT + 1 {
                        player_x -= 1;
                    }
                }
                KeyCode::Right | KeyCode::Char('d') => {
                    if player_x < ROOM_RIGHT - 1 {
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
