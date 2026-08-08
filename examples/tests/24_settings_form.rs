//! Snapshot tests for the `24_settings_form` example.
//!
//! Includes the example's own source (rather than reimplementing its logic) via `#[path]`, so
//! these tests exercise exactly what `cargo run --example 24_settings_form` runs.
//!
//! Per `examples/AGENTS.md`, an input-driven example's headless proof drives synthetic events
//! through the backend rather than snapshotting an idle/legend frame; `headless_snapshot_focus_routing`
//! is that proof for this example, and is the point of `retroglyph#1284`: it drives Tab, a
//! direct click on a field that overrides wherever Tab had gotten to, keyboard list navigation,
//! typed input landing in whichever field is actually focused, and Enter saving regardless of
//! which widget holds focus, in one continuous session.

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/24_settings_form.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod settings_form;

use retroglyph::TestHarness;
use retroglyph::app::Flow;
use retroglyph::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use retroglyph::grid::Pos;
use retroglyph_examples::Example;
use settings_form::SettingsForm;
use support::TestApp;

/// A plain, unmodified key press.
const fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

const fn mouse_event(kind: MouseEventKind, x: u16, y: u16) -> Event {
    Event::Mouse(MouseEvent::new(kind, Pos { x, y }, KeyModifiers::NONE))
}

/// Drives `SettingsForm` through `events` (one batch of zero or more events per tick), returning
/// each frame's rendered text.
fn drive(events: &[&[Event]]) -> String {
    let mut harness = TestHarness::new(50, 25);
    let mut app = TestApp(SettingsForm::init(harness.term_mut()));

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

#[test]
fn headless_snapshot_focus_routing() {
    // Frame 1: registers all five widgets (Items, ItemsScroll, Name, Notes, Save, in draw
    // order) for frame 2's Tab/click resolution -- see `Interaction`'s own frame-lifecycle docs.
    // Frame 2: Tab focuses `Items`, the first registered id.
    // Frame 3: Down, while `Items` is focused, moves the inventory selection from "Potion" to
    // "Sword" -- proves keyboard input reaches whichever widget the focus ring says is focused.
    // Frame 4: a mouse down+up lands inside the `Name` field's rect. Frame 5 (empty, since a
    // click resolves one frame later than the press/release that produced it -- the same
    // two-frame shape `09_widgets_dashboard`'s button-click test relies on) is where the click
    // actually lands: `Name` becomes focused even though `Items` still held it a moment ago --
    // proves click-to-focus, wired by hand via `Ui::region` since `TextInput` has no
    // `InteractiveWidget` impl (retroglyph#1284).
    // Frame 6: types "Ada" -- lands in `Name`, not `Items`, proving per-field event routing.
    // Frame 7: Tab moves focus from `Name` to `Notes`, the next id in draw order.
    // Frame 8: types "Hero" -- lands in `Notes`.
    // Frame 9: Enter saves, regardless of `Notes` (not `Save`) holding focus.
    // Frame 10 (empty): renders the "Saved: ..." status line frame 9's save produced.
    let type_ada: [Event; 3] = [
        key(KeyCode::Char('A')),
        key(KeyCode::Char('d')),
        key(KeyCode::Char('a')),
    ];
    let type_hero: [Event; 4] = [
        key(KeyCode::Char('H')),
        key(KeyCode::Char('e')),
        key(KeyCode::Char('r')),
        key(KeyCode::Char('o')),
    ];
    let click_name: [Event; 2] = [
        mouse_event(MouseEventKind::Down(MouseButton::Left), 30, 3),
        mouse_event(MouseEventKind::Up(MouseButton::Left), 30, 3),
    ];
    let events: [&[Event]; 10] = [
        &[],
        &[key(KeyCode::Tab)],
        &[key(KeyCode::Down)],
        &click_name,
        &[],
        &type_ada,
        &[key(KeyCode::Tab)],
        &type_hero,
        &[key(KeyCode::Enter)],
        &[],
    ];
    insta::assert_snapshot!(drive(&events));
}

#[test]
fn escape_quits_regardless_of_focus() {
    let mut harness = TestHarness::new(50, 25);
    let mut app = TestApp(SettingsForm::init(harness.term_mut()));
    harness.push_event(key(KeyCode::Tab));
    assert_eq!(harness.step(&mut app), Flow::Continue);

    harness.push_event(key(KeyCode::Escape));
    assert_eq!(harness.step(&mut app), Flow::Exit);
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<SettingsForm>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    let bin = support::build_crossterm_example("24_settings_form");
    // Sends Escape rather than `capture_pty`'s usual trailing `q`: this example deliberately does
    // not quit on `q` (a focused text field would just type it) -- same reasoning as
    // `21_text_input`'s own `svg_snapshot` test, which this mirrors.
    let raw = support::capture_pty_until(
        &bin,
        b"\x1b",
        25,
        50,
        "Inventory",
        &[("RG_FPS", "0")],
        &|_| {
            std::thread::sleep(std::time::Duration::from_millis(100));
            true
        },
    );
    let svg = support::svg_snapshot(&raw, 25, 50);
    assert!(
        svg.contains("Inventory"),
        "SVG output missing expected inventory panel text"
    );
    support::write_snapshot_file("24_settings_form.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}
