//! Mouse events: [`MouseButton`], [`MouseEventKind`], and [`MouseEvent`].

use super::key::KeyModifiers;
use crate::grid::Pos;

/// Physical (pixel) position relative to the window's top-left corner.
///
/// Using `ixy::Pos<u32>` rather than the cell-grid [`Pos`] (`ixy::Pos<u16>`)
/// makes the distinction type-safe: you cannot accidentally pass a pixel
/// coordinate where a cell coordinate is expected.
pub type PhysicalPos = ixy::Pos<u32>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
/// Mouse button identifiers.
pub enum MouseButton {
    /// Left mouse button.
    Left,
    /// Right mouse button.
    Right,
    /// Middle mouse button.
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
/// Kinds of mouse events.
///
/// Does not derive `Eq`/`Hash`: [`Scroll`](Self::Scroll)'s `f32` fields implement neither.
pub enum MouseEventKind {
    /// Mouse button pressed.
    Down(MouseButton),
    /// Mouse button released.
    Up(MouseButton),
    /// Mouse moved while a button was held down; carries which button.
    Drag(MouseButton),
    /// Mouse moved.
    Moved,
    /// Mouse wheel/touchpad scroll.
    ///
    /// `dy > 0.0` is scroll up, `dy < 0.0` is scroll down; `dx > 0.0` is scroll right, `dx < 0.0`
    /// is scroll left (mostly from a laptop touchpad). Magnitude is backend-dependent: the winit
    /// backend reports the exact pixel/line delta from the platform, while the crossterm backend
    /// synthesizes a fixed step of `1.0` per tick since terminals can't report scroll precision.
    Scroll {
        /// Horizontal delta. See the variant docs for the sign convention.
        dx: f32,
        /// Vertical delta. See the variant docs for the sign convention.
        dy: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
/// Mouse input event.
///
/// Does not derive `Eq`/`Hash`: [`MouseEventKind`] does not (its `Scroll` variant's `f32`
/// fields implement neither).
pub struct MouseEvent {
    /// The kind of mouse event.
    pub kind: MouseEventKind,
    /// Cell-grid position of the mouse cursor.
    pub position: Pos,
    /// Physical pixel position of the mouse cursor, relative to the window's top-left.
    ///
    /// Populated by backends that support sub-cell precision (e.g. the software
    /// renderer). `None` on character-mode backends such as crossterm.
    pub pixel_position: Option<PhysicalPos>,
    /// Modifiers held down during the event.
    pub modifiers: KeyModifiers,
}

impl MouseEvent {
    /// Creates a mouse event at the given cell-grid position, with no pixel position.
    #[must_use]
    pub const fn new(kind: MouseEventKind, position: Pos, modifiers: KeyModifiers) -> Self {
        Self {
            kind,
            position,
            pixel_position: None,
            modifiers,
        }
    }

    /// Creates a mouse event with an explicit pixel position, for backends with sub-cell
    /// precision.
    #[must_use]
    pub const fn with_pixel_position(
        kind: MouseEventKind,
        position: Pos,
        modifiers: KeyModifiers,
        pixel_position: PhysicalPos,
    ) -> Self {
        Self {
            kind,
            position,
            pixel_position: Some(pixel_position),
            modifiers,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::Event;
    use super::*;

    #[test]
    fn test_mouse_event_no_pixel_position() {
        let mouse_event = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            position: Pos { x: 10, y: 5 },
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        };
        assert!(mouse_event.pixel_position.is_none());
        assert!(matches!(Event::Mouse(mouse_event), Event::Mouse(_)));
    }

    #[test]
    fn test_mouse_event_with_pixel_position() {
        let mouse_event = MouseEvent {
            kind: MouseEventKind::Moved,
            position: Pos { x: 3, y: 2 },
            pixel_position: Some(PhysicalPos { x: 55, y: 38 }),
            modifiers: KeyModifiers::NONE,
        };
        let px = mouse_event.pixel_position.unwrap();
        assert_eq!(px.x, 55);
        assert_eq!(px.y, 38);
        // Cell and pixel positions are distinct coordinate spaces.
        assert_ne!(px.x, u32::from(mouse_event.position.x));
    }

    #[test]
    fn test_mouse_event_new_has_no_pixel_position() {
        let mouse_event = MouseEvent::new(
            MouseEventKind::Down(MouseButton::Left),
            Pos { x: 10, y: 5 },
            KeyModifiers::NONE,
        );
        assert_eq!(mouse_event.kind, MouseEventKind::Down(MouseButton::Left));
        assert_eq!(mouse_event.position, Pos { x: 10, y: 5 });
        assert!(mouse_event.pixel_position.is_none());
    }

    #[test]
    fn test_mouse_event_with_pixel_position_constructor() {
        let mouse_event = MouseEvent::with_pixel_position(
            MouseEventKind::Moved,
            Pos { x: 3, y: 2 },
            KeyModifiers::NONE,
            PhysicalPos { x: 55, y: 38 },
        );
        assert_eq!(
            mouse_event.pixel_position,
            Some(PhysicalPos { x: 55, y: 38 })
        );
    }

    #[test]
    fn test_physical_pos_is_copy() {
        let p = PhysicalPos { x: 10, y: 20 };
        let q = p; // Copy
        assert_eq!(p, q);
    }
}
