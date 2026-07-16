//! Snapshot tests for the `04_mouse` example.
//!
//! Includes the example's own source (rather than reimplementing its logic) via
//! `#[path]`, so these tests exercise exactly what `cargo run --example 04_mouse` runs.
//!
//! `04_mouse` is the reference graceful-fallback example (see its own doc comment), so
//! its headless snapshots cover both branches: driving a `Moved` event through
//! [`Headless::push_event`] proves the tracked path, and ticking with no events at all
//! (headless's usual "no live input" state) proves the fallback note actually appears
//! once `MOTION_GRACE_TICKS` elapses, rather than silently rendering a blank frame.

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/04_mouse.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod mouse;

use mouse::Mouse;
use retroglyph_core::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use retroglyph_core::{Frame, Headless, Pos, Terminal};
use retroglyph_examples::{Example, HEADLESS_FRAME_DELTA};

const fn mouse_event(kind: MouseEventKind, x: u16, y: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        position: Pos { x, y },
        pixel_position: None,
        modifiers: KeyModifiers::NONE,
    })
}

/// Drives `Mouse` through `events` (one per tick, `None` meaning "tick with no input"),
/// returning each frame's rendered text.
fn drive(events: &[Option<Event>]) -> String {
    let backend = Headless::new(50, 25);
    let mut term = Terminal::new(backend);
    let mut state = Mouse::init(&mut term);

    let mut views = Vec::new();
    for (i, event) in events.iter().enumerate() {
        if let Some(event) = event {
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
fn headless_snapshot_motion_tracked() {
    // A move followed by a click: proves the tracked-position path, the on-screen
    // coordinate/event/click-count formatting, and that `motion_seen` latches once set.
    let events = [
        Some(mouse_event(MouseEventKind::Moved, 10, 5)),
        Some(mouse_event(MouseEventKind::Down(MouseButton::Left), 10, 5)),
        Some(mouse_event(MouseEventKind::Up(MouseButton::Left), 10, 5)),
    ];
    insta::assert_snapshot!(drive(&events));
}

#[test]
fn headless_snapshot_motion_unavailable_fallback() {
    // No events at all (headless's normal "no live input" state) for longer than
    // MOTION_GRACE_TICKS: the fallback note must appear rather than a blank frame.
    // 3 representative frames (start, mid-grace, past-grace) rather than all 125+ --
    // enough to pin the transition without an unreadable multi-hundred-line snapshot.
    let backend = Headless::new(50, 25);
    let mut term = Terminal::new(backend);
    let mut state = Mouse::init(&mut term);

    let mut views = Vec::new();
    for i in 1..=130u64 {
        let frame = Frame {
            delta: HEADLESS_FRAME_DELTA,
            frame: i,
        };
        if !state.tick(&mut term, &frame) {
            break;
        }
        if matches!(i, 1 | 60 | 130) {
            views.push(format!("-- tick {i} --\n{}", term.backend().format_view()));
        }
    }
    insta::assert_snapshot!(views.join("\n"));
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<Mouse>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    // The crossterm binary's event loop is a tight spin (no fixed frame rate, no
    // blocking read -- see `run_blocking`), so it blows past `MOTION_GRACE_TICKS`
    // in well under a second with no synthetic clock to pin an exact frame. Waiting
    // on the fallback note itself (rather than the always-present header text) is
    // the only ready_marker that captures a stable, non-racy frame: the "waiting for
    // mouse motion..." frame is real but real-time-transient by design, and pinning
    // it here would make this test flake on machine speed, not catch a regression.
    // "unavailable" (not the full phrase "motion unavailable"): `capture_pty`'s ready_marker
    // is matched against the raw PTY byte stream, and this frame is a cell-diff redraw (not
    // the first, full-draw frame), so an unchanged run between two draws can split unrelated
    // words across separate cursor-positioned writes -- "motion" and " unavailable " land in
    // different `CUP` runs even though they render adjacently on screen. A single word never
    // splits internally, so it's a marker that is actually contiguous in the raw stream.
    let bin = support::build_crossterm_example("04_mouse");
    let raw = support::capture_pty(&bin, b"", 25, 50, "unavailable");
    let svg = support::svg_snapshot(&raw, 25, 50);
    assert!(
        svg.contains("motion unavailable on this backend"),
        "SVG output missing expected fallback text"
    );
    support::write_snapshot_file("04_mouse.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}
