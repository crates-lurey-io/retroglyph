//! Minimal `Headless` backend demo.
//!
//! Shows the smallest possible use of [`Headless`]: build a `Terminal`,
//! draw a frame, inject a synthetic key event, tick, and print the grid
//! before/after via [`Headless::format_view`] -- the same technique used in
//! this crate's own unit and integration tests. No terminal or window is
//! required; this only depends on `retroglyph-core` itself, so it's the
//! right starting point if you haven't picked a backend crate yet.
//!
//! Run with: `cargo run -p retroglyph-core --example headless`

use retroglyph_core::Headless;
use retroglyph_core::Terminal;
use retroglyph_core::event::{Event, KeyCode, KeyEvent, KeyModifiers};

fn main() {
    let backend = Headless::new(10, 3);
    let mut term = Terminal::new(backend);

    // Frame 1: draw a lone `@` and present it.
    term.put(1, 1, '@');
    term.present().unwrap();
    println!("--- Frame 1 ---");
    println!("{}", term.backend().format_view());

    // Inject a synthetic "move right" key event, same as a real backend
    // would push from its own input source.
    term.backend_mut().push_event(Event::Key(KeyEvent::new(
        KeyCode::Right,
        KeyModifiers::NONE,
    )));

    // A real app's event loop would call `drain_events`/`poll_event` here to
    // read the injected event back and move `@` in response; this demo just
    // shows the injection landing in the queue, then redraws one cell over
    // to keep the example self-contained.
    let _ = term.drain_events();
    term.put(1, 1, ' ');
    term.put(2, 1, '@');
    term.present().unwrap();
    println!("--- Frame 2 (after injecting a move-right event) ---");
    println!("{}", term.backend().format_view());
}
