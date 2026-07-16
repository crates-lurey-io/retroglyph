//! Snapshot tests for the `13_combat_log` example.
//!
//! Includes the example's own source (rather than reimplementing its logic) via
//! `#[path]`, so these tests exercise exactly what `cargo run --example
//! 13_combat_log` runs.
//!
//! Like `11_sokoban`/`12_dungeon_scroll`, the headless snapshot drives synthetic key events
//! rather than snapshotting an idle frame -- here, the full fixed-damage fight to its end, so
//! the snapshot actually proves `StatBar`, `Log`, `Scrollbar`, and `Modal` all render correctly
//! together, not just that the opening frame does.

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/13_combat_log.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod combat_log;

use combat_log::CombatLog;
use retroglyph_core::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use retroglyph_core::{Frame, Headless, Terminal};
use retroglyph_examples::{Example, HEADLESS_FRAME_DELTA};

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
    for (i, event) in events.iter().enumerate() {
        term.backend_mut().push_event(event.clone());
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

/// `Headless::format_view` renders spaces as middle dots (see its own doc comment), so a plain
/// substring check against normal text would never match -- normalize dots back to spaces first.
fn normalize(view: &str) -> String {
    view.replace('\u{b7}', " ")
}

/// Six attacks: the fixed 7/5 damage exchange (see the example's own `attack` doc comment)
/// brings the 40-HP goblin down exactly on the sixth strike, without a final retaliation.
fn six_attacks() -> Vec<Event> {
    std::iter::repeat_n(key(KeyCode::Char('a')), 6).collect()
}

#[test]
fn headless_snapshot() {
    insta::assert_snapshot!(drive::<CombatLog>(&six_attacks()));
}

#[test]
fn six_attacks_defeats_the_goblin_and_shows_the_win_modal() {
    let view = normalize(&drive::<CombatLog>(&six_attacks()));
    let last_frame = view.rsplit("--- frame ---").next().unwrap_or(&view);
    assert!(
        last_frame.contains("You win!"),
        "expected the win modal:\n{last_frame}"
    );
    assert!(
        last_frame.contains("You: 5/30"),
        "expected the player to have survived at 5 hp:\n{last_frame}"
    );
    assert!(
        last_frame.contains("Goblin: 0/40"),
        "expected the goblin defeated:\n{last_frame}"
    );
}

#[test]
fn scrolling_the_log_shows_earlier_messages() {
    // Enough attacks to overflow the log's visible height, then one Up press: the view should
    // no longer show the newest message ("The goblin falls...") at the bottom.
    let mut events = six_attacks();
    events.push(key(KeyCode::Up));
    let view = normalize(&drive::<CombatLog>(&events));
    let last_frame = view.rsplit("--- frame ---").next().unwrap_or(&view);
    assert!(
        !last_frame.contains("The goblin falls"),
        "expected scrolling back to move the newest message out of view:\n{last_frame}"
    );
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<CombatLog>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    let bin = support::build_crossterm_example("13_combat_log");
    let raw = support::capture_pty(&bin, b"", 25, 50, "a: attack");
    let svg = support::svg_snapshot(&raw, 25, 50);
    assert!(svg.contains("attack"), "SVG output missing expected text");
    support::write_snapshot_file("13_combat_log.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}
