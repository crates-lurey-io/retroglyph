//! Input event system.

use crate::grid::Pos;
use alloc::vec::Vec;
use core::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Not};

/// Physical (pixel) position relative to the window's top-left corner.
///
/// Using `ixy::Pos<u32>` rather than the cell-grid [`Pos`] (`ixy::Pos<u16>`)
/// makes the distinction type-safe: you cannot accidentally pass a pixel
/// coordinate where a cell coordinate is expected.
pub type PhysicalPos = ixy::Pos<u32>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
/// Keyboard modifier flags.
///
/// Implemented as a manual bitflag over `u8` rather than using the
/// [`bitflags`](https://crates.io/crates/bitflags) crate to keep the
/// dependency surface minimal for `no_std` environments. Combine with `|`.
pub struct KeyModifiers(u8);

impl KeyModifiers {
    /// No modifiers.
    pub const NONE: Self = Self(0);
    /// Shift key.
    pub const SHIFT: Self = Self(1 << 0);
    /// Control key.
    pub const CONTROL: Self = Self(1 << 1);
    /// Alt key.
    pub const ALT: Self = Self(1 << 2);
    /// Super/Meta key (macOS Cmd, Windows/Super key).
    pub const SUPER: Self = Self(1 << 3);

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
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for KeyModifiers {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl BitAnd for KeyModifiers {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl BitAndAssign for KeyModifiers {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl Not for KeyModifiers {
    type Output = Self;
    fn not(self) -> Self {
        Self(!self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
/// Whether a key event is a press, an auto-repeat, or a release.
///
/// Not every backend can distinguish these. Plain terminals only ever emit
/// [`Press`](Self::Press). Backends with richer input report the full set:
///
/// - The winit/software backend emits `Press`, `Repeat` (winit's `repeat`
///   flag), and `Release`.
/// - The crossterm backend emits the full set only when the terminal supports
///   the kitty keyboard protocol (kitty, `WezTerm`, foot, Ghostty, recent
///   Alacritty); otherwise it degrades to `Press`-only.
pub enum KeyEventKind {
    /// The key was pressed.
    #[default]
    Press,
    /// The key is held and auto-repeating.
    Repeat,
    /// The key was released.
    Release,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Keyboard input event.
pub struct KeyEvent {
    /// The key code.
    pub code: KeyCode,
    /// Modifiers held down during the event.
    pub modifiers: KeyModifiers,
    /// Whether this is a press, auto-repeat, or release.
    ///
    /// Backends that cannot distinguish these always report
    /// [`KeyEventKind::Press`]. See [`KeyEventKind`] for per-backend behavior.
    pub kind: KeyEventKind,
}

impl KeyEvent {
    /// Creates a key press event with the given code and modifiers.
    #[must_use]
    pub const fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self {
            code,
            modifiers,
            kind: KeyEventKind::Press,
        }
    }

    /// Creates a key event with an explicit [`KeyEventKind`].
    #[must_use]
    pub const fn with_kind(code: KeyCode, modifiers: KeyModifiers, kind: KeyEventKind) -> Self {
        Self {
            code,
            modifiers,
            kind,
        }
    }

    /// Returns `true` if this event is a press or auto-repeat (i.e. the key is
    /// down), and `false` for a release.
    #[must_use]
    pub const fn is_down(self) -> bool {
        matches!(self.kind, KeyEventKind::Press | KeyEventKind::Repeat)
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
/// Kinds of mouse events.
pub enum MouseEventKind {
    /// Mouse button pressed.
    Down(MouseButton),
    /// Mouse button released.
    Up(MouseButton),
    /// Mouse moved while a button was held down; carries which button.
    Drag(MouseButton),
    /// Mouse moved.
    Moved,
    /// Mouse wheel scrolled up.
    ScrollUp,
    /// Mouse wheel scrolled down.
    ScrollDown,
    /// Mouse wheel scrolled left (mostly on a laptop touchpad).
    ScrollLeft,
    /// Mouse wheel scrolled right (mostly on a laptop touchpad).
    ScrollRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Mouse input event.
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

/// The system's light/dark color-scheme preference, as reported by the
/// windowing/browser layer.
///
/// Deliberately just these two variants (not, say, a `HighContrast` or
/// `Auto` case): every source that can report this (winit's `Theme`, the
/// browser's `prefers-color-scheme` media query) only ever resolves to one
/// of exactly these two, and a backend that can't determine a preference
/// simply never emits [`Event::ThemeChanged`] rather than emitting a third
/// "unknown" case for callers to handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SystemTheme {
    /// The system prefers a light color scheme.
    Light,
    /// The system prefers a dark color scheme.
    Dark,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
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
    /// The system's light/dark color-scheme preference changed, or was
    /// determined for the first time at startup.
    ///
    /// Only backends with a real source of truth for this emit it: the
    /// windowed (winit) backend, on both native and wasm (winit's web
    /// target derives it from the browser's `prefers-color-scheme` media
    /// query, including live updates). Character-mode backends (crossterm)
    /// have no equivalent free API -- see the windowed backend's own docs
    /// for why -- and never emit this; an app that wants a default should
    /// pick one itself rather than waiting for an event that may never
    /// arrive.
    ThemeChanged(SystemTheme),
    /// Pasted text, delivered as a single event rather than individual key
    /// presses.
    ///
    /// Not emitted by all backends -- see each backend's own docs for
    /// whether and how it sources this. Content is forwarded verbatim from
    /// the source, including embedded newlines; the receiving app is
    /// responsible for any filtering it needs.
    Paste(String),
    /// The terminal or application window gained input focus.
    ///
    /// This reflects OS/terminal-level focus, not in-app widget focus (see
    /// `retroglyph-widgets`' focus ring for that).
    FocusGained,
    /// The terminal or application window lost input focus.
    ///
    /// This reflects OS/terminal-level focus, not in-app widget focus (see
    /// `retroglyph-widgets`' focus ring for that).
    FocusLost,
    /// An application-defined event injected from outside the normal input
    /// source (e.g. a network, audio, or timer thread), carrying an opaque
    /// tag the app assigns its own meaning to.
    ///
    /// Only emitted by backends with a real cross-thread injection point:
    /// the windowed (winit) backend's `EventProxy`
    /// (`retroglyph_window::winit::EventProxy::send_event`), which forwards
    /// the `u64` unchanged. The payload is deliberately a plain `u64`
    /// rather than an arbitrary boxed value: it keeps `Event` cheaply
    /// `Clone`/`PartialEq`/`Eq`/`Hash` (a `Box<dyn Any>` could not derive
    /// any of those) and needs no generic parameter threaded through every
    /// crate that names `Event`. Treat it as a correlation id -- look up
    /// the real payload in whatever shared state or channel the sending
    /// thread already placed it in.
    Custom(u64),
}

/// Tracks which keys are currently held down.
///
/// Feed it every [`KeyEvent`] (or [`Event`]) you receive and query
/// [`is_held`](Self::is_held) each frame for held-key movement. A key is
/// considered held from its first [`KeyEventKind::Press`] until a matching
/// [`KeyEventKind::Release`].
///
/// This is only useful on backends that emit release events (winit, or a
/// terminal with the kitty keyboard protocol). On press-only backends a key
/// never leaves the held set on its own, so call [`clear`](Self::clear) at a
/// suitable boundary (e.g. once per turn) if you rely on it there.
#[derive(Debug, Clone, Default)]
pub struct KeyState {
    held: Vec<KeyCode>,
}

impl KeyState {
    /// Creates an empty key-state tracker.
    #[must_use]
    pub const fn new() -> Self {
        Self { held: Vec::new() }
    }

    /// Updates the held set from a key event.
    ///
    /// [`Press`](KeyEventKind::Press) and [`Repeat`](KeyEventKind::Repeat) add
    /// the key; [`Release`](KeyEventKind::Release) removes it.
    pub fn apply(&mut self, event: KeyEvent) {
        match event.kind {
            KeyEventKind::Press | KeyEventKind::Repeat => {
                if !self.held.contains(&event.code) {
                    self.held.push(event.code);
                }
            }
            KeyEventKind::Release => {
                self.held.retain(|&c| c != event.code);
            }
        }
    }

    /// Updates the held set from an [`Event`], ignoring non-key events.
    pub fn apply_event(&mut self, event: &Event) {
        if let Event::Key(key) = event {
            self.apply(*key);
        }
    }

    /// Returns `true` if `code` is currently held.
    #[must_use]
    pub fn is_held(&self, code: KeyCode) -> bool {
        self.held.contains(&code)
    }

    /// Iterates the currently held keys, in first-pressed order.
    pub fn held(&self) -> impl Iterator<Item = KeyCode> + '_ {
        self.held.iter().copied()
    }

    /// Clears all held keys.
    pub fn clear(&mut self) {
        self.held.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_modifiers() {
        let mods = KeyModifiers::SHIFT | KeyModifiers::CONTROL;
        assert!(mods.contains(KeyModifiers::SHIFT));
        assert!(mods.contains(KeyModifiers::CONTROL));
        assert!(!mods.contains(KeyModifiers::ALT));
        assert!(!mods.is_empty());

        let inverse = !mods;
        assert!(inverse.contains(KeyModifiers::ALT));
        assert!(inverse.contains(KeyModifiers::SUPER));
        assert!(!inverse.contains(KeyModifiers::SHIFT));
        assert!(!inverse.contains(KeyModifiers::CONTROL));
    }

    #[test]
    fn test_key_modifiers_super() {
        let mods = KeyModifiers::SUPER;
        assert!(mods.contains(KeyModifiers::SUPER));
        assert!(!mods.contains(KeyModifiers::SHIFT));
        assert!(!mods.contains(KeyModifiers::CONTROL));
        assert!(!mods.contains(KeyModifiers::ALT));

        let all =
            KeyModifiers::SHIFT | KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER;
        assert!(all.contains(KeyModifiers::SUPER));
        assert!(all.contains(KeyModifiers::SHIFT));
        assert!(all.contains(KeyModifiers::CONTROL));
        assert!(all.contains(KeyModifiers::ALT));
    }

    #[test]
    fn test_event_construction() {
        let key_event = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::SHIFT);
        let event = Event::Key(key_event);

        if let Event::Key(ke) = event {
            assert_eq!(ke.code, KeyCode::Char('a'));
            assert!(ke.modifiers.contains(KeyModifiers::SHIFT));
            assert_eq!(ke.kind, KeyEventKind::Press);
        } else {
            panic!("Expected Event::Key");
        }
    }

    #[test]
    fn test_key_event_kind_helpers() {
        let press = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(press.kind, KeyEventKind::Press);
        assert!(press.is_down());

        let repeat =
            KeyEvent::with_kind(KeyCode::Char('x'), KeyModifiers::NONE, KeyEventKind::Repeat);
        assert!(repeat.is_down());

        let release = KeyEvent::with_kind(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );
        assert!(!release.is_down());
    }

    #[test]
    fn test_key_state_tracks_held_keys() {
        let mut state = KeyState::new();
        assert!(!state.is_held(KeyCode::Left));

        state.apply(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert!(state.is_held(KeyCode::Left));

        // Repeat keeps it held.
        state.apply(KeyEvent::with_kind(
            KeyCode::Left,
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        ));
        assert!(state.is_held(KeyCode::Left));

        state.apply(KeyEvent::with_kind(
            KeyCode::Left,
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));
        assert!(!state.is_held(KeyCode::Left));
    }

    #[test]
    fn test_key_state_apply_event_ignores_non_key() {
        let mut state = KeyState::new();
        state.apply_event(&Event::Resize(1, 1));
        assert!(state.held().next().is_none());
        state.apply_event(&Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)));
        assert!(state.is_held(KeyCode::Up));
    }

    #[test]
    fn test_paste_event_carries_text() {
        let event = Event::Paste("hello".to_string());
        let Event::Paste(text) = event else {
            panic!("Expected Event::Paste");
        };
        assert_eq!(text, "hello");
    }

    #[test]
    fn test_custom_event_carries_opaque_id() {
        let event = Event::Custom(42);
        let Event::Custom(id) = event else {
            panic!("Expected Event::Custom");
        };
        assert_eq!(id, 42);
        assert_ne!(Event::Custom(1), Event::Custom(2));
    }

    #[test]
    fn test_focus_gained_and_lost_are_distinct() {
        assert!(matches!(Event::FocusGained, Event::FocusGained));
        assert!(matches!(Event::FocusLost, Event::FocusLost));
        assert_ne!(Event::FocusGained, Event::FocusLost);
    }

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
    fn test_physical_pos_is_copy() {
        let p = PhysicalPos { x: 10, y: 20 };
        let q = p; // Copy
        assert_eq!(p, q);
    }
}
