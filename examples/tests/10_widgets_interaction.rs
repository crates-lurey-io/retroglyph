//! Snapshot tests for the `10_widgets_interaction` example.
//!
//! Includes the example's own source (rather than reimplementing its logic) via
//! `#[path]`, so these tests exercise exactly what `cargo run --example
//! 10_widgets_interaction` runs.
//!
//! Both headless tests drive synthetic input through [`Headless::push_event`], mirroring
//! `04_mouse`'s and `03_keyboard`'s decode-and-echo proofs -- but here what's being proven is
//! `Interaction`'s resolution, not raw event decode: a mouse click resolves one frame after the
//! press/release pair arrives (see [`Interaction`](retroglyph::ui::interact::Interaction)'s own doc
//! comment on why), while Tab-focus and Enter-activation resolve the same frame their event
//! arrives, since `FocusRing` state is read live rather than snapshotted.

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/10_widgets_interaction.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod widgets_interaction;

use retroglyph::TestHarness;
use retroglyph::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use retroglyph::grid::Pos;
use retroglyph_examples::Example;
use support::TestApp;
use widgets_interaction::WidgetsInteraction;

/// Drives `WidgetsInteraction` through `events` (one batch of zero or more events per tick),
/// returning each frame's rendered text.
fn drive(events: &[&[Event]]) -> String {
    let mut harness = TestHarness::new(50, 25);
    let mut app = TestApp(WidgetsInteraction::init(harness.term_mut()));

    let mut views = Vec::new();
    for batch in events {
        // Pushed straight onto the backend (bypassing `TestHarness`'s own single-event queue),
        // like the raw `Headless::push_event` this replaces: a whole batch has to land in the
        // same `update` call, not be drip-fed one event per `step`.
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
fn headless_snapshot_click_increments() {
    // Frame 1: press+release land on the Increment button's rect (registered by frame 1's own
    // draw pass, read back at frame 2's begin_frame -- see the module doc comment). Frame 2:
    // resolves the click, incrementing the counter.
    let down = mouse_event(MouseEventKind::Down(MouseButton::Left), 5, 4);
    let up = mouse_event(MouseEventKind::Up(MouseButton::Left), 5, 4);
    insta::assert_snapshot!(drive(&[&[down, up], &[]]));
}

/// Regression test for retroglyph#1262: `TestHarness::click` queues Down and Up as two entries,
/// so [`TestHarness::step`] (unlike this file's own `drive`, which pushes a whole batch straight
/// onto the backend before one `step`) lands them in two separate `Interaction::frame` calls, one
/// frame later than `drive`'s single-frame batch above -- costing a third, event-free frame before
/// `resolved_release` is observed. `run`/`settle` must run that trailing frame themselves, or a
/// click queued through the documented `TestHarness::click` + `run` pairing is never resolved.
#[test]
fn harness_click_resolves_via_run() {
    let mut harness = TestHarness::new(50, 25);
    let mut app = TestApp(WidgetsInteraction::init(harness.term_mut()));

    // Registers the buttons' hitboxes for this click to land on.
    harness.step(&mut app);

    harness.click(5, 4);
    harness.run(&mut app);

    // `Headless::format_view` renders every space glyph as `·` for visibility, including the
    // one `format!("Count: {count}")` prints.
    assert!(
        harness.view().contains("Count:\u{b7}1"),
        "click queued via TestHarness::click + run should resolve, got:\n{}",
        harness.view()
    );
}

#[test]
fn headless_snapshot_keyboard_focus_activate_and_reset() {
    // Frame 1: no input, just registers the buttons in the focus ring for frame 2's Tab to
    // cycle through. Frame 2: Tab focuses "Increment". Frame 3: Enter activates it (count -> 1).
    // Frame 4: `r` resets the counter via `Shortcuts`' global binding, independent of focus.
    let events: [&[Event]; 4] = [
        &[],
        &[key(KeyCode::Tab)],
        &[key(KeyCode::Enter)],
        &[key(KeyCode::Char('r'))],
    ];
    insta::assert_snapshot!(drive(&events));
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<WidgetsInteraction>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    let bin = support::build_crossterm_example("10_widgets_interaction");
    let raw = support::capture_pty(&bin, b"", 25, 50, "Tab/Shift+Tab");
    let svg = support::svg_snapshot(&raw, 25, 50);
    assert!(
        svg.contains("Increment"),
        "SVG output missing expected button label"
    );
    support::write_snapshot_file("10_widgets_interaction.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}
