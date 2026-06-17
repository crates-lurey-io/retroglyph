//! Input event system.

use crate::grid::Position;
use core::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Not};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
/// Keyboard modifier flags.
///
/// Implemented as a manual bitflag over `u8`. Combine with `|`.
pub struct KeyModifiers(u8);

impl KeyModifiers {
    /// No modifiers.
    pub const NONE:    Self = Self(0);
    /// Shift key.
    pub const SHIFT:   Self = Self(1 << 0);
    /// Control key.
    pub const CONTROL: Self = Self(1 << 1);
    /// Alt key.
    pub const ALT:     Self = Self(1 << 2);

    /// Returns `true` if all bits in `other` are set in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Returns `true` if no modifiers are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl BitOr for KeyModifiers {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self { Self(self.0 | rhs.0) }
}

impl BitOrAssign for KeyModifiers {
    fn bitor_assign(&mut self, rhs: Self) { self.0 |= rhs.0; }
}

impl BitAnd for KeyModifiers {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self { Self(self.0 & rhs.0) }
}

impl BitAndAssign for KeyModifiers {
    fn bitand_assign(&mut self, rhs: Self) { self.0 &= rhs.0; }
}

impl Not for KeyModifiers {
    type Output = Self;
    fn not(self) -> Self { Self(!self.0) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Keyboard key codes.
pub enum KeyCode {
    /// A character key.
    Char(char),
    /// A function key.
    F(u8),
    /// Backspace.
    Backspace,
    /// Enter.
    Enter,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Home.
    Home,
    /// End.
    End,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
    /// Tab.
    Tab,
    /// Backtab.
    BackTab,
    /// Delete.
    Delete,
    /// Insert.
    Insert,
    /// Escape.
    Escape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Keyboard input event.
pub struct KeyEvent {
    /// The key code.
    pub code: KeyCode,
    /// Modifiers held down when the key was pressed.
    pub modifiers: KeyModifiers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Mouse button identifiers.
pub enum MouseButton {
    /// Left mouse button.
    Left,
    /// Right mouse button.
    Right,
    /// Middle mouse button.
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Kinds of mouse events.
pub enum MouseEventKind {
    /// Mouse button pressed.
    Down(MouseButton),
    /// Mouse button released.
    Up(MouseButton),
    /// Mouse moved.
    Moved,
    /// Mouse wheel scrolled up.
    ScrollUp,
    /// Mouse wheel scrolled down.
    ScrollDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Mouse input event.
pub struct MouseEvent {
    /// The kind of mouse event.
    pub kind: MouseEventKind,
    /// The position of the mouse.
    pub position: Position,
    /// Modifiers held down during the event.
    pub modifiers: KeyModifiers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Terminal input event.
pub enum Event {
    /// Keyboard event.
    Key(KeyEvent),
    /// Mouse event.
    Mouse(MouseEvent),
    /// Terminal window resized.
    Resize(u16, u16),
    /// Window closed.
    Close,
}
