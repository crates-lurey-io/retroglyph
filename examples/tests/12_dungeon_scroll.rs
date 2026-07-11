//! Snapshot tests for the `12_dungeon_scroll` example.
//!
//! Includes the example's own source (rather than reimplementing its logic) via
//! `#[path]`, so these tests exercise exactly what `cargo run --example
//! 12_dungeon_scroll` runs.
//!
//! Like `03_keyboard`/`04_mouse`/`11_sokoban`, the headless snapshot drives synthetic key events
//! rather than snapshotting an idle frame -- here, a walk from room 1 all the way to room 4
//! along the corridors, so the snapshot actually proves the camera scrolls and clamps at both
//! edges, not just that the starting room renders once.

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/12_dungeon_scroll.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod dungeon_scroll;

use dungeon_scroll::DungeonScroll;
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

/// The full corridor walk from room 1's center to room 4's center: right along the first
/// corridor, down along the second, right along the third -- see `CORRIDORS` in the example
/// itself for why these are straight lines with no diagonal movement needed.
fn walk_to_room_four() -> Vec<Event> {
    let mut events = Vec::new();
    events.extend(std::iter::repeat_n(key(KeyCode::Right), 34)); // (6,4) -> (40,4)
    events.extend(std::iter::repeat_n(key(KeyCode::Down), 26)); // (40,4) -> (40,30)
    events.extend(std::iter::repeat_n(key(KeyCode::Right), 40)); // (40,30) -> (80,30)
    events
}

#[test]
fn headless_snapshot() {
    insta::assert_snapshot!(drive::<DungeonScroll>(&walk_to_room_four()));
}

/// The camera must have actually scrolled by the time the player reaches room 4: the first
/// frame's view (room 1, near the world's top-left) and the last frame's view (room 4, clamped
/// against the world's bottom-right) must differ.
#[test]
fn the_camera_scrolls_as_the_player_crosses_the_world() {
    let view = drive::<DungeonScroll>(&walk_to_room_four());
    let frames: Vec<&str> = view.split("--- frame ---").collect();
    let first = frames.first().copied().unwrap_or_default();
    let last = frames.last().copied().unwrap_or_default();
    assert_ne!(
        first, last,
        "expected the rendered view to change as the camera follows the player"
    );
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<DungeonScroll>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    let bin = support::build_crossterm_example("12_dungeon_scroll");
    let raw = support::capture_pty(&bin, b"", 25, 50, "Dungeon scroll");
    let svg = support::svg_snapshot(&raw, 25, 50);
    assert!(
        svg.contains("Dungeon scroll"),
        "SVG output missing expected text"
    );
    support::write_snapshot_file("12_dungeon_scroll.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}
