//! Semantic game actions translated from raw backend events.
//!
//! Avoids scattering `KeyCode::*` matches across every example. Add new
//! variants here as demos require them; the translation is intentionally
//! narrow — don't handle events that no demo uses.
#![allow(dead_code)]

use retroglyph_core::event::{Event, KeyCode, MouseButton, MouseEventKind};

/// High-level game intent derived from a raw input event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    Interact,
    Confirm,
    Cancel,
    Quit,
    /// Mouse click at cell coordinates (col, row).
    Click(u16, u16),
    /// No action (ignored event).
    None,
}

/// Translate one [`Event`] into an [`Action`].
///
/// Arrow keys and WASD both map to movement. Enter confirms, Escape cancels
/// and quits, Q also quits. Mouse left-click maps to [`Action::Click`].
pub fn event_to_action(event: &Event) -> Action {
    match event {
        // Only act on key-down (press or auto-repeat); ignore releases so a
        // single physical keypress maps to a single action on backends that
        // report releases (winit, kitty keyboard protocol).
        Event::Key(k) if k.is_down() => match k.code {
            KeyCode::Up | KeyCode::Char('w' | 'W') => Action::MoveUp,
            KeyCode::Down | KeyCode::Char('s' | 'S') => Action::MoveDown,
            KeyCode::Left | KeyCode::Char('a' | 'A') => Action::MoveLeft,
            KeyCode::Right | KeyCode::Char('d' | 'D') => Action::MoveRight,
            KeyCode::Char('e' | 'E' | ' ') => Action::Interact,
            KeyCode::Enter => Action::Confirm,
            KeyCode::Escape | KeyCode::Char('q' | 'Q') => Action::Quit,
            _ => Action::None,
        },
        Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
            Action::Click(m.position.x, m.position.y)
        }
        Event::Close => Action::Quit,
        _ => Action::None,
    }
}

/// Drain all pending events from the terminal and return the first non-`None`
/// action, or `Action::None` if every event was uninteresting.
///
/// Crossterm and software backends buffer events; this flushes the whole
/// queue each frame so no input is silently dropped.
pub fn next_action<B: retroglyph_core::Backend>(term: &mut retroglyph_core::Terminal<B>) -> Action {
    for event in term.drain_events() {
        let action = event_to_action(&event);
        if action != Action::None {
            return action;
        }
    }
    Action::None
}
