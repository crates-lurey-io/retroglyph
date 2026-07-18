//! Snapshot tests for the `17_theme_switch` example.
//!
//! Includes the example's own source (rather than reimplementing its logic) via
//! `#[path]`, so these tests exercise exactly what `cargo run --example
//! 17_theme_switch` runs.
//!
//! The headless text snapshots can't see color (see
//! [`Headless::format_view`]'s doc comment), so what they prove is that `t`
//! actually flips [`ThemeSwitch`]'s state -- visible here as the panel title and
//! button label text changing between "Dark"/"Light" -- not that the resulting colors differ.
//! The PNG/SVG snapshots (like every other example's) capture the default startup state, which
//! proves [`Theme::DARK`]'s palette reaches the pixel-level and terminal-I/O render paths; the
//! headless snapshots above are what proves the toggle itself works.
//!
//! [`Headless::format_view`]: retroglyph_core::Headless::format_view
//! [`Theme::DARK`]: retroglyph_widgets::Theme::DARK

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/17_theme_switch.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod theme_switch;

use retroglyph_core::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use retroglyph_core::{Frame, Headless, Terminal};
use retroglyph_examples::{Example, HEADLESS_FRAME_DELTA};
use theme_switch::ThemeSwitch;

/// A plain, unmodified key press.
const fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

/// Drives `ThemeSwitch` through `events` (one batch of zero or more events per tick), returning
/// each frame's rendered text.
fn drive(events: &[&[Event]]) -> String {
    let backend = Headless::new(50, 25);
    let mut term = Terminal::new(backend);
    let mut state = ThemeSwitch::init(&mut term);

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
fn headless_snapshot_default_dark() {
    // No input: the default state (`Theme::DARK`, "Theme: Dark" title, "Switch to Light" button).
    insta::assert_snapshot!(drive(&[&[]]));
}

#[test]
fn headless_snapshot_toggle_and_navigate() {
    // Frame 1: `t` flips to `Theme::LIGHT` -- the title/button text below flips to "Theme:
    // Light"/"Switch to Dark", proving the toggle actually changed state (see the module doc
    // comment for why this text flip, not a color diff, is what a headless snapshot can prove).
    // Frames 2-3 (Right, then Down) exercise the tab-select/list-select input paths the same way
    // -- like the selected tab/item highlight itself, they render as a color-only change this
    // text-only snapshot can't see, so what these two frames actually prove is that neither input
    // path panics or otherwise disturbs the rendered layout, not that the highlight visibly moved.
    insta::assert_snapshot!(drive(&[
        &[key(KeyCode::Char('t'))],
        &[key(KeyCode::Right)],
        &[key(KeyCode::Down)],
    ]));
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<ThemeSwitch>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    let bin = support::build_crossterm_example("17_theme_switch");
    let raw = support::capture_pty(&bin, b"", 25, 50, "toggles theme");
    let svg = support::svg_snapshot(&raw, 25, 50);
    assert!(
        svg.contains("Alpha"),
        "SVG output missing expected list content"
    );
    support::write_snapshot_file("17_theme_switch.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}
