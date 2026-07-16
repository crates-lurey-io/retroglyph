//! Snapshot tests for the `07_sprites_tileset` example.
//!
//! Includes the example's own source (rather than reimplementing its logic) via
//! `#[path]`, so these tests exercise exactly what `cargo run --example
//! 07_sprites_tileset` runs.
//!
//! The headless snapshot drives synthetic movement through
//! [`Headless::push_event`] to actually walk the player onto a coin -- this is what
//! proves collection (and the score readout) work, not just that the room renders.
//! The PNG snapshot is the one that matters most for this example specifically: it's
//! the only one of the three where `Example::configure_software`'s tileset
//! registration (see `support::png_snapshot`'s doc comment) actually renders real
//! sprites instead of the bitmap font.

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/07_sprites_tileset.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod sprites_tileset;

use retroglyph_core::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use retroglyph_core::{Frame, Headless, Terminal};
use retroglyph_examples::{Example, HEADLESS_FRAME_DELTA};
use sprites_tileset::SpritesTileset;

/// A plain, unmodified key press.
const fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn headless_snapshot() {
    // Walks the player left 5 then down 2, from its spawn point onto the coin at
    // `COIN_OFFSETS[3]` -- proves movement, collision-free floor traversal, and coin
    // collection (the score readout incrementing) all in one driven sequence.
    let events = [
        key(KeyCode::Left),
        key(KeyCode::Left),
        key(KeyCode::Left),
        key(KeyCode::Left),
        key(KeyCode::Left),
        key(KeyCode::Down),
        key(KeyCode::Down),
    ];

    let backend = Headless::new(50, 25);
    let mut term = Terminal::new(backend);
    let mut state = SpritesTileset::init(&mut term);

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
    insta::assert_snapshot!(views.join("\n--- frame ---\n"));
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<SpritesTileset>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    let bin = support::build_crossterm_example("07_sprites_tileset");
    let raw = support::capture_pty(&bin, b"", 25, 50, "Score:");
    let svg = support::svg_snapshot(&raw, 25, 50);
    assert!(
        svg.contains("collect"),
        "SVG output missing expected header text"
    );
    support::write_snapshot_file("07_sprites_tileset.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}
