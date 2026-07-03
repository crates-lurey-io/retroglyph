//! winit-event -> retroglyph-event converters.
//!
//! Pure functions, unit-testable without a window (same role as
//! `bevy_winit::converters` or `egui-winit`'s event translation).

use retroglyph_core::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, PhysicalPos};
use retroglyph_core::grid::Pos;

/// Translates a winit key event into an [`Event`].
///
/// Returns `None` for key releases or unhandled keys.
#[must_use]
#[allow(clippy::needless_pass_by_value)]
pub fn translate_key(input: winit::event::KeyEvent, modifiers: KeyModifiers) -> Option<Event> {
    use winit::keyboard::{Key, NamedKey};

    if !input.state.is_pressed() {
        return None;
    }

    let code = match input.logical_key {
        Key::Named(NamedKey::Enter) => KeyCode::Enter,
        Key::Named(NamedKey::Escape) => KeyCode::Escape,
        Key::Named(NamedKey::Backspace) => KeyCode::Backspace,
        Key::Named(NamedKey::Delete) => KeyCode::Delete,
        Key::Named(NamedKey::Insert) => KeyCode::Insert,
        Key::Named(NamedKey::Tab) => KeyCode::Tab,
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
        Key::Character(ref s) => {
            let ch = s.chars().next()?;
            KeyCode::Char(ch)
        }
        _ => return None,
    };

    Some(Event::Key(KeyEvent::new(code, modifiers)))
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
    m
}

#[cfg(test)]
mod tests {
    use super::*;

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
