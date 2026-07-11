//! Snapshot tests for the `14_resize` example.
//!
//! Includes the example's own source (rather than reimplementing its logic) via
//! `#[path]`, so these tests exercise exactly what `cargo run --example
//! 14_resize` runs.
//!
//! Like `03_keyboard`/`04_mouse`/`11_sokoban`/`12_dungeon_scroll`, the headless snapshot drives
//! synthetic events rather than snapshotting an idle frame -- here, a sequence of
//! `Event::Resize` events, since that's the one capability this example exists to prove. The
//! `Headless` backend's own `resize` genuinely reallocates its grid (see
//! `retroglyph_core::backend::Headless`'s `Backend` impl), so this exercises the real
//! `Terminal::resize` path, not a stand-in.

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/14_resize.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod resize;

use resize::Resize;
use retroglyph_core::event::Event;
use retroglyph_core::{Frame, Headless, Terminal};
use retroglyph_examples::{Example, HEADLESS_FRAME_DELTA};

/// Drives `E` through one synthetic event per tick, returning each frame's
/// [`Headless::format_view`] text.
fn drive<E: Example>(events: &[Event]) -> Vec<String> {
    let backend = Headless::new(50, 25);
    let mut term = Terminal::new(backend);
    let mut state = E::init(&mut term);

    let mut views = Vec::new();
    for (i, &event) in events.iter().enumerate() {
        term.backend_mut().push_event(event);
        let frame = Frame {
            delta: HEADLESS_FRAME_DELTA,
            frame: i as u64,
        };
        if !state.tick(&mut term, &frame) {
            break;
        }
        views.push(term.backend().format_view());
    }
    views
}

/// Shrink to 30x10, then grow past the original 50x25 to 70x30.
fn resize_sequence() -> Vec<Event> {
    vec![Event::Resize(30, 10), Event::Resize(70, 30)]
}

#[test]
fn headless_snapshot() {
    insta::assert_snapshot!(drive::<Resize>(&resize_sequence()).join("\n--- frame ---\n"));
}

#[test]
fn shrinking_actually_reallocates_the_grid() {
    let views = drive::<Resize>(&[Event::Resize(30, 10)]);
    let view = &views[0];
    // `format_view` is exactly one line per grid row.
    let rows = view.lines().count();
    assert_eq!(
        rows, 10,
        "expected the grid to actually shrink to 10 rows:\n{view}"
    );
    // Every row should be exactly 30 cells wide now, not the original 50.
    for line in view.lines() {
        assert_eq!(line.chars().count(), 30, "expected 30-wide rows:\n{view}");
    }
}

#[test]
fn growing_back_past_the_original_size_leaves_no_stale_content() {
    // Shrink to 10x5, then grow back to 50x25 (the original size): a naive hollow-border-only
    // redraw would leave the smaller border's glyphs sitting in what is now the middle of the
    // frame (see the module doc comment) -- this example instead repaints its whole area every
    // frame, so the interior should be entirely blank again once it's back to full size.
    let views = drive::<Resize>(&[Event::Resize(10, 5), Event::Resize(50, 25)]);
    let view = &views[1];
    let interior_glyph = view.lines().nth(2).and_then(|l| l.chars().nth(2));
    assert_eq!(
        interior_glyph,
        Some('\u{b7}'), // '·', format_view's stand-in for a blank cell
        "expected no stale border left over from the smaller size:\n{view}"
    );
}

#[test]
fn growing_past_the_original_size_works_too() {
    let views = drive::<Resize>(&resize_sequence());
    let view = &views[1]; // after both resizes: now 70x30
    let rows = view.lines().count();
    assert_eq!(rows, 30, "expected the grid to grow to 30 rows:\n{view}");
    for line in view.lines() {
        assert_eq!(line.chars().count(), 70, "expected 70-wide rows:\n{view}");
    }
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<Resize>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    let bin = support::build_crossterm_example("14_resize");
    let raw = support::capture_pty(&bin, b"", 25, 50, "resize me");
    let svg = support::svg_snapshot(&raw, 25, 50);
    assert!(
        svg.contains("resize me"),
        "SVG output missing expected text"
    );
    support::write_snapshot_file("14_resize.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}
