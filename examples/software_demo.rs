//! Playable software-backend demo matching the crossterm demo pattern.
//!
//! Run with:
//!   `cargo run --example software_demo --features software-default-font`

use rg::Terminal;
use rg::backend::software::SoftwareBackendBuilder;
use rg::color::{AnsiColor, Color};
use rg::event::{Event, KeyCode};
use std::time::Duration;

// Fixed room size in world space (matches crossterm_demo).
const ROOM_LEFT: u16 = 2;
const ROOM_TOP: u16 = 2;
const ROOM_RIGHT: u16 = 40;
const ROOM_BOTTOM: u16 = 20;

fn main() {
    let backend = SoftwareBackendBuilder::new()
        .title("rg software demo")
        .grid_size(50, 25)
        .scale(2)
        .build()
        .expect("backend init failed (try the `software-default-font` feature)");

    let mut player_x: u16 = 5;
    let mut player_y: u16 = 5;

    backend
        .run(move |term: &mut Terminal<_>| {
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
            term.print(2, 0, "rg Library — Software Interactive Demo");
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
                            std::process::exit(0);
                        }
                        _ => {}
                    },
                    Event::Close => std::process::exit(0),
                    _ => {}
                }
            }
        })
        .expect("event loop failed");
}
