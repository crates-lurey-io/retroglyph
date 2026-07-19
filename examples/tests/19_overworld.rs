//! Snapshot and behavior tests for the `19_overworld` example.
//!
//! Includes the example's own source (rather than reimplementing its logic) via `#[path]`, so
//! these tests exercise exactly what `cargo run --example 19_overworld` runs.
//!
//! Beyond the headless/PNG/SVG snapshots every example gets, this drives real synthetic mouse
//! sequences against the sidebar minimap and the main map: click-to-jump, drag-to-keep-jumping,
//! release-to-stop, a plain click on the main map (which must not jump like the minimap does),
//! and a drag on the main map (which pans instead) -- the interaction pattern
//! `Overworld::jump_to_minimap`'s doc comment describes.

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/19_overworld.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod overworld;

use overworld::{Overworld, WORLD_H, WORLD_W};
use retroglyph_core::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use retroglyph_core::{Frame, Headless, Terminal};
use retroglyph_examples::{Example, HEADLESS_FRAME_DELTA};

const fn mouse(kind: MouseEventKind, x: u16, y: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        position: retroglyph_core::Pos { x, y },
        pixel_position: None,
        modifiers: KeyModifiers::NONE,
    })
}

/// Draws one idle frame at `width`x`height` and returns the resulting state, with
/// `last_map_rect`/`last_minimap_rect` populated for the caller to pick coordinates against.
fn draw_at(width: u16, height: u16) -> (Overworld, String) {
    let backend = Headless::new(width, height);
    let mut term = Terminal::new(backend);
    let mut state = Overworld::init(&mut term);
    state.draw(&mut term);
    let view = term.backend().format_view();
    (state, view)
}

/// Drives `Overworld` through one synthetic event per tick at `width`x`height`, returning the
/// resulting state and each frame's rendered text.
///
/// Primes one frame with no input first: `draw` is what populates `last_map_rect`/
/// `last_minimap_rect`, and a real interactive loop always draws at least once before any input
/// arrives, so mouse events aimed at the sidebar on the very first driven tick need that state
/// to already exist.
fn drive_sized(width: u16, height: u16, events: &[Event]) -> (Overworld, Vec<String>) {
    let backend = Headless::new(width, height);
    let mut term = Terminal::new(backend);
    let mut state = Overworld::init(&mut term);

    let priming = Frame {
        delta: HEADLESS_FRAME_DELTA,
        frame: 0,
    };
    state.tick(&mut term, &priming);

    let mut views = vec![term.backend().format_view()];
    for event in events {
        term.backend_mut().push_event(event.clone());
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

/// [`drive_sized`] at 100x32 -- wide/tall enough for the sidebar and its minimap to appear (see
/// `BP_SIDEBAR`/`BP_TALL`/`MINIMAP_H` in the example itself).
fn drive(events: &[Event]) -> (Overworld, Vec<String>) {
    drive_sized(100, 32, events)
}

#[test]
fn headless_snapshot() {
    insta::assert_snapshot!(support::headless_snapshot::<Overworld>(1));
}

#[test]
fn wide_layout_snapshot() {
    // At or above BP_SIDEBAR/BP_TALL: the info sidebar, minimap, and legend all render.
    let (_, view) = draw_at(100, 32);
    insta::assert_snapshot!(view);
}

#[test]
fn narrow_layout_snapshot() {
    // Below BP_SIDEBAR: chrome collapses to a single status line, no sidebar/minimap.
    let (_, view) = draw_at(50, 20);
    insta::assert_snapshot!(view);
}

#[test]
fn clicking_the_minimap_top_left_jumps_near_the_world_origin() {
    let (state0, _) = draw_at(100, 32);
    let rect = state0
        .last_minimap_rect
        .expect("minimap should be visible at this size");
    let (x, y) = (rect.left(), rect.top());
    let (state, _) = drive(&[
        mouse(MouseEventKind::Down(MouseButton::Left), x, y),
        mouse(MouseEventKind::Up(MouseButton::Left), x, y),
    ]);
    assert!(
        state.cam_center.x < 20 && state.cam_center.y < 20,
        "expected a jump near the world origin, got {:?}",
        state.cam_center
    );
}

#[test]
fn clicking_the_minimap_bottom_right_jumps_near_the_world_end() {
    let (state0, _) = draw_at(100, 32);
    let rect = state0
        .last_minimap_rect
        .expect("minimap should be visible at this size");
    let (x, y) = (rect.right() - 1, rect.bottom() - 1);
    let (state, _) = drive(&[
        mouse(MouseEventKind::Down(MouseButton::Left), x, y),
        mouse(MouseEventKind::Up(MouseButton::Left), x, y),
    ]);
    assert!(
        state.cam_center.x > WORLD_W - 40 && state.cam_center.y > WORLD_H - 40,
        "expected a jump near the world's far corner, got {:?}",
        state.cam_center
    );
}

#[test]
fn dragging_across_the_minimap_keeps_jumping_as_it_moves() {
    let (state0, _) = draw_at(100, 32);
    let rect = state0
        .last_minimap_rect
        .expect("minimap should be visible at this size");
    let (after_down, _) = drive(&[mouse(
        MouseEventKind::Down(MouseButton::Left),
        rect.left(),
        rect.top(),
    )]);
    let first = after_down.cam_center;
    let (state, _) = drive(&[
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            rect.left(),
            rect.top(),
        ),
        mouse(MouseEventKind::Moved, rect.right() - 1, rect.bottom() - 1),
    ]);
    assert_ne!(
        state.cam_center, first,
        "dragging across the minimap should keep moving the camera"
    );
}

#[test]
fn releasing_and_moving_again_stops_jumping() {
    let (state0, _) = draw_at(100, 32);
    let rect = state0
        .last_minimap_rect
        .expect("minimap should be visible at this size");
    let (state, _) = drive(&[
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            rect.left(),
            rect.top(),
        ),
        mouse(
            MouseEventKind::Up(MouseButton::Left),
            rect.left(),
            rect.top(),
        ),
    ]);
    let after_release = state.cam_center;

    let (state, _) = drive(&[
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            rect.left(),
            rect.top(),
        ),
        mouse(
            MouseEventKind::Up(MouseButton::Left),
            rect.left(),
            rect.top(),
        ),
        mouse(MouseEventKind::Moved, rect.right() - 1, rect.bottom() - 1),
    ]);
    assert_eq!(
        state.cam_center, after_release,
        "movement after release shouldn't still be dragging"
    );
}

#[test]
fn clicking_the_main_map_does_not_jump_the_camera() {
    let (state0, _) = draw_at(100, 32);
    let before = state0.cam_center;
    let (state, _) = drive(&[mouse(MouseEventKind::Down(MouseButton::Left), 0, 0)]);
    assert_eq!(
        before, state.cam_center,
        "a plain click-and-hold on the main map shouldn't jump the camera"
    );
}

#[test]
fn dragging_the_main_map_pans_the_camera() {
    let (state0, _) = draw_at(100, 32);
    let map = state0.last_map_rect.expect("map rect");
    let before = state0.cam_center;
    let (x, y) = (map.left() + 5, map.top() + 3);
    let (state, _) = drive(&[
        mouse(MouseEventKind::Down(MouseButton::Left), x, y),
        mouse(MouseEventKind::Moved, x + 3, y + 2),
    ]);
    assert_ne!(
        state.cam_center, before,
        "dragging the main map should pan the camera"
    );
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<Overworld>(100, 32, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    let bin = support::build_crossterm_example("19_overworld");
    let raw = support::capture_pty(&bin, b"", 32, 100, "OVERWORLD");
    let svg = support::svg_snapshot(&raw, 32, 100);

    // Not `insta::assert_snapshot!(svg)`, unlike every other example's `svg_snapshot`: `06_layers`
    // and `08_animation` solve the crossterm binary's unthrottled-spin-loop timing (see their own
    // tests' comments) by parking their animation at a settled end state before this capture
    // fires, giving a reproducible frame to pin byte-for-byte. Overworld's water swell/foam and
    // biome twinkle/gleam/ember/pulse flourishes are deliberately ambient -- they animate for as
    // long as the app runs, with no settled state to park at -- so the exact RGB this frame
    // lands on is real-wall-clock dependent and provably flaky (confirmed by hand: this snapshot
    // fails on a plain rerun with no code change). Assert on the chrome that layout, not the
    // clock, controls instead: the sidebar panel, legend, and status hint text.
    for expected in ["OVERWORLD", "Legend", "arrows/drag pan, R rerolls"] {
        assert!(
            svg.contains(expected),
            "SVG output missing expected sidebar text {expected:?}"
        );
    }
    support::write_snapshot_file("19_overworld.svg", svg.as_bytes());
}
