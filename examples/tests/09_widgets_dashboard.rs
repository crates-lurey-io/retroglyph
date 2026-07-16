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

use retroglyph_core::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use retroglyph_core::{Frame, Headless, Pos, Terminal};
use retroglyph_examples::{Example, HEADLESS_FRAME_DELTA};
use widgets_dashboard::Dashboard;

/// A plain, unmodified key press.
const fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

const fn mouse_event(kind: MouseEventKind, x: u16, y: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        position: Pos { x, y },
        pixel_position: None,
        modifiers: KeyModifiers::NONE,
    })
}

/// Drives `Dashboard` through `events` (one batch of zero or more events per tick), returning
/// each frame's rendered text.
fn drive(events: &[&[Event]]) -> String {
    let backend = Headless::new(50, 25);
    let mut term = Terminal::new(backend);
    let mut state = Dashboard::init(&mut term);

    let mut views = Vec::new();
    for (i, batch) in events.iter().enumerate() {
        for event in *batch {
            term.backend_mut().push_event(event.clone());
        }
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

#[test]
fn headless_snapshot() {
    // Two Down presses move the table's highlighted row from "api-gateway" (the default
    // selection) to "billing" -- proves `Table`/`ListState`'s highlight actually tracks
    // input, not just that the widget renders once statically.
    insta::assert_snapshot!(drive(&[&[key(KeyCode::Down)], &[key(KeyCode::Down)]]));
}

#[test]
fn headless_snapshot_ping_button_click() {
    // Frame 1: registers the dashboard's widgets, including the Metrics tab's "Ping" button, for
    // frame 2's hit-test (column 33, row 12 is inside the button's rect -- see
    // `Dashboard::draw_ping_button`). Frame 2: resolves the click one frame later (see
    // `Interaction`'s own doc comment on why), incrementing the ping counter -- the same
    // click-resolution proof `10_widgets_interaction` runs on its own buttons.
    let down = mouse_event(MouseEventKind::Down(MouseButton::Left), 33, 12);
    let up = mouse_event(MouseEventKind::Up(MouseButton::Left), 33, 12);
    insta::assert_snapshot!(drive(&[&[down, up], &[]]));
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
