//! Snapshot tests for the `25_menu` example.
//!
//! Includes the example's own source (rather than reimplementing its logic) via `#[path]`, so
//! these tests exercise exactly what `cargo run --example 25_menu` runs.
//!
//! `headless_snapshot_click_opens_navigates_and_activates` drives the full loop retroglyph#1290
//! is about: a click resolves one frame after the press/release pair arrives (same one-frame
//! latency `10_widgets_interaction`'s tests document), opening the menu; Up/Down then skip the
//! separator between "Open" and "Word Wrap"; Enter activates and closes it.
//! `headless_snapshot_click_outside_closes_without_firing` proves the click-outside/backdrop half
//! separately: the menu opens, a click lands well outside its popup rect, and it closes with
//! `last_action` left untouched.

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/25_menu.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod menu_example;

use menu_example::MenuDemo;
use retroglyph_core::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use retroglyph_core::grid::Pos;
use retroglyph_core::testing::TestHarness;
use retroglyph_examples::Example;
use support::TestApp;

/// Drives `MenuDemo` through `events` (one batch of zero or more events per tick), returning
/// each frame's rendered text.
fn drive(events: &[&[Event]]) -> String {
    let mut harness = TestHarness::new(50, 25);
    let mut app = TestApp(MenuDemo::init(harness.term_mut()));

    let mut views = Vec::new();
    for batch in events {
        for event in *batch {
            harness.term_mut().backend_mut().push_event(event.clone());
        }
        harness.step(&mut app);
        views.push(harness.view());
    }
    views.join("\n--- frame ---\n")
}

const fn mouse_event(kind: MouseEventKind, x: u16, y: u16) -> Event {
    Event::Mouse(MouseEvent::new(kind, Pos { x, y }, KeyModifiers::NONE))
}

const fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn headless_snapshot_click_opens_navigates_and_activates() {
    // Frame 1: registers the "File" button's hitbox. Frame 2: press+release land on it (not
    // resolved until frame 3, one-frame click latency). Frame 3: resolves the click, opening the
    // menu with "New" selected. Frame 4/5: Down skips "Open" then the separator, landing on
    // "Word Wrap". Frame 6: Enter activates it (toggling word wrap off) and closes the menu.
    let down = mouse_event(MouseEventKind::Down(MouseButton::Left), 2, 1);
    let up = mouse_event(MouseEventKind::Up(MouseButton::Left), 2, 1);
    let events: [&[Event]; 6] = [
        &[],
        &[down, up],
        &[],
        &[key(KeyCode::Down)],
        &[key(KeyCode::Down)],
        &[key(KeyCode::Enter)],
    ];
    let view = drive(&events);
    assert!(
        view.contains("Word\u{b7}Wrap:\u{b7}off"),
        "expected the final frame to show the toggled action, got:\n{view}"
    );
    insta::assert_snapshot!(view);
}

#[test]
fn headless_snapshot_click_outside_closes_without_firing() {
    let down = mouse_event(MouseEventKind::Down(MouseButton::Left), 2, 1);
    let up = mouse_event(MouseEventKind::Up(MouseButton::Left), 2, 1);
    // Well outside the popup, which opens below/near the "File" trigger in the screen's
    // top-left corner.
    let outside_down = mouse_event(MouseEventKind::Down(MouseButton::Left), 45, 20);
    let outside_up = mouse_event(MouseEventKind::Up(MouseButton::Left), 45, 20);
    let events: [&[Event]; 5] = [&[], &[down, up], &[], &[outside_down, outside_up], &[]];
    let view = drive(&events);
    assert!(
        view.contains("(none\u{b7}yet)"),
        "outside click must close without activating a row, got:\n{view}"
    );
    insta::assert_snapshot!(view);
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<MenuDemo>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    let bin = support::build_crossterm_example("25_menu");
    let raw = support::capture_pty(&bin, b"", 25, 50, "File");
    let svg = support::svg_snapshot(&raw, 25, 50);
    assert!(
        svg.contains("File"),
        "SVG output missing expected trigger label"
    );
    support::write_snapshot_file("25_menu.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}
