//! Input event system.
//!
//! [`Terminal::poll`](crate::Terminal::poll) returns an optional [`Event`] with support for
//! keyboard ([`KeyEvent`], all standard keys plus [`KeyModifiers`]), mouse ([`MouseEvent`]:
//! buttons, movement, scroll), touch (synthesized into the same mouse events on the
//! software/WASM backend), window resize, and close events.
//! [`has_input`](crate::Terminal::has_input) checks for a pending event without blocking. Resize
//! events are applied to the grid automatically, before the event reaches your code.

use crate::grid::Pos;
use alloc::string::String;
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
/// Implemented as a manual bitflag over `u8` (`SHIFT = 1`, `CONTROL = 2`, `ALT = 4`,
/// `SUPER = 8`) rather than a [`bitflags`](https://crates.io/crates/bitflags)-generated type, so
/// this stays a plain value type with no macro-generated API surface. Combine with `|`.
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

    /// Builds modifiers from a raw bitmask, silently ignoring any bits above `SUPER` (`0b1111`).
    ///
    /// Bitmask layout: `SHIFT = 1`, `CONTROL = 2`, `ALT = 4`, `SUPER = 8`. This is the wire format
    /// shared by backends that encode modifiers as a single byte (for example the WASM backend's
    /// JS/Rust boundary).
    #[must_use]
    pub const fn from_bits_truncate(bits: u8) -> Self {
        Self(bits & 0b1111)
    }

    /// Builds modifiers from four independent flags, one per platform modifier key.
    ///
    /// Naming all four as separate parameters (rather than taking a platform-specific modifiers
    /// type) makes the modifier set exhaustive by function signature: a backend that gains a
    /// fifth modifier fails to compile at every call site instead of silently dropping it.
    #[must_use]
    // Four bools is the point: it makes the modifier set exhaustive by signature (see above).
    #[allow(clippy::fn_params_excessive_bools)]
    pub const fn from_parts(shift: bool, control: bool, alt: bool, super_: bool) -> Self {
        Self((shift as u8) | (control as u8) << 1 | (alt as u8) << 2 | (super_ as u8) << 3)
    }

    /// Returns the raw bitmask, in the same layout [`from_bits_truncate`](Self::from_bits_truncate)
    /// accepts. Only the low four bits are ever set.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

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
/// A modifier key pressed as a standalone key event, independent of the [`KeyModifiers`] flags
/// carried on non-modifier key events.
///
/// This is flat (no per-side variants) because side is conveyed separately: pair this with the
/// surrounding [`KeyEvent`]'s [`KeyLocation::Left`]/[`KeyLocation::Right`] rather than duplicating
/// left/right into `ModifierKey` itself.
///
/// Reporting a bare modifier press as a [`KeyCode::Modifier`] event is backend-dependent: the
/// crossterm backend requires the terminal to support the kitty keyboard protocol with the
/// `REPORT_ALL_KEYS_AS_ESCAPE_CODES` enhancement flag enabled; plain terminals never report these.
pub enum ModifierKey {
    /// Shift.
    Shift,
    /// Control.
    Control,
    /// Alt.
    Alt,
    /// Super/Meta (macOS Cmd, Windows/Super key).
    Super,
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
    /// A modifier key pressed on its own, without another key. See [`ModifierKey`] for the
    /// backend-availability caveat.
    Modifier(ModifierKey),
    /// Caps Lock.
    CapsLock,
    /// Scroll Lock.
    ScrollLock,
    /// Num Lock.
    NumLock,
    /// Print Screen.
    PrintScreen,
    /// Pause.
    Pause,
    /// Menu (context menu key).
    Menu,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
/// The physical location of a key on the keyboard, for keys that appear in more than one place.
///
/// Mirrors [winit's `KeyLocation`](https://docs.rs/winit/latest/winit/keyboard/enum.KeyLocation.html):
/// a key like "1" carries the same [`KeyCode`] whether it's pressed above the letters or on the
/// numpad, and modifier keys like Shift exist on both the left and right sides. This field
/// disambiguates those cases.
pub enum KeyLocation {
    /// The key is in its single, non-duplicated location, or the backend cannot determine which
    /// side/area a duplicated key came from.
    #[default]
    Standard,
    /// The key is the left-hand copy of a duplicated key (e.g. left Shift).
    Left,
    /// The key is the right-hand copy of a duplicated key (e.g. right Shift).
    Right,
    /// The key originates from the numeric keypad.
    Numpad,
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
    /// The physical location of the key, for keys that appear in more than one place.
    ///
    /// Backends that cannot determine this always report [`KeyLocation::Standard`].
    pub location: KeyLocation,
}

impl KeyEvent {
    /// Creates a key press event with the given code and modifiers, and
    /// [`KeyLocation::Standard`].
    #[must_use]
    pub const fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            location: KeyLocation::Standard,
        }
    }

    /// Creates a key event with an explicit [`KeyEventKind`] and [`KeyLocation::Standard`].
    #[must_use]
    pub const fn with_kind(code: KeyCode, modifiers: KeyModifiers, kind: KeyEventKind) -> Self {
        Self {
            code,
            modifiers,
            kind,
            location: KeyLocation::Standard,
        }
    }

    /// Creates a key event with an explicit [`KeyEventKind`] and [`KeyLocation`].
    #[must_use]
    pub const fn with_location(
        code: KeyCode,
        modifiers: KeyModifiers,
        kind: KeyEventKind,
        location: KeyLocation,
    ) -> Self {
        Self {
            code,
            modifiers,
            kind,
            location,
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

/// The system's light/dark color-scheme preference, as reported by the
/// windowing/browser layer.
///
/// Currently just these two variants: every source that can report this
/// (winit's `Theme`, the browser's `prefers-color-scheme` media query) only
/// ever resolves to one of exactly these two, and a backend that can't
/// determine a preference simply never emits [`Event::ThemeChanged`] rather
/// than emitting a third "unknown" case for callers to handle. Marked
/// `#[non_exhaustive]` for consistency with sibling public enums, in case a
/// future source (e.g. a `HighContrast` case) needs to be added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SystemTheme {
    /// The system prefers a light color scheme.
    Light,
    /// The system prefers a dark color scheme.
    Dark,
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
/// Terminal input event.
///
/// Does not derive `Eq`/`Hash`: [`MouseEvent`] does not (its `MouseEventKind::Scroll` variant's
/// `f32` fields implement neither).
pub enum Event {
    /// Keyboard event.
    Key(KeyEvent),
    /// Mouse event.
    Mouse(MouseEvent),
    /// Terminal window resized to the given `(cols, rows)`.
    ///
    /// This event does not resize anything on its own: the receiving app must call
    /// [`Terminal::resize`](crate::Terminal::resize) with these dimensions to resize the
    /// terminal's own grid buffers. A windowed backend's own reported
    /// [`Output::size`](crate::backend::Output::size) may already reflect the new dimensions
    /// by the time this event is polled, but that does not substitute for resizing the grid.
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
    /// have no equivalent free API (see the windowed backend's own docs
    /// for why) and never emit this; an app that wants a default should
    /// pick one itself rather than waiting for an event that may never
    /// arrive.
    ThemeChanged(SystemTheme),
    /// Pasted text, delivered as a single event rather than individual key
    /// presses.
    ///
    /// Not emitted by all backends: see each backend's own docs for
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
    /// the `u64` unchanged. The payload is a plain `u64`
    /// rather than an arbitrary boxed value: it keeps `Event` cheaply
    /// `Clone`/`PartialEq`/`Eq`/`Hash` (a `Box<dyn Any>` could not derive
    /// any of those) and needs no generic parameter threaded through every
    /// crate that names `Event`. Treat it as a correlation id: look up
    /// the real payload in whatever shared state or channel the sending
    /// thread already placed it in.
    Custom(u64),
}

/// Whether `new` should replace the queue's current tail event instead of being pushed alongside
/// it, when a backend is appending `new` to a `Vec`/`VecDeque` of pending events.
///
/// True only for two consecutive [`Event::Mouse`] events both carrying [`MouseEventKind::Moved`]:
/// a queue owner (winit, the wasm FFI boundary, `Headless`) can be fed pointer-move events far
/// faster than a consumer drains them, and only the most recent position matters once it does, so
/// collapsing a `Moved` run in place keeps the queue from growing unbounded (retroglyph#294,
/// retroglyph#768). Every other event kind (clicks, scrolls, keys, resize, ...) always returns
/// `false`, so this never reorders or merges anything but a `Moved` run.
#[must_use]
pub const fn coalesces_with(new: &Event, existing: &Event) -> bool {
    matches!(
        (new, existing),
        (
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Moved,
                ..
            }),
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Moved,
                ..
            }),
        )
    )
}

/// Tracks which keys are currently held down.
///
/// Feed it every [`KeyEvent`] (or [`Event`]) you receive and query
/// [`is_held`](Self::is_held) each frame for held-key movement. A key is
/// considered held from its first [`KeyEventKind::Press`] until a matching
/// [`KeyEventKind::Release`].
///
/// Held keys are keyed by `(KeyCode, KeyLocation)`, so a held Numpad8 and a held digit-row 8 are
/// tracked separately: [`is_held`](Self::is_held) takes the pair, and [`held`](Self::held) yields
/// it.
///
/// This is only useful on backends that emit release events (winit, or a
/// terminal with the kitty keyboard protocol). On press-only backends a key
/// never leaves the held set on its own, so call [`clear`](Self::clear) at a
/// suitable boundary (e.g. once per turn) if you rely on it there.
#[derive(Debug, Clone, Default)]
pub struct KeyState {
    held: Vec<(KeyCode, KeyLocation)>,
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
    /// the `(code, location)` pair; [`Release`](KeyEventKind::Release) removes it.
    pub fn apply(&mut self, event: KeyEvent) {
        let entry = (event.code, event.location);
        match event.kind {
            KeyEventKind::Press | KeyEventKind::Repeat => {
                if !self.held.contains(&entry) {
                    self.held.push(entry);
                }
            }
            KeyEventKind::Release => {
                self.held.retain(|&e| e != entry);
            }
        }
    }

    /// Updates the held set from an [`Event`], ignoring non-key events.
    pub fn apply_event(&mut self, event: &Event) {
        if let Event::Key(key) = event {
            self.apply(*key);
        }
    }

    /// Returns `true` if `code` at `location` is currently held.
    #[must_use]
    pub fn is_held(&self, code: KeyCode, location: KeyLocation) -> bool {
        self.held.contains(&(code, location))
    }

    /// Iterates the currently held `(code, location)` pairs, in first-pressed order.
    pub fn held(&self) -> impl Iterator<Item = (KeyCode, KeyLocation)> + '_ {
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
    fn test_key_modifiers_from_bits_truncate() {
        assert_eq!(KeyModifiers::from_bits_truncate(0), KeyModifiers::NONE);
        assert_eq!(KeyModifiers::from_bits_truncate(1), KeyModifiers::SHIFT);
        assert_eq!(KeyModifiers::from_bits_truncate(2), KeyModifiers::CONTROL);
        assert_eq!(KeyModifiers::from_bits_truncate(4), KeyModifiers::ALT);
        assert_eq!(KeyModifiers::from_bits_truncate(8), KeyModifiers::SUPER);
        assert_eq!(
            KeyModifiers::from_bits_truncate(0b1111),
            KeyModifiers::SHIFT | KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER
        );
        // Bits above SUPER are silently truncated.
        assert_eq!(
            KeyModifiers::from_bits_truncate(0xFF),
            KeyModifiers::SHIFT | KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER
        );
    }

    #[test]
    fn test_key_modifiers_bits_round_trip() {
        for bits in 0..=u8::MAX {
            assert_eq!(KeyModifiers::from_bits_truncate(bits).bits(), bits & 0b1111);
        }
    }

    #[test]
    fn test_key_modifiers_from_parts() {
        assert_eq!(
            KeyModifiers::from_parts(false, false, false, false),
            KeyModifiers::NONE
        );
        assert_eq!(
            KeyModifiers::from_parts(true, false, false, false),
            KeyModifiers::SHIFT
        );
        assert_eq!(
            KeyModifiers::from_parts(false, true, false, false),
            KeyModifiers::CONTROL
        );
        assert_eq!(
            KeyModifiers::from_parts(false, false, true, false),
            KeyModifiers::ALT
        );
        assert_eq!(
            KeyModifiers::from_parts(false, false, false, true),
            KeyModifiers::SUPER
        );
        assert_eq!(
            KeyModifiers::from_parts(true, true, true, true),
            KeyModifiers::SHIFT | KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER
        );
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
        assert!(!state.is_held(KeyCode::Left, KeyLocation::Standard));

        state.apply(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert!(state.is_held(KeyCode::Left, KeyLocation::Standard));

        // Repeat keeps it held.
        state.apply(KeyEvent::with_kind(
            KeyCode::Left,
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        ));
        assert!(state.is_held(KeyCode::Left, KeyLocation::Standard));

        state.apply(KeyEvent::with_kind(
            KeyCode::Left,
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));
        assert!(!state.is_held(KeyCode::Left, KeyLocation::Standard));
    }

    #[test]
    fn test_key_state_distinguishes_numpad_from_standard() {
        let mut state = KeyState::new();
        state.apply(KeyEvent::with_location(
            KeyCode::Char('8'),
            KeyModifiers::NONE,
            KeyEventKind::Press,
            KeyLocation::Numpad,
        ));
        assert!(state.is_held(KeyCode::Char('8'), KeyLocation::Numpad));
        assert!(!state.is_held(KeyCode::Char('8'), KeyLocation::Standard));

        state.apply(KeyEvent::new(KeyCode::Char('8'), KeyModifiers::NONE));
        assert!(state.is_held(KeyCode::Char('8'), KeyLocation::Standard));
        assert!(state.is_held(KeyCode::Char('8'), KeyLocation::Numpad));

        state.apply(KeyEvent::with_kind(
            KeyCode::Char('8'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));
        assert!(!state.is_held(KeyCode::Char('8'), KeyLocation::Standard));
        assert!(state.is_held(KeyCode::Char('8'), KeyLocation::Numpad));
    }

    #[test]
    fn test_key_state_apply_event_ignores_non_key() {
        let mut state = KeyState::new();
        state.apply_event(&Event::Resize(1, 1));
        assert!(state.held().next().is_none());
        state.apply_event(&Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)));
        assert!(state.is_held(KeyCode::Up, KeyLocation::Standard));
    }

    #[test]
    fn test_paste_event_carries_text() {
        use alloc::string::ToString as _;

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

    fn moved_at(x: u16, y: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            position: Pos::new(x, y),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        })
    }

    #[test]
    fn coalesces_with_true_for_two_consecutive_moved_events() {
        assert!(coalesces_with(&moved_at(1, 1), &moved_at(0, 0)));
    }

    #[test]
    fn coalesces_with_false_for_non_moved_mouse_events() {
        let down = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            position: Pos::new(0, 0),
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        });
        assert!(!coalesces_with(&moved_at(1, 1), &down));
        assert!(!coalesces_with(&down, &moved_at(0, 0)));
    }

    #[test]
    fn coalesces_with_false_for_non_mouse_events() {
        let key = Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(!coalesces_with(&moved_at(1, 1), &key));
        assert!(!coalesces_with(&key, &moved_at(0, 0)));
    }
}
