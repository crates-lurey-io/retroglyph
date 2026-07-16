//! Snapshot tests for the `09_widgets_dashboard` example.
//!
//! Includes the example's own source (rather than reimplementing its logic) via
//! `#[path]`, so these tests exercise exactly what `cargo run --example
//! 09_widgets_dashboard` runs.

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/09_widgets_dashboard.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod widgets_dashboard;

use retroglyph_core::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use retroglyph_core::{Frame, Headless, Terminal};
use retroglyph_examples::{Example, HEADLESS_FRAME_DELTA};
use widgets_dashboard::Dashboard;

/// A plain, unmodified key press.
const fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn headless_snapshot() {
    // Two Down presses move the table's highlighted row from "api-gateway" (the default
    // selection) to "billing" -- proves `Table`/`ListState`'s highlight actually tracks
    // input, not just that the widget renders once statically.
    let events = [key(KeyCode::Down), key(KeyCode::Down)];

    let backend = Headless::new(50, 25);
    let mut term = Terminal::new(backend);
    let mut state = Dashboard::init(&mut term);

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
    let png = support::png_snapshot::<Dashboard>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    let bin = support::build_crossterm_example("09_widgets_dashboard");
    let raw = support::capture_pty(&bin, b"", 25, 50, "retroglyph dashboard");
    let svg = support::svg_snapshot(&raw, 25, 50);
    assert!(
        svg.contains("api-gateway"),
        "SVG output missing expected table content"
    );
    support::write_snapshot_file("09_widgets_dashboard.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}
