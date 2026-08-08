//! Headless smoke test for `examples/tutorial/03_a_map.rs`.
//!
//! Deliberately lighter than the `examples/examples/*.rs` gallery's three-snapshot treatment
//! (`examples/AGENTS.md`): the tutorial's job is to compile and run headless for `docs/src/
//! tutorial/`'s `{{#include}}` anchors, not to join the WASM gallery or pin pixel-exact output.
//! Drives synthetic key events through [`Headless::push_event`], the same shape
//! `examples/tests/11_sokoban.rs` uses, to prove the map renders and wall collision actually
//! blocks movement rather than only that the example compiles.

#![allow(unreachable_pub)]

#[path = "../tutorial/03_a_map.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by this test
mod a_map;

use a_map::AMap;
use retroglyph::app::Frame;
use retroglyph::backend::Headless;
use retroglyph::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use retroglyph::terminal::Terminal;
use retroglyph_examples::{Example, HEADLESS_FRAME_DELTA};

/// A plain, unmodified key press.
const fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

/// Drives `AMap` through one synthetic key event per tick, returning each frame's
/// [`Headless::format_view`] text.
fn drive(events: &[Event]) -> Vec<String> {
    let backend = Headless::new(50, 25);
    let mut term = Terminal::new(backend);
    let mut state = AMap::init(&mut term);

    let mut views = Vec::new();
    for (i, event) in events.iter().enumerate() {
        term.backend_mut().push_event(event.clone());
        let frame = Frame {
            delta: HEADLESS_FRAME_DELTA,
            frame: i as u64,
        };
        if !state.tick(&mut term, &frame) {
            break;
        }
        term.present().ok();
        views.push(term.backend().format_view());
    }
    views
}

/// Column of `@` in row `y`'s text. `chars().position()`, not `find()`: the default (empty)
/// tile's glyph is `·`, a multi-byte character, so a byte offset would overcount.
fn player_column(view: &str, y: usize) -> Option<usize> {
    view.lines().nth(y)?.chars().position(|c| c == '@')
}

#[test]
fn draws_the_level_and_the_player() {
    let views = drive(&[]);
    assert_eq!(views.len(), 0, "no events: nothing ticked yet");

    // A single no-op-ish tick (arrow key that doesn't hit a wall) to get one frame to inspect.
    let views = drive(&[key(KeyCode::Right)]);
    assert_eq!(views.len(), 1);
    let view = &views[0];
    assert!(view.contains('#'), "level walls should be drawn:\n{view}");
    assert!(view.contains('@'), "player should be drawn:\n{view}");
}

#[test]
fn walls_block_movement() {
    // Player starts at (2, 2), one cell in from the top-left corner's walls. Repeatedly
    // pushing up/left should stop exactly at (1, 1), the first floor cell, rather than walking
    // onto the `#` border.
    let events: Vec<Event> = std::iter::repeat_n(key(KeyCode::Up), 3)
        .chain(std::iter::repeat_n(key(KeyCode::Left), 3))
        .collect();
    let views = drive(&events);
    assert_eq!(views.len(), 6, "walls should block, not quit, the example");
    let last = views.last().expect("at least one frame");
    assert_eq!(player_column(last, 1), Some(1));
}
