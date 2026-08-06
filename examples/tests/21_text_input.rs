//! Snapshot tests for the `21_text_input` example.
//!
//! Includes the example's own source (rather than reimplementing its logic) via `#[path]`, so
//! these tests exercise exactly what `cargo run --example 21_text_input` runs.
//!
//! Per `examples/AGENTS.md`, an input-driven example's headless proof drives synthetic events
//! through `Headless::push_event` rather than snapshotting an idle/legend frame -- see
//! `headless_snapshot_types_navigates_backspaces_and_cycles_style` below. That snapshot alone
//! can't tell a real terminal cursor apart from the field's own cell-drawn caret, though (both
//! land on the same cell, and `Headless::format_view` only ever sees glyphs): the two tests below
//! it read `Headless`'s `cursor_visible`/`cursor_position`/`cursor_style` accessors directly,
//! which only ever change via `Cursor::set_cursor_visible`/`set_cursor_position`/
//! `set_cursor_style` -- proving this example's `Terminal` passthrough calls actually ran, with
//! the exact values the field's state implies.

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/21_text_input.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod text_input;

use retroglyph_core::app::Frame;
use retroglyph_core::backend::{CursorStyle, Headless};
use retroglyph_core::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use retroglyph_core::grid::Pos;
use retroglyph_core::terminal::Terminal;
use retroglyph_examples::{Example, HEADLESS_FRAME_DELTA};
use text_input::TextInputDemo;

/// Drives `TextInputDemo` through `events` (one batch of zero or more events per tick) against a
/// fresh 50x25 headless backend, returning each frame's rendered text (joined for
/// `insta::assert_snapshot!`) alongside the terminal itself, so a caller can also inspect
/// `term.backend()`'s recorded cursor state after the last frame.
fn drive(events: &[&[Event]]) -> (String, Terminal<Headless>) {
    let backend = Headless::new(50, 25);
    let mut term = Terminal::new(backend);
    let mut state = TextInputDemo::init(&mut term);

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
        term.present().ok();
        views.push(term.backend().format_view());
    }
    (views.join("\n--- frame ---\n"), term)
}

/// A `NONE`-modifier key press, the shape every event below needs.
const fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn headless_snapshot_types_navigates_backspaces_and_cycles_style() {
    // Frame 1: types "Hi". Frame 2: Home moves the cursor to the start (no visible change).
    // Frame 3: types "> " before "Hi", proving insert-at-cursor. Frame 4: End then Backspace
    // removes the trailing "i". Frame 5: Tab is not consumed by the field (its own style-cycling
    // effect is proven separately below, since `Headless::format_view` can't see cursor style).
    let events: [&[Event]; 5] = [
        &[key(KeyCode::Char('H')), key(KeyCode::Char('i'))],
        &[key(KeyCode::Home)],
        &[key(KeyCode::Char('>')), key(KeyCode::Char(' '))],
        &[key(KeyCode::End), key(KeyCode::Backspace)],
        &[key(KeyCode::Tab)],
    ];
    let (view, _term) = drive(&events);
    insta::assert_snapshot!(view);
}

#[test]
fn cursor_position_and_visibility_track_the_fields_caret() {
    let (_, term) = drive(&[&[]]);
    assert!(term.backend().cursor_visible());
    // Empty field, cursor at 0: the caret sits on the panel interior's first column.
    assert_eq!(term.backend().cursor_position(), Pos::new(2, 5));

    let (_, term) = drive(&[&[key(KeyCode::Char('h')), key(KeyCode::Char('i'))]]);
    // Cursor after "hi" (display column 2 of the value) lands two columns further right.
    assert_eq!(term.backend().cursor_position(), Pos::new(4, 5));

    let (_, term) = drive(&[&[
        key(KeyCode::Char('h')),
        key(KeyCode::Char('i')),
        key(KeyCode::Home),
    ]]);
    // Home moves the cursor back to the start without changing the value.
    assert_eq!(term.backend().cursor_position(), Pos::new(2, 5));
}

#[test]
fn tab_cycles_through_every_cursor_style_and_wraps() {
    let expected = [
        CursorStyle::BlinkingBlock,     // 0 tabs: the initial default.
        CursorStyle::SteadyBlock,       // 1
        CursorStyle::BlinkingUnderline, // 2
        CursorStyle::SteadyUnderline,   // 3
        CursorStyle::BlinkingBar,       // 4
        CursorStyle::SteadyBar,         // 5
        CursorStyle::BlinkingBlock,     // 6: wraps back to the first.
    ];
    for (n, &want) in expected.iter().enumerate() {
        let batches: Vec<Vec<Event>> = (0..n).map(|_| vec![key(KeyCode::Tab)]).collect();
        let batch_refs: Vec<&[Event]> = batches.iter().map(Vec::as_slice).collect();
        let (_, term) = drive(&batch_refs);
        assert_eq!(term.backend().cursor_style(), want, "after {n} tab(s)");
    }
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<TextInputDemo>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    let bin = support::build_crossterm_example("21_text_input");
    // Sends Escape rather than `capture_pty`'s usual `b""`: this example deliberately does not
    // quit on `q` (it would just be typed into the field), so without this the harness's own
    // hardcoded trailing `q` keypress (see `capture_pty_until`) is only ever typed into the
    // field and the child never exits, hanging the test. Goes through `capture_pty_until`
    // directly (rather than `capture_pty`) so `settled` can wait out crossterm's own escape-
    // sequence disambiguation window: written too close together, a raw-mode parser reads a bare
    // `\x1b` immediately followed by `q` as one Alt+q combo, not Escape then a separate `q`, and
    // this example would never see a standalone Escape key to quit on.
    let raw = support::capture_pty_until(
        &bin,
        b"\x1b",
        25,
        50,
        "Text Input",
        &[("RG_FPS", "0")],
        &|_| {
            std::thread::sleep(std::time::Duration::from_millis(100));
            true
        },
    );
    let svg = support::svg_snapshot(&raw, 25, 50);
    // Not "Type something..." whole: the caret sits on the placeholder's first column while the
    // field is empty, so `T` renders as its own inverted-style run, split from the rest of the
    // placeholder into a separate `<tspan>` -- see this example's own doc comment on the caret.
    assert!(
        svg.contains("something..."),
        "SVG output missing expected placeholder text"
    );
    support::write_snapshot_file("21_text_input.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}
