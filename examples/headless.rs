//! Headless backend demo.
//!
//! Shows how to drive the shared game loop against the [`Headless`] backend
//! by injecting events and inspecting the rendered grid — the same technique
//! used in unit and integration tests. No terminal or window is required.
//!
//! Run with: `cargo run --example headless`

mod util;

use retroglyph::Terminal;
use retroglyph::backend::Headless;
use retroglyph::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use util::game::{GameState, tick};

fn main() {
    let backend = Headless::new(50, 25);
    let mut term = Terminal::new(backend);
    let mut state = GameState::new(&mut term);

    // Frame 1: initial render
    tick(&mut term, &mut state);
    println!("--- Frame 1 ---");
    println!("{}", term.backend().grid());

    // Inject a move-right event, then let tick consume it
    term.backend_mut().push_event(Event::Key(KeyEvent {
        code: KeyCode::Right,
        modifiers: KeyModifiers::NONE,
    }));

    tick(&mut term, &mut state);
    println!("--- Frame 2 (after move right) ---");
    println!("{}", term.backend().grid());
}
