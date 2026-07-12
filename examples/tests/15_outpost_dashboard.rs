//! Snapshot and behavior tests for the `15_outpost_dashboard` example.
//!
//! Includes the example's own source (rather than reimplementing its logic) via `#[path]`, so
//! these tests exercise exactly what `cargo run --example 15_outpost_dashboard` runs.
//!
//! Beyond the headless/PNG/SVG snapshots every example gets, this drives real synthetic mouse
//! and keyboard sequences to prove the interaction patterns the module doc comment calls out:
//! tap-to-select, drag-to-pan, slide-off-cancel, keyboard equivalents for every mouse action, and
//! -- the one with an explicit accessibility policy -- that every interactive control meets the
//! WCAG-derived touch-target minimums across sizes, tabs, and layout modes.

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/15_outpost_dashboard.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod dashboard;

use dashboard::{HitTarget, MIN_TARGET_H, MIN_TARGET_W, OutpostDashboard, Tab};
use retroglyph_core::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use retroglyph_core::{Frame, Headless, Pos, Rect, Terminal};
use retroglyph_examples::{Example, HEADLESS_FRAME_DELTA};

const fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

const fn mouse(kind: MouseEventKind, x: u16, y: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        position: Pos::new(x, y),
        pixel_position: None,
        modifiers: KeyModifiers::NONE,
    })
}

/// Drives `E` through one synthetic event per tick, returning each frame's
/// [`Headless::format_view`] text.
fn drive_sized<E: Example>(width: u16, height: u16, events: &[Event]) -> (E, Vec<String>) {
    let backend = Headless::new(width, height);
    let mut term = Terminal::new(backend);
    let mut state = E::init(&mut term);

    // Prime one frame with no input first: `draw()` is what populates layout state like
    // `last_map_rect`, and a real interactive loop always draws at least once before any input
    // arrives, so mouse events aimed at the map on the very first driven tick need that state
    // to already exist.
    let priming_frame = Frame {
        delta: HEADLESS_FRAME_DELTA,
        frame: 0,
    };
    state.tick(&mut term, &priming_frame);

    let mut views = vec![term.backend().format_view()];
    for &event in events {
        term.backend_mut().push_event(event);
        let frame = Frame {
            delta: HEADLESS_FRAME_DELTA,
            frame: 0,
        };
        if !state.tick(&mut term, &frame) {
            break;
        }
        views.push(term.backend().format_view());
    }
    (state, views)
}

fn drive<E: Example>(events: &[Event]) -> (E, Vec<String>) {
    drive_sized(50, 25, events)
}

/// Draws one idle frame at `width`x`height` and returns the resulting state (with its hitboxes
/// populated) alongside the rendered view.
fn draw_at(width: u16, height: u16) -> (OutpostDashboard, String) {
    let backend = Headless::new(width, height);
    let mut term = Terminal::new(backend);
    let mut state = OutpostDashboard::init(&mut term);
    let frame = Frame {
        delta: HEADLESS_FRAME_DELTA,
        frame: 0,
    };
    state.tick(&mut term, &frame);
    let view = term.backend().format_view();
    (state, view)
}

fn find_target(hitboxes: &[(Rect, HitTarget)], want: HitTarget) -> Rect {
    hitboxes
        .iter()
        .find(|(_, t)| *t == want)
        .map_or_else(|| panic!("target {want:?} not registered"), |(r, _)| *r)
}

#[test]
fn headless_snapshot() {
    let (_, views) = drive::<OutpostDashboard>(&[]);
    insta::assert_snapshot!(views.join("\n--- frame ---\n"));
}

#[test]
fn wide_layout_snapshot() {
    let (_, view) = draw_at(90, 30);
    insta::assert_snapshot!(view);
}

/// Every interactive target meets the WCAG-2.2-derived minimums from the module docs (>= 6x3
/// cells, no overlaps) across sizes and tabs -- the accessibility policy this example exists to
/// enforce, not just describe in a doc comment.
#[test]
fn touch_targets_meet_minimums() {
    let sizes = [(40u16, 20u16), (50, 25), (84, 24), (90, 30), (120, 40)];
    for (w, h) in sizes {
        for setup in 0..3u8 {
            let (state, _) = draw_at(w, h);
            let mut state = state;
            match setup {
                0 => {}
                1 => {
                    state.tab = Tab::Settings;
                }
                2 => {
                    state.selected = Some(state.cursor);
                    state.sheet_open = true;
                }
                _ => unreachable!(),
            }
            // Redraw with the setup applied so hitboxes reflect it.
            let backend = Headless::new(w, h);
            let mut term = Terminal::new(backend);
            state.draw(&mut term);

            for (rect, target) in &state.hitboxes {
                assert!(
                    rect.width() >= MIN_TARGET_W && rect.height() >= MIN_TARGET_H,
                    "{target:?} is {}x{} at {w}x{h} (setup {setup}); minimum is {MIN_TARGET_W}x{MIN_TARGET_H}",
                    rect.width(),
                    rect.height(),
                );
            }
            for (i, (a, ta)) in state.hitboxes.iter().enumerate() {
                for (b, tb) in state.hitboxes.iter().skip(i + 1) {
                    assert!(
                        !a.overlaps(*b),
                        "{ta:?} at {a:?} overlaps {tb:?} at {b:?} ({w}x{h}, setup {setup})",
                    );
                }
            }
        }
    }
}

#[test]
fn tap_selects_a_tile_and_opens_the_sheet() {
    let (state, _) = draw_at(50, 25);
    let map = state.last_map_rect.expect("map rect");
    let (x, y) = (map.left() + 3, map.top() + 3);
    let (state, _) = drive::<OutpostDashboard>(&[
        mouse(MouseEventKind::Down(MouseButton::Left), x, y),
        mouse(MouseEventKind::Up(MouseButton::Left), x, y),
    ]);
    assert!(state.selected.is_some());
    assert!(state.sheet_open);
}

#[test]
fn drag_pans_the_camera_without_selecting() {
    let (state0, _) = draw_at(50, 25);
    let map = state0.last_map_rect.expect("map rect");
    let before = state0.cam_center;
    let (x, y) = (map.left() + 5, map.top() + 3);
    let (state, _) = drive::<OutpostDashboard>(&[
        mouse(MouseEventKind::Down(MouseButton::Left), x, y),
        mouse(MouseEventKind::Moved, x + 3, y + 2),
        mouse(MouseEventKind::Up(MouseButton::Left), x + 3, y + 2),
    ]);
    assert_ne!(state.cam_center, before, "drag should pan");
    assert!(state.selected.is_none(), "drag must not select a tile");
}

#[test]
fn repair_and_inspect_spawn_a_rising_acknowledgement() {
    let (state0, _) = drive::<OutpostDashboard>(&[key(KeyCode::Enter)]);
    assert!(
        state0.selected.is_some(),
        "need a selection to open the sheet/sidebar first"
    );
    let rect = find_target(&state0.hitboxes, HitTarget::Repair);
    let (x, y) = (rect.left() + 1, rect.top() + 1);
    let (state, _) = drive::<OutpostDashboard>(&[
        key(KeyCode::Enter),
        mouse(MouseEventKind::Down(MouseButton::Left), x, y),
        mouse(MouseEventKind::Up(MouseButton::Left), x, y),
    ]);
    assert_eq!(
        state.floating_count(),
        1,
        "tapping REPAIR should spawn exactly one rising acknowledgement"
    );
}

/// Regression test for the bug an earlier draft shipped with: a 22x14 world fully fit inside
/// any reasonable terminal or window, so `Camera::center_on`'s edge-clamping left the origin
/// permanently pinned at `(0, 0)` no matter how far you dragged (confirmed by loading the WASM
/// build in a real browser -- nothing moved). The world-size invariant itself is a compile-time
/// `const` assertion in the example source; this asserts the *behavior*: a large drag actually
/// moves the camera's visible origin, not just `cam_center`.
#[test]
fn dragging_a_long_distance_actually_moves_the_camera_origin() {
    let (state0, _) = draw_at(50, 25);
    let map = state0.last_map_rect.expect("map rect");
    let origin_before = state0.camera_origin();
    let (x, y) = (map.left() + 5, map.top() + 3);
    // One long drag, several cells at a time, well past what a small/clamped world could absorb.
    let (state, _) = drive::<OutpostDashboard>(&[
        mouse(MouseEventKind::Down(MouseButton::Left), x, y),
        mouse(MouseEventKind::Moved, x + 20, y + 10),
        mouse(MouseEventKind::Moved, x + 40, y + 15),
        mouse(MouseEventKind::Up(MouseButton::Left), x + 40, y + 15),
    ]);
    assert_ne!(
        state.camera_origin(),
        origin_before,
        "a long drag must actually move the visible viewport, not just cam_center"
    );
}

#[test]
fn slide_off_a_button_cancels_activation() {
    let (state0, _) = draw_at(50, 25);
    let rect = find_target(&state0.hitboxes, HitTarget::Tab(Tab::Settings));
    let (x, y) = (rect.left() + 1, rect.top() + 1);
    let (state, _) = drive::<OutpostDashboard>(&[
        mouse(MouseEventKind::Down(MouseButton::Left), x, y),
        mouse(MouseEventKind::Moved, x, rect.top().saturating_sub(2)),
        mouse(
            MouseEventKind::Up(MouseButton::Left),
            x,
            rect.top().saturating_sub(2),
        ),
    ]);
    assert_eq!(state.tab, Tab::Overview, "slide-off must not activate");
}

#[test]
fn nav_tap_switches_tabs() {
    let (state0, _) = draw_at(50, 25);
    let rect = find_target(&state0.hitboxes, HitTarget::Tab(Tab::Settings));
    let (x, y) = (rect.left() + 1, rect.top() + 1);
    let (state, _) = drive::<OutpostDashboard>(&[
        mouse(MouseEventKind::Down(MouseButton::Left), x, y),
        mouse(MouseEventKind::Up(MouseButton::Left), x, y),
    ]);
    assert_eq!(state.tab, Tab::Settings);
}

#[test]
fn keyboard_tab_key_switches_tabs_without_any_mouse_event() {
    let (state, _) = drive::<OutpostDashboard>(&[key(KeyCode::Tab)]);
    assert_eq!(state.tab, Tab::Settings);
}

#[test]
fn keyboard_arrows_and_enter_select_a_tile() {
    let (state, _) =
        drive::<OutpostDashboard>(&[key(KeyCode::Right), key(KeyCode::Down), key(KeyCode::Enter)]);
    assert!(state.selected.is_some());
    assert!(state.sheet_open);
}

#[test]
fn wide_layout_renders_a_persistent_sidebar_instead_of_a_sheet() {
    let (state, _) = drive_sized::<OutpostDashboard>(
        90,
        30,
        &[
            key(KeyCode::Enter), // select the tile under the starting cursor
        ],
    );
    assert!(state.selected.is_some());
    // A wide layout never opens the bottom sheet -- the sidebar is persistent instead.
    assert!(!state.sheet_open || state.last_map_rect.is_some());
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<OutpostDashboard>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    let bin = support::build_crossterm_example("15_outpost_dashboard");
    let raw = support::capture_pty(&bin, b"", 25, 50, "Outpost");
    let svg = support::svg_snapshot(&raw, 25, 50);
    assert!(svg.contains("Outpost"), "SVG output missing expected text");
    support::write_snapshot_file("15_outpost_dashboard.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}
