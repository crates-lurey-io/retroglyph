//! winit-event -> retroglyph-event converters.
//!
//! Pure functions, unit-testable without a window.

use retroglyph_core::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, PhysicalPos,
};
use retroglyph_core::grid::Pos;

/// Maps a winit logical [`Key`](winit::keyboard::Key) plus modifiers to a [`KeyCode`].
///
/// Split out from [`translate_key`] so this -- the actual key-identity logic -- is unit-testable
/// directly: `winit::event::KeyEvent` (the type `translate_key` takes) has a private
/// platform-specific field in the pinned winit version, so it can't be constructed in test code,
/// but `winit::keyboard::Key`/`NamedKey` are plain public enums a test can build directly.
fn key_code_from_logical(key: &winit::keyboard::Key, modifiers: KeyModifiers) -> Option<KeyCode> {
    use winit::keyboard::{Key, NamedKey};

    Some(match key {
        Key::Named(NamedKey::Enter) => KeyCode::Enter,
        Key::Named(NamedKey::Escape) => KeyCode::Escape,
        Key::Named(NamedKey::Backspace) => KeyCode::Backspace,
        Key::Named(NamedKey::Delete) => KeyCode::Delete,
        Key::Named(NamedKey::Insert) => KeyCode::Insert,
        // winit has no distinct "Shift+Tab" key value: `Tab` is reported with `modifiers.shift()`
        // set instead. Normalize that to `KeyCode::BackTab` here (rather than making every
        // consumer separately check `code == Tab && modifiers.contains(SHIFT)`) so the same
        // "Shift+Tab" gesture always arrives as one canonical code, matching the crossterm
        // backend's legacy `ESC[Z` -> `BackTab` behavior.
        Key::Named(NamedKey::Tab) if modifiers.contains(KeyModifiers::SHIFT) => KeyCode::BackTab,
        Key::Named(NamedKey::Tab) => KeyCode::Tab,
        // winit 0.30 still reports the spacebar as `NamedKey::Space` (a later winit version is
        // expected to switch to `Key::Character(" ")` per the UI Events spec, but that hasn't
        // shipped in the pinned 0.30 line) -- without this arm, every Space press silently falls
        // through to `_ => return None` and is dropped.
        Key::Named(NamedKey::Space) => KeyCode::Char(' '),
        Key::Named(NamedKey::ArrowUp) => KeyCode::Up,
        Key::Named(NamedKey::ArrowDown) => KeyCode::Down,
        Key::Named(NamedKey::ArrowLeft) => KeyCode::Left,
        Key::Named(NamedKey::ArrowRight) => KeyCode::Right,
        Key::Named(NamedKey::Home) => KeyCode::Home,
        Key::Named(NamedKey::End) => KeyCode::End,
        Key::Named(NamedKey::PageUp) => KeyCode::PageUp,
        Key::Named(NamedKey::PageDown) => KeyCode::PageDown,
        Key::Named(NamedKey::F1) => KeyCode::F(1),
        Key::Named(NamedKey::F2) => KeyCode::F(2),
        Key::Named(NamedKey::F3) => KeyCode::F(3),
        Key::Named(NamedKey::F4) => KeyCode::F(4),
        Key::Named(NamedKey::F5) => KeyCode::F(5),
        Key::Named(NamedKey::F6) => KeyCode::F(6),
        Key::Named(NamedKey::F7) => KeyCode::F(7),
        Key::Named(NamedKey::F8) => KeyCode::F(8),
        Key::Named(NamedKey::F9) => KeyCode::F(9),
        Key::Named(NamedKey::F10) => KeyCode::F(10),
        Key::Named(NamedKey::F11) => KeyCode::F(11),
        Key::Named(NamedKey::F12) => KeyCode::F(12),
        Key::Character(s) => KeyCode::Char(s.chars().next()?),
        _ => return None,
    })
}

/// Translates a winit key event into an [`Event`].
///
/// Reports [`KeyEventKind::Press`], [`KeyEventKind::Repeat`] (winit's `repeat`
/// flag), and [`KeyEventKind::Release`]. Returns `None` only for keys we don't
/// map.
#[must_use]
#[allow(clippy::needless_pass_by_value)]
pub fn translate_key(input: winit::event::KeyEvent, modifiers: KeyModifiers) -> Option<Event> {
    let kind = key_event_kind(input.state, input.repeat);
    let code = key_code_from_logical(&input.logical_key, modifiers)?;
    Some(Event::Key(KeyEvent::with_kind(code, modifiers, kind)))
}

/// Converts a raw f64 cursor position to a [`PhysicalPos`].
///
/// `f64.max(0.0) as u32`: the `.max(0.0)` clamp makes sign loss intentional.
/// Truncation of the fractional part is also intentional — pixel coordinates
/// are always integers.
#[must_use]
#[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
pub const fn physical_pos_from(x: f64, y: f64) -> PhysicalPos {
    PhysicalPos {
        x: x.max(0.0) as u32,
        y: y.max(0.0) as u32,
    }
}

/// Converts physical pixel coordinates to a grid cell [`Pos`].
///
/// Clamps to `u16::MAX` so out-of-bounds cursor positions (negative or
/// extremely large) don't panic — the game loop is responsible for
/// bounds-checking against the terminal size.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn pixel_to_cell(px_x: f64, px_y: f64, cell_w: u32, cell_h: u32) -> Pos {
    // .max(0.0) guards against negatives before the f64→u32 cast.
    // .min(u16::MAX as u32) guarantees the u32→u16 cast never truncates.
    let col =
        u32::checked_div(px_x.max(0.0) as u32, cell_w).map_or(0, |v| v.min(u32::from(u16::MAX)));
    let col = u16::try_from(col).unwrap_or(u16::MAX);
    let row =
        u32::checked_div(px_y.max(0.0) as u32, cell_h).map_or(0, |v| v.min(u32::from(u16::MAX)));
    let row = u16::try_from(row).unwrap_or(u16::MAX);
    Pos { x: col, y: row }
}

/// Translates a winit [`winit::event::MouseButton`] into our [`MouseButton`].
///
/// Returns `None` for side buttons and other unrecognized buttons.
#[must_use]
pub const fn translate_mouse_button(button: winit::event::MouseButton) -> Option<MouseButton> {
    match button {
        winit::event::MouseButton::Left => Some(MouseButton::Left),
        winit::event::MouseButton::Right => Some(MouseButton::Right),
        winit::event::MouseButton::Middle => Some(MouseButton::Middle),
        _ => None,
    }
}

/// Maps a winit key `state`/`repeat` pair to a [`KeyEventKind`].
#[must_use]
pub const fn key_event_kind(state: winit::event::ElementState, repeat: bool) -> KeyEventKind {
    use winit::event::ElementState;
    match (state, repeat) {
        (ElementState::Pressed, false) => KeyEventKind::Press,
        (ElementState::Pressed, true) => KeyEventKind::Repeat,
        (ElementState::Released, _) => KeyEventKind::Release,
    }
}

/// Translates winit modifier state into our [`KeyModifiers`].
#[must_use]
pub fn translate_modifiers(state: winit::keyboard::ModifiersState) -> KeyModifiers {
    let mut m = KeyModifiers::NONE;
    if state.shift_key() {
        m |= KeyModifiers::SHIFT;
    }
    if state.control_key() {
        m |= KeyModifiers::CONTROL;
    }
    if state.alt_key() {
        m |= KeyModifiers::ALT;
    }
    if state.super_key() {
        m |= KeyModifiers::SUPER;
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── key_code_from_logical ─────────────────────────────────────────────────

    #[test]
    fn space_maps_to_char_space() {
        // Regression test: winit 0.30 reports the spacebar as `NamedKey::Space`, not
        // `Key::Character(" ")` -- without a dedicated arm this silently mapped to `None` and
        // every Space press was dropped.
        let key = winit::keyboard::Key::Named(winit::keyboard::NamedKey::Space);
        assert_eq!(
            key_code_from_logical(&key, KeyModifiers::NONE),
            Some(KeyCode::Char(' '))
        );
    }

    #[test]
    fn shift_tab_normalizes_to_backtab() {
        // Regression test: winit has no distinct "Shift+Tab" key value -- it reports `Tab` with
        // the shift modifier set instead, which has to be normalized to `KeyCode::BackTab` here
        // (matching the crossterm backend's legacy `ESC[Z` -> `BackTab` behavior) or every
        // consumer of the event stream sees indistinguishable plain-Tab and Shift+Tab presses.
        let key = winit::keyboard::Key::Named(winit::keyboard::NamedKey::Tab);
        assert_eq!(
            key_code_from_logical(&key, KeyModifiers::SHIFT),
            Some(KeyCode::BackTab)
        );
    }

    #[test]
    fn plain_tab_is_unaffected() {
        let key = winit::keyboard::Key::Named(winit::keyboard::NamedKey::Tab);
        assert_eq!(
            key_code_from_logical(&key, KeyModifiers::NONE),
            Some(KeyCode::Tab)
        );
    }

    #[test]
    fn shift_modifier_on_non_tab_keys_is_unaffected() {
        let key = winit::keyboard::Key::Character("a".into());
        assert_eq!(
            key_code_from_logical(&key, KeyModifiers::SHIFT),
            Some(KeyCode::Char('a'))
        );
    }

    #[test]
    fn unmapped_key_returns_none() {
        let key = winit::keyboard::Key::Named(winit::keyboard::NamedKey::AudioVolumeUp);
        assert_eq!(key_code_from_logical(&key, KeyModifiers::NONE), None);
    }

    // ── pixel_to_cell ─────────────────────────────────────────────────────────

    #[test]
    fn pixel_to_cell_basic() {
        // 8×16 cells: pixel (20, 48) → col 2, row 3
        let pos = pixel_to_cell(20.0, 48.0, 8, 16);
        assert_eq!(pos, Pos { x: 2, y: 3 });
    }

    #[test]
    fn pixel_to_cell_origin() {
        let pos = pixel_to_cell(0.0, 0.0, 8, 16);
        assert_eq!(pos, Pos { x: 0, y: 0 });
    }

    #[test]
    fn pixel_to_cell_negative_coords_clamp_to_zero() {
        // Cursor briefly outside the window can produce negative physical coords.
        let pos = pixel_to_cell(-5.0, -10.0, 8, 16);
        assert_eq!(pos, Pos { x: 0, y: 0 });
    }

    #[test]
    fn pixel_to_cell_zero_cell_size_returns_origin() {
        // Degenerate case: backend not yet initialised with a valid cell size.
        let pos = pixel_to_cell(100.0, 200.0, 0, 0);
        assert_eq!(pos, Pos { x: 0, y: 0 });
    }

    #[test]
    fn pixel_to_cell_clamps_to_u16_max() {
        // A huge pixel coordinate must not overflow u16.
        let pos = pixel_to_cell(f64::from(u32::MAX), f64::from(u32::MAX), 1, 1);
        assert_eq!(
            pos,
            Pos {
                x: u16::MAX,
                y: u16::MAX
            }
        );
    }

    // ── translate_modifiers ──────────────────────────────────────────────────

    #[test]
    fn translate_modifiers_none() {
        let state = winit::keyboard::ModifiersState::empty();
        assert_eq!(translate_modifiers(state), KeyModifiers::NONE);
    }

    #[test]
    fn translate_modifiers_super_only() {
        let state = winit::keyboard::ModifiersState::SUPER;
        let mods = translate_modifiers(state);
        assert!(mods.contains(KeyModifiers::SUPER));
        assert!(!mods.contains(KeyModifiers::SHIFT));
        assert!(!mods.contains(KeyModifiers::CONTROL));
        assert!(!mods.contains(KeyModifiers::ALT));
    }

    #[test]
    fn translate_modifiers_super_without_super_key() {
        let state = winit::keyboard::ModifiersState::SHIFT;
        let mods = translate_modifiers(state);
        assert!(!mods.contains(KeyModifiers::SUPER));
    }

    #[test]
    fn translate_modifiers_super_combined_with_shift() {
        let state = winit::keyboard::ModifiersState::SUPER | winit::keyboard::ModifiersState::SHIFT;
        let mods = translate_modifiers(state);
        assert!(mods.contains(KeyModifiers::SUPER));
        assert!(mods.contains(KeyModifiers::SHIFT));
        assert!(!mods.contains(KeyModifiers::CONTROL));
        assert!(!mods.contains(KeyModifiers::ALT));
    }

    #[test]
    fn translate_modifiers_all_together() {
        let state = winit::keyboard::ModifiersState::SHIFT
            | winit::keyboard::ModifiersState::CONTROL
            | winit::keyboard::ModifiersState::ALT
            | winit::keyboard::ModifiersState::SUPER;
        let mods = translate_modifiers(state);
        assert!(mods.contains(KeyModifiers::SHIFT));
        assert!(mods.contains(KeyModifiers::CONTROL));
        assert!(mods.contains(KeyModifiers::ALT));
        assert!(mods.contains(KeyModifiers::SUPER));
    }

    // ── key_event_kind ────────────────────────────────────────────────────────

    #[test]
    fn key_event_kind_press_repeat_release() {
        use winit::event::ElementState;
        assert_eq!(
            key_event_kind(ElementState::Pressed, false),
            KeyEventKind::Press
        );
        assert_eq!(
            key_event_kind(ElementState::Pressed, true),
            KeyEventKind::Repeat
        );
        assert_eq!(
            key_event_kind(ElementState::Released, false),
            KeyEventKind::Release
        );
        // A release is a release regardless of the repeat flag.
        assert_eq!(
            key_event_kind(ElementState::Released, true),
            KeyEventKind::Release
        );
    }

    // ── translate_mouse_button ────────────────────────────────────────────────

    #[test]
    fn translate_mouse_button_left() {
        assert_eq!(
            translate_mouse_button(winit::event::MouseButton::Left),
            Some(MouseButton::Left)
        );
    }

    #[test]
    fn translate_mouse_button_right() {
        assert_eq!(
            translate_mouse_button(winit::event::MouseButton::Right),
            Some(MouseButton::Right)
        );
    }

    #[test]
    fn translate_mouse_button_middle() {
        assert_eq!(
            translate_mouse_button(winit::event::MouseButton::Middle),
            Some(MouseButton::Middle)
        );
    }

    #[test]
    fn translate_mouse_button_other_is_none() {
        assert_eq!(
            translate_mouse_button(winit::event::MouseButton::Back),
            None
        );
        assert_eq!(
            translate_mouse_button(winit::event::MouseButton::Forward),
            None
        );
        assert_eq!(
            translate_mouse_button(winit::event::MouseButton::Other(7)),
            None
        );
    }
}
