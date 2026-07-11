//! Snapshot tests for the `03_keyboard` example.
//!
//! Includes the example's own source (rather than reimplementing its logic) via
//! `#[path]`, so these tests exercise exactly what `cargo run --example 03_keyboard` runs.
//!
//! Unlike `01_hello_world`/`02_colors` (static content, driven with no input),
//! `03_keyboard`'s whole point is decode-and-echo, so its headless snapshot injects
//! synthetic key events through [`Headless::push_event`] before each tick -- this is
//! what actually proves key decode is stable, and it's the same path the WASM
//! `decode_key` FFI feeds into (see `examples/src/util/wasm_headless.rs`'s own tests).

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/03_keyboard.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod keyboard;

use keyboard::Keyboard;
use retroglyph_core::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use retroglyph_core::{Frame, Headless, Terminal};
use retroglyph_examples::{Example, HEADLESS_FRAME_DELTA};

/// Drives `E` through one synthetic key event per tick, returning each frame's
/// [`Headless::format_view`] text -- mirrors `support::headless_snapshot`'s shape but
/// injects `events` before each tick rather than ticking on an empty queue.
fn headless_keyboard_snapshot<E: Example>(events: &[Event]) -> String {
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

/// A plain, unmodified key press -- shorthand for the common case in the event list below.
const fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn headless_snapshot() {
    let events = [
        key(KeyCode::Char('a')),
        key(KeyCode::Left),
        key(KeyCode::Right),
        key(KeyCode::Up),
        key(KeyCode::Down),
        key(KeyCode::F(5)),
        Event::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
        Event::Key(KeyEvent::new(
            KeyCode::Char('z'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        )),
        key(KeyCode::Escape),
    ];
    insta::assert_snapshot!(headless_keyboard_snapshot::<Keyboard>(&events));
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<Keyboard>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    let bin = support::build_crossterm_example("03_keyboard");
    let raw = support::capture_pty(&bin, b"", 25, 50, "Press any key");
    let svg = support::svg_snapshot(&raw, 25, 50);
    assert!(
        svg.contains("Press any key"),
        "SVG output missing expected text"
    );
    support::write_snapshot_file("03_keyboard.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}
