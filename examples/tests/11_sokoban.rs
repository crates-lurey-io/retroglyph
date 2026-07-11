//! Snapshot tests for the `11_sokoban` example.
//!
//! Includes the example's own source (rather than reimplementing its logic) via
//! `#[path]`, so these tests exercise exactly what `cargo run --example 11_sokoban` runs.
//!
//! Like `03_keyboard`/`04_mouse`, the headless snapshot drives synthetic key events rather
//! than snapshotting an idle frame -- here, the full solve sequence for `LEVEL`, so the
//! snapshot actually proves push/win logic works, not just that the board renders once.

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/11_sokoban.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod sokoban;

use retroglyph_core::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use retroglyph_core::{Frame, Headless, Terminal};
use retroglyph_examples::{Example, HEADLESS_FRAME_DELTA};
use sokoban::Sokoban;

/// A plain, unmodified key press.
const fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

/// Drives `E` through one synthetic key event per tick, returning each frame's
/// [`Headless::format_view`] text.
fn drive<E: Example>(events: &[Event]) -> String {
    let backend = Headless::new(50, 25);
    let mut term = Terminal::new(backend);
    let mut state = E::init(&mut term);

    let mut views = Vec::new();
    for (i, &event) in events.iter().enumerate() {
        term.backend_mut().push_event(event);
        let frame = Frame {
            delta: HEADLESS_FRAME_DELTA,
            frame: i as u64,
        };
        if !state.tick(&mut term, &frame) {
            break;
        }
        views.push(term.backend().format_view());
    }
    views.join("\n--- frame ---\n")
}

/// The intended solve for `LEVEL`: walk left to below the left box, push it up onto its goal,
/// walk around below the right box, push it up onto its goal too.
const fn solve_sequence() -> [Event; 9] {
    [
        key(KeyCode::Left),
        key(KeyCode::Left),
        key(KeyCode::Up),
        key(KeyCode::Down),
        key(KeyCode::Right),
        key(KeyCode::Right),
        key(KeyCode::Right),
        key(KeyCode::Right),
        key(KeyCode::Up),
    ]
}

#[test]
fn headless_snapshot() {
    insta::assert_snapshot!(drive::<Sokoban>(&solve_sequence()));
}

/// `Headless::format_view` renders spaces as middle dots (see its own doc comment), so a plain
/// `"Moves: 9"` substring check would never match -- normalize dots back to spaces first.
fn normalize(view: &str) -> String {
    view.replace('\u{b7}', " ")
}

#[test]
fn pushing_a_box_onto_both_goals_wins() {
    let view = normalize(&drive::<Sokoban>(&solve_sequence()));
    assert!(
        view.contains("Moves: 9"),
        "expected 9 recorded moves:\n{view}"
    );
    assert!(
        view.contains("Solved!"),
        "expected the win message:\n{view}"
    );
}

#[test]
fn undo_reverts_the_last_move_and_decrements_the_counter() {
    // One step left, then undo: the counter should land back at zero.
    let events = [key(KeyCode::Left), key(KeyCode::Char('u'))];
    let view = normalize(&drive::<Sokoban>(&events));
    let last_frame = view.rsplit("--- frame ---").next().unwrap_or(&view);
    assert!(
        last_frame.contains("Moves: 0"),
        "expected the move counter back at 0:\n{view}"
    );
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<Sokoban>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    let bin = support::build_crossterm_example("11_sokoban");
    let raw = support::capture_pty(&bin, b"", 25, 50, "Sokoban");
    let svg = support::svg_snapshot(&raw, 25, 50);
    assert!(svg.contains("Sokoban"), "SVG output missing expected text");
    support::write_snapshot_file("11_sokoban.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}
