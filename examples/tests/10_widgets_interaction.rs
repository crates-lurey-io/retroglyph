//! Snapshot tests for the `10_widgets_interaction` example.
//!
//! Includes the example's own source (rather than reimplementing its logic) via
//! `#[path]`, so these tests exercise exactly what `cargo run --example
//! 10_widgets_interaction` runs.
//!
//! Both headless tests drive synthetic input through [`Headless::push_event`], mirroring
//! `04_mouse`'s and `03_keyboard`'s decode-and-echo proofs -- but here what's being proven is
//! `Interaction`'s resolution, not raw event decode: a mouse click resolves one frame after the
//! press/release pair arrives (see [`Interaction`](retroglyph_widgets::Interaction)'s own doc
//! comment on why), while Tab-focus and Enter-activation resolve the same frame their event
//! arrives, since `FocusRing` state is read live rather than snapshotted.

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/10_widgets_interaction.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod widgets_interaction;

use retroglyph_core::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use retroglyph_core::{Frame, Headless, Pos, Terminal};
use retroglyph_examples::{Example, HEADLESS_FRAME_DELTA};
use widgets_interaction::WidgetsInteraction;

/// Drives `WidgetsInteraction` through `events` (one batch of zero or more events per tick),
/// returning each frame's rendered text.
fn drive(events: &[&[Event]]) -> String {
    let backend = Headless::new(50, 25);
    let mut term = Terminal::new(backend);
    let mut state = WidgetsInteraction::init(&mut term);

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

const fn mouse_event(kind: MouseEventKind, x: u16, y: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        position: Pos { x, y },
        pixel_position: None,
        modifiers: KeyModifiers::NONE,
    })
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
