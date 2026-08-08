//! Headless smoke test for `examples/tutorial/02_input.rs`.
//!
//! Deliberately lighter than the `examples/examples/*.rs` gallery's three-snapshot treatment
//! (`examples/AGENTS.md`): the tutorial's job is to compile and run headless for `docs/src/
//! tutorial/`'s `{{#include}}` anchors, not to join the WASM gallery or pin pixel-exact output.
//! Drives synthetic key events through [`Headless::push_event`], the same shape
//! `examples/tests/03_keyboard.rs` uses, to prove movement and quitting actually work rather
//! than only that the example compiles.

#![allow(unreachable_pub)]

#[path = "../tutorial/02_input.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by this test
mod input;

use input::Input;
use retroglyph_core::app::Frame;
use retroglyph_core::backend::Headless;
use retroglyph_core::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use retroglyph_core::terminal::Terminal;
use retroglyph_examples::{Example, HEADLESS_FRAME_DELTA};

/// A plain, unmodified key press.
const fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

/// Drives `Input` through one synthetic key event per tick, returning each frame's
/// [`Headless::format_view`] text. Mirrors `examples/tests/03_keyboard.rs`'s own helper.
fn drive(events: &[Event]) -> Vec<String> {
    let backend = Headless::new(50, 25);
    let mut term = Terminal::new(backend);
    let mut state = Input::init(&mut term);

    let mut views = Vec::new();
    for (i, event) in events.iter().enumerate() {
        term.backend_mut().push_event(event.clone());
        let frame = Frame::new(HEADLESS_FRAME_DELTA, i as u64);
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
fn moves_with_arrow_keys() {
    // Starts at (25, 12); one step right should move `@` one column over.
    let views = drive(&[key(KeyCode::Right)]);
    assert_eq!(views.len(), 1);
    assert_eq!(player_column(&views[0], 12), Some(26));
}

#[test]
fn quits_on_q() {
    let views = drive(&[
        key(KeyCode::Right),
        key(KeyCode::Char('q')),
        key(KeyCode::Right),
    ]);
    // The quitting frame's `tick` returns before drawing, so only the first move is recorded.
    assert_eq!(views.len(), 1);
}
