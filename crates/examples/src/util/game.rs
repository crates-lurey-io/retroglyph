//! Shared game loop for rg examples.
#![allow(dead_code)] // not every example uses every item in this module
//!
//! The [`GameState`] struct and [`tick`] function are used by both the
//! interactive demos and the headless demo, which injects events and
//! inspects the grid directly — the same technique used in unit tests.

use retroglyph_core::color::{AnsiColor, Color};
use retroglyph_core::event::{Event, KeyCode};
use retroglyph_core::{Backend, Pos, Terminal};

// Fixed room size in world space. The terminal clips on small windows rather
// than resizing the room, so positions are stable across resize events.
const ROOM_LEFT: u16 = 2;
const ROOM_TOP: u16 = 2;
const ROOM_RIGHT: u16 = 40;
const ROOM_BOTTOM: u16 = 20;

/// State for the shared interactive demo.
///
/// Create with [`GameState::new`] and drive one frame at a time with [`tick`].
pub struct GameState {
    /// Player position in grid coordinates.
    pub player: Pos,
}

impl GameState {
    /// Create a new game state with the player at the default starting position.
    pub const fn new<B: Backend>(_term: &mut Terminal<B>) -> Self {
        Self {
            player: Pos::new(5, 5),
        }
    }
}

/// Run one tick of the demo game loop.
///
/// Polls for input first, updates state, then draws and presents the frame.
/// Input is therefore visible in the same frame it arrives — no one-frame lag.
///
/// Returns `false` when the user requests quit (Esc or Q), signalling the
/// caller to stop the loop rather than calling `process::exit` directly —
/// backends like `Crossterm` restore the terminal on `Drop`.
pub fn tick(term: &mut Terminal<impl Backend>, state: &mut GameState) -> bool {
    // Poll for input and update state before drawing.
    //
    // drain_events pulls *all* buffered events. This is required for the software
    // backend on WASM, where requestAnimationFrame gates frame delivery to ~60 fps.
    // If we only poll one event per frame, rapid keypresses replay in slow motion.
    // On crossterm the loop is uncapped, so drain vs poll makes no visible difference.
    for event in term.drain_events() {
        match event {
            Event::Key(key_event) if key_event.is_down() => match key_event.code {
                KeyCode::Up | KeyCode::Char('w') => {
                    if state.player.y > ROOM_TOP + 1 {
                        state.player.y -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('s') => {
                    if state.player.y < ROOM_BOTTOM - 1 {
                        state.player.y += 1;
                    }
                }
                KeyCode::Left | KeyCode::Char('a') => {
                    if state.player.x > ROOM_LEFT + 1 {
                        state.player.x -= 1;
                    }
                }
                KeyCode::Right | KeyCode::Char('d') => {
                    if state.player.x < ROOM_RIGHT - 1 {
                        state.player.x += 1;
                    }
                }
                KeyCode::Escape | KeyCode::Char('q') => return false,
                _ => {}
            },
            Event::Close => return false,
            _ => {}
        }
    }

    let size = term.size();

    // 2. Draw room boundary (box-drawing characters)
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

    // 3. Draw enemy (red D) at room center
    term.fg(Color::Ansi(AnsiColor::Red));
    term.put(
        u16::midpoint(ROOM_LEFT, ROOM_RIGHT),
        u16::midpoint(ROOM_TOP, ROOM_BOTTOM),
        'D',
    );

    // 4. Draw player at the (possibly just-updated) position
    term.fg(Color::Ansi(AnsiColor::Green));
    term.put(state.player.x, state.player.y, '@');
    term.reset_style();

    // 5. Status lines
    term.print(2, 0, "rg Library — Interactive Demo");
    term.print(
        2,
        size.height.saturating_sub(1),
        "HP: 100 | Arrow Keys to move, Q or ESC to Quit",
    );

    term.present().expect("present failed");

    true
}
