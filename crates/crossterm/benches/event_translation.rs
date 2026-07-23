//! Benchmarks `from_crossterm_event`, the per-event mapping `Input::poll_event` runs on every
//! `crossterm::event::read()` result before handing an event back to
//! [`retroglyph_core::Terminal::drain_events`].
//!
//! retroglyph#285 flags this as cheap per-call but sitting on the input hot path -- an uncapped
//! game loop calling `drain_events()` every iteration runs this once per buffered event. This
//! benchmark measures translation throughput over a representative mix of the event kinds a real
//! session produces: key presses (with and without the kitty-protocol Shift+Tab special case),
//! key repeat/release, mouse moves/clicks/scrolls, resizes, paste, and focus changes, plus the
//! single unmappable-event case (`MouseEventKind::Drag` maps via `Moved`, so genuinely unmappable
//! crossterm events are rare; `KeyCode::Media(_)` etc. are used here as a stand-in) that exercises
//! the `None` branch `poll_event`'s retry loop depends on.
//!
//! `from_crossterm_event` is a private implementation detail of `retroglyph-crossterm` and is
//! exposed here only via a `#[doc(hidden)] pub` escape hatch (see its doc comment in `lib.rs`) --
//! this bench is not evidence of a supported public API.

// `criterion_group!`/`criterion_main!` below expand to an undocumented `pub fn benches(..)`; this
// bench binary isn't a published API surface for `missing_docs` to usefully gate.
#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use crossterm::event::{
    Event as CtEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use retroglyph_crossterm::from_crossterm_event;
use std::hint::black_box;

/// Builds a deterministic, representative mix of crossterm events: plain key presses, a
/// kitty-protocol Shift+Tab (the one key event that needs special-case normalization), key
/// repeat/release, mouse move/click/scroll, a resize batch, a paste, and a focus change --
/// roughly proportioned the way an interactive session would actually produce them (many key
/// presses and mouse moves, occasional everything else), plus one genuinely unmappable event
/// (`KeyCode::Media`, which `from_crossterm_key_code` has no mapping for) to cover the `None`
/// path `poll_event`'s retry loop relies on.
fn representative_events() -> Vec<CtEvent> {
    let mut events = Vec::new();

    for c in "the quick brown fox".chars() {
        events.push(CtEvent::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )));
    }

    events.push(CtEvent::Key(KeyEvent::new(
        KeyCode::Tab,
        KeyModifiers::SHIFT,
    )));

    events.push(CtEvent::Key(KeyEvent::new_with_kind(
        KeyCode::Char('a'),
        KeyModifiers::NONE,
        KeyEventKind::Repeat,
    )));
    events.push(CtEvent::Key(KeyEvent::new_with_kind(
        KeyCode::Char('a'),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    )));

    for col in 0..10 {
        events.push(CtEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            column: col,
            row: 5,
            modifiers: KeyModifiers::NONE,
        }));
    }
    events.push(CtEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 3,
        row: 5,
        modifiers: KeyModifiers::NONE,
    }));
    events.push(CtEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 3,
        row: 5,
        modifiers: KeyModifiers::NONE,
    }));
    events.push(CtEvent::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 3,
        row: 5,
        modifiers: KeyModifiers::NONE,
    }));

    events.push(CtEvent::Resize(120, 40));
    events.push(CtEvent::Paste("pasted text".to_string()));
    events.push(CtEvent::FocusLost);
    events.push(CtEvent::FocusGained);

    // Unmappable: no `KeyCode::Media(_)` arm in `from_crossterm_key_code`.
    events.push(CtEvent::Key(KeyEvent::new(
        KeyCode::Media(crossterm::event::MediaKeyCode::Play),
        KeyModifiers::NONE,
    )));

    events
}

fn event_translation(c: &mut Criterion) {
    let events = representative_events();

    c.bench_function("event_translation/representative_mix", |b| {
        b.iter(|| {
            for event in &events {
                let _ = black_box(from_crossterm_event(event.clone()));
            }
        });
    });
}

criterion_group!(benches, event_translation);
criterion_main!(benches);
