//! Shared game loop for rg examples.

use rg::color::{AnsiColor, Color};
use rg::event::{Event, KeyCode};
use rg::{Backend, Pos, Terminal};
use std::time::Duration;

// Fixed room size in world space. The terminal clips on small windows rather
// than resizing the room, so positions are stable across resize events.
const ROOM_LEFT: u16 = 2;
const ROOM_TOP: u16 = 2;
const ROOM_RIGHT: u16 = 40;
const ROOM_BOTTOM: u16 = 20;

/// Run one tick of the demo game loop.
///
/// Call this from your backend's frame closure or loop body.
/// Updates `player` position based on input events.
///
/// Returns `false` when the user quits (ESC or Q). The caller should stop
/// the game loop on `false` rather than calling `process::exit`, so that
/// backends like `Crossterm` get their `Drop` • which restores the terminal.
pub fn tick(term: &mut Terminal<impl Backend>, player: &mut Pos) -> bool {
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
    term.put(player.x, player.y, '@');
    term.reset_style();

    // 4. Draw UI status line
    term.print(2, 0, "rg Library — Interactive Demo");
    term.print(
        2,
        size.height.saturating_sub(1),
        "HP: 100 | Arrow Keys to move, Q or ESC to Quit",
    );

    // Present double-buffered frame
    term.present();

    // Handle input (non-blocking poll with ~60 FPS timeout)
    if let Some(event) = term.poll(Duration::from_millis(16)) {
        match event {
            Event::Key(key_event) => match key_event.code {
                KeyCode::Up | KeyCode::Char('w') => {
                    if player.y > ROOM_TOP + 1 {
                        player.y -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('s') => {
                    if player.y < ROOM_BOTTOM - 1 {
                        player.y += 1;
                    }
                }
                KeyCode::Left | KeyCode::Char('a') => {
                    if player.x > ROOM_LEFT + 1 {
                        player.x -= 1;
                    }
                }
                KeyCode::Right | KeyCode::Char('d') => {
                    if player.x < ROOM_RIGHT - 1 {
                        player.x += 1;
                    }
                }
                KeyCode::Escape | KeyCode::Char('q') => {
                    return false;
                }
                _ => {}
            },
            Event::Close => return false,
            _ => {}
        }
    }

    true
}
