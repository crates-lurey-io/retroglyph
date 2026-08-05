//! Snapshot tests for the `13_combat_log` example.
//!
//! Includes the example's own source (rather than reimplementing its logic) via
//! `#[path]`, so these tests exercise exactly what `cargo run --example
//! 13_combat_log` runs.
//!
//! Like `11_sokoban`/`12_dungeon_scroll`, the headless snapshot drives synthetic key events
//! rather than snapshotting an idle frame -- here, the full fixed-damage fight to its end, so
//! the snapshot actually proves `StatBar`, `Log`, `Scrollbar`, and `Modal` all render correctly
//! together, not just that the opening frame does.

#![allow(unreachable_pub)]

#[path = "support/mod.rs"]
mod support;

#[path = "../examples/13_combat_log.rs"]
#[allow(dead_code)] // `main`/the `wasm_entry!` FFI surface aren't exercised by these tests
mod combat_log;

use combat_log::CombatLog;
use retroglyph_core::app::Frame;
use retroglyph_core::backend::Headless;
use retroglyph_core::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use retroglyph_core::grid::Pos;
use retroglyph_core::terminal::Terminal;
use retroglyph_examples::{Example, HEADLESS_FRAME_DELTA};

/// A plain, unmodified key press.
const fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

/// Drives `E` through one synthetic key event per tick, returning each frame's
/// [`Headless::format_view`] text.
///
/// Not migrated to [`TestHarness`](retroglyph_core::testing::TestHarness) (retroglyph#1001):
/// `CombatLog::draw` integrates `ScrollState`'s wheel momentum against `frame.delta`, calibrated
/// against [`HEADLESS_FRAME_DELTA`]'s 100ms (see its own doc comment), not `TestHarness::step`'s
/// fixed 16ms -- `mouse_wheel_scrolls_the_log_with_momentum` below counts idle frames to prove the
/// physics settle, and that count is only meaningful at the delta it was tuned against.
fn drive<E: Example>(events: &[Event]) -> String {
    let backend = Headless::new(50, 25);
    let mut term = Terminal::new(backend);
    let mut state = E::init(&mut term);

    let mut views = Vec::new();
    for (i, event) in events.iter().enumerate() {
        term.backend_mut().push_event(event.clone());
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
    views.join("\n--- frame ---\n")
}

/// `Headless::format_view` renders spaces as middle dots (see its own doc comment), so a plain
/// substring check against normal text would never match -- normalize dots back to spaces first.
fn normalize(view: &str) -> String {
    view.replace('\u{b7}', " ")
}

/// Six attacks: the fixed 7/5 damage exchange (see the example's own `attack` doc comment)
/// brings the 40-HP goblin down exactly on the sixth strike, without a final retaliation.
fn six_attacks() -> Vec<Event> {
    std::iter::repeat_n(key(KeyCode::Char('a')), 6).collect()
}

#[test]
fn headless_snapshot() {
    insta::assert_snapshot!(drive::<CombatLog>(&six_attacks()));
}

#[test]
fn six_attacks_defeats_the_goblin_and_shows_the_win_modal() {
    let view = normalize(&drive::<CombatLog>(&six_attacks()));
    let last_frame = view.rsplit("--- frame ---").next().unwrap_or(&view);
    assert!(
        last_frame.contains("You win!"),
        "expected the win modal:\n{last_frame}"
    );
    assert!(
        last_frame.contains("You: 5/30"),
        "expected the player to have survived at 5 hp:\n{last_frame}"
    );
    assert!(
        last_frame.contains("Goblin: 0/40"),
        "expected the goblin defeated:\n{last_frame}"
    );
}

#[test]
fn scrolling_the_log_shows_earlier_messages() {
    // Enough attacks to overflow the log's visible height, then one Up press: the view should
    // no longer show the newest message ("The goblin falls...") at the bottom.
    let mut events = six_attacks();
    events.push(key(KeyCode::Up));
    let view = normalize(&drive::<CombatLog>(&events));
    let last_frame = view.rsplit("--- frame ---").next().unwrap_or(&view);
    assert!(
        !last_frame.contains("The goblin falls"),
        "expected scrolling back to move the newest message out of view:\n{last_frame}"
    );
}

/// A mouse wheel "notch" over `pos`, in the direction that scrolls the log back into history
/// (see `Pointer::handle_event`'s `dy` sign convention: negative `dy` accumulates a positive
/// `Response::scroll_delta`, which `ScrollState::apply` feeds into `scroll_by_wheel` as-is).
const fn wheel_scroll(x: u16, y: u16) -> Event {
    Event::Mouse(MouseEvent::new(
        MouseEventKind::Scroll { dx: 0.0, dy: -1.0 },
        Pos { x, y },
        KeyModifiers::NONE,
    ))
}

/// A harmless pointer-move at `pos`: advances a frame without adding any new wheel input, so a
/// run of these isolates however much of `ScrollState`'s momentum is still decaying on its own.
const fn idle_at(x: u16, y: u16) -> Event {
    Event::Mouse(MouseEvent::new(
        MouseEventKind::Moved,
        Pos { x, y },
        KeyModifiers::NONE,
    ))
}

/// Proves the wheel path end to end: `Interaction` resolves the wheel input into a `Response`,
/// `ScrollState::apply` turns it into velocity, and `ScrollState::tick` (called every frame,
/// unconditionally, from `CombatLog::draw`) keeps integrating that velocity forward even once the
/// wheel itself goes quiet -- the momentum `scrolling_the_log_shows_earlier_messages` doesn't
/// exercise, since a key press moves the log with `set_offset` (velocity always zero) instead.
///
/// Two attacks, not six: enough log lines to scroll, but short of the fight-ending sixth attack,
/// so the win `Modal` never covers the log -- important here since several of the log's own
/// messages repeat verbatim ("You strike the goblin for 7."), so a narrow sliver of visible log
/// column left uncovered by a modal can't reliably distinguish one scroll position from another.
#[test]
fn mouse_wheel_scrolls_the_log_with_momentum() {
    // A point inside the log's rect (`Rect::new(1, 4, 47, 20)` in the example itself).
    const LOG_X: u16 = 5;
    const LOG_Y: u16 = 10;

    let mut events: Vec<Event> = std::iter::repeat_n(key(KeyCode::Char('a')), 2).collect();
    events.push(wheel_scroll(LOG_X, LOG_Y));
    events.push(wheel_scroll(LOG_X, LOG_Y));
    for _ in 0..30 {
        events.push(idle_at(LOG_X, LOG_Y));
    }

    let view = drive::<CombatLog>(&events);
    let frames: Vec<String> = view
        .split("--- frame ---")
        .map(|f| normalize(f.trim()))
        .collect();
    // Two attack frames (offset still 0, nothing scrolled yet), then two wheel-notch frames,
    // then thirty idle frames -- comfortably past how long the physics take to settle (a few
    // hundred ms; see ScrollPhysics::DEFAULT).
    let baseline = &frames[1];
    let after_wheel = &frames[4..];

    // The wheel scrolled the log at all: some frame after it differs from the pre-scroll
    // baseline. A whole-frame comparison, not a specific-message substring check, since several
    // of the log's own messages repeat verbatim ("You strike the goblin for 7."), so a substring
    // present/absent check can't reliably tell one scroll position from another here.
    assert!(
        after_wheel.iter().any(|f| f != baseline),
        "expected the wheel to change the log's view at some point:\n{view}"
    );

    // Momentum, not an instant jump to a resting position: some consecutive pair of frames after
    // the wheel went quiet still differs (offset kept advancing under its own velocity, with no
    // new wheel input to explain the change).
    assert!(
        after_wheel.windows(2).any(|pair| pair[0] != pair[1]),
        "expected the log to keep scrolling for at least one frame after the last wheel input:\n{view}"
    );

    // ...and it settles rather than scrolling forever: the last few idle frames agree once
    // friction/the rubber-band spring has fully decayed velocity to zero.
    let tail = &frames[frames.len() - 3..];
    assert!(
        tail.windows(2).all(|pair| pair[0] == pair[1]),
        "expected the log to have settled by the last few idle frames:\n{view}"
    );
}

#[cfg(all(feature = "software", not(target_arch = "wasm32")))]
#[test]
fn png_snapshot() {
    let png = support::png_snapshot::<CombatLog>(50, 25, 2);
    insta::assert_binary_snapshot!(".png", png);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn svg_snapshot() {
    let bin = support::build_crossterm_example("13_combat_log");
    let raw = support::capture_pty(&bin, b"", 25, 50, "a: attack");
    let svg = support::svg_snapshot(&raw, 25, 50);
    assert!(svg.contains("attack"), "SVG output missing expected text");
    support::write_snapshot_file("13_combat_log.svg", svg.as_bytes());
    insta::assert_snapshot!(svg);
}
