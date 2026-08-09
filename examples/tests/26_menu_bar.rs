//! Snapshot tests for the `26_menu_bar` example.
//!
//! Includes the example's own source (rather than reimplementing its logic) via `#[path]`, so
//! these tests exercise exactly what `cargo run --example 26_menu_bar` runs.
//!
//! `headless_snapshot_click_opens_and_hover_switches` drives the two behaviors retroglyph#1354
//! adds on top of `Menu` itself: a click opens "File", then a hover over "Edit" (with no click)
//! switches the open popup to it, proving the "hovering another label while one is open switches
//! to it" rule. `headless_snapshot_left_right_and_activation` opens "File" by click, steps to
//! "Edit" with Right, navigates to "Word Wrap" with Down, and activates it with Enter.

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/26_menu_bar.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod menu_bar_example;

use menu_bar_example::MenuBarDemo;
use retroglyph::TestHarness;
use retroglyph::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use retroglyph::grid::Pos;
use retroglyph_examples::Example;
use support::TestApp;

/// Drives `MenuBarDemo` through `events` (one batch of zero or more events per tick), returning
/// each frame's rendered text.
fn drive(events: &[&[Event]]) -> String {
    let mut harness = TestHarness::new(50, 25);
    let mut app = TestApp(MenuBarDemo::init(harness.term_mut()));

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
fn headless_snapshot_click_opens_and_hover_switches() {
    // Two kinds of one-frame latency stack here, both documented on `MenuBar::show`: a mouse
    // event fed during frame N only resolves (`Response::clicked`/`hovered`) at frame N+1's own
    // `show` call (the same latency every `Response` in this crate has), and a state change
    // `show` makes as a *result* of resolving that isn't drawn until frame N+2, since `show`
    // already decided what to draw from `open_label()` before resolving this frame's gesture.
    //
    // Frame 1: registers the bar's hitbox. Frame 2: press+release land on "File" (x=0..4, y=0).
    // Frame 3: resolves the click, opening "File" in `open_label()` (not drawn yet). Frame 4:
    // "File"'s popup finally draws. Frame 5: the pointer moves to "Edit" (x=5..9), with no
    // click. Frame 6: resolves the hover, switching `open_label()` to "Edit" (not drawn yet).
    // Frame 7: "Edit"'s popup finally draws.
    let down = mouse_event(MouseEventKind::Down(MouseButton::Left), 1, 0);
    let up = mouse_event(MouseEventKind::Up(MouseButton::Left), 1, 0);
    let hover_edit = mouse_event(MouseEventKind::Moved, 6, 0);
    let events: [&[Event]; 7] = [&[], &[down, up], &[], &[], &[hover_edit], &[], &[]];
    let view = drive(&events);
    assert!(
        view.contains("Cut"),
        "expected the final frame to show Edit's popup (hover-switched from File), got:\n{view}"
    );
    insta::assert_snapshot!(view);
}

#[test]
fn headless_snapshot_left_right_and_activation() {
    // Frame 1: registers the bar's hitbox. Frame 2: press+release land on "File". Frame 3:
    // resolves the click, opening "File" with "New" selected. Frame 4: Right steps to "Edit".
    // Frame 5: Down moves from "Cut" to "Copy". Frame 6: Enter activates "Copy" and closes it.
    let down = mouse_event(MouseEventKind::Down(MouseButton::Left), 1, 0);
    let up = mouse_event(MouseEventKind::Up(MouseButton::Left), 1, 0);
    let events: [&[Event]; 6] = [
        &[],
        &[down, up],
        &[],
        &[key(KeyCode::Right)],
        &[key(KeyCode::Down)],
        &[key(KeyCode::Enter)],
    ];
    let view = drive(&events);
    assert!(
        view.contains("Last\u{b7}action:\u{b7}Copy"),
        "expected the final frame to show Edit's \"Copy\" activated, got:\n{view}"
    );
    insta::assert_snapshot!(view);
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<MenuBarDemo>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    let bin = support::build_crossterm_example("26_menu_bar");
    let raw = support::capture_pty(&bin, b"", 25, 50, "File");
    let svg = support::svg_snapshot(&raw, 25, 50);
    assert!(svg.contains("File"), "SVG output missing expected label");
    support::write_snapshot_file("26_menu_bar.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}
