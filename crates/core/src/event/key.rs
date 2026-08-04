//! Keyboard events: [`KeyModifiers`], [`ModifierKey`], [`KeyCode`], [`KeyEventKind`],
//! [`KeyLocation`], [`KeyEvent`], and [`KeyState`].

use alloc::vec::Vec;
use core::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Not};

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
        // Mask to the defined bits so the result stays within the same invariant that
        // `from_bits_truncate` upholds; otherwise unused high bits leak into `Eq`/`Hash`.
        Self(!self.0 & 0b1111)
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
#[non_exhaustive]
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
///
/// Marked `#[non_exhaustive]` for consistency with sibling public enums, in case a future source
/// reports a state finer-grained than this set (e.g. distinguishing an OS-level key-repeat from a
/// backend-synthesized one).
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

/// Tracks which keys are currently held down.
///
/// Feed it every [`KeyEvent`] (or [`Event`](super::Event)) you receive and query
/// [`is_held`](Self::is_held) each frame for held-key movement. A key is
/// considered held from its first [`KeyEventKind::Press`] until a matching
/// [`KeyEventKind::Release`], or until [`apply_event`](Self::apply_event) sees an
/// [`Event::FocusLost`](super::Event::FocusLost), whichever comes first.
///
/// Held keys are keyed by `(KeyCode, KeyLocation)`, so a held Numpad8 and a held digit-row 8 are
/// tracked separately: [`is_held`](Self::is_held) takes the pair, and [`held`](Self::held) yields
/// it.
///
/// Release events are what actually clear a key, and not every backend emits them: plain
/// terminals only ever report [`KeyEventKind::Press`] (see [`KeyEventKind`]). On those
/// press-only backends a key never leaves the held set on its own -- not even on focus loss, since
/// there is no release to match -- so call [`clear`](Self::clear) at a suitable boundary (e.g.
/// once per turn) if you rely on held-key state there. Backends rich enough to emit releases
/// (winit, or a terminal with the kitty keyboard protocol) are exactly the ones that also emit
/// [`Event::FocusLost`](super::Event::FocusLost), so [`apply_event`](Self::apply_event) clears the
/// held set on it: without that, alt-tabbing or clicking away while a key is down would leave it
/// stuck held forever, since the release is delivered to whichever window/app gains focus instead.
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

    /// Updates the held set from an [`Event`](super::Event).
    ///
    /// Key events update the held set as [`apply`](Self::apply) does;
    /// [`Event::FocusLost`](super::Event::FocusLost) clears it entirely (see the type-level docs
    /// for why); every other event is ignored.
    pub fn apply_event(&mut self, event: &super::Event) {
        match event {
            super::Event::Key(key) => self.apply(*key),
            super::Event::FocusLost => self.clear(),
            _ => {}
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
    use super::super::Event;
    use super::*;
    use alloc::vec;

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

    /// Minimal [`Hasher`] so the regression test below works without `std` (`DefaultHasher`
    /// is a `std`-only type, and this crate is `no_std`-compatible; see `crates/core/src/lib.rs`).
    #[derive(Default)]
    struct TestHasher(u64);

    impl core::hash::Hasher for TestHasher {
        fn finish(&self) -> u64 {
            self.0
        }
        fn write(&mut self, bytes: &[u8]) {
            for byte in bytes {
                self.0 = self.0.wrapping_mul(31).wrapping_add(u64::from(*byte));
            }
        }
    }

    #[test]
    fn test_key_modifiers_not_masks_unused_bits() {
        use core::hash::Hash;

        let all =
            KeyModifiers::SHIFT | KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER;
        let inverse = !KeyModifiers::NONE;

        // `!NONE` must equal the explicit combination of all defined bits, not just satisfy
        // `contains` (which masks internally and would hide leaked high bits).
        assert_eq!(inverse, all, "NOT NONE should equal ALL");

        let mut inverse_hasher = TestHasher::default();
        inverse.hash(&mut inverse_hasher);
        let mut all_hasher = TestHasher::default();
        all.hash(&mut all_hasher);
        assert_eq!(
            core::hash::Hasher::finish(&inverse_hasher),
            core::hash::Hasher::finish(&all_hasher),
            "NOT NONE should hash the same as ALL"
        );
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
    fn test_key_state_apply_event_clears_on_focus_lost() {
        let mut state = KeyState::new();
        state.apply_event(&Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)));
        state.apply_event(&Event::Key(KeyEvent::new(
            KeyCode::Left,
            KeyModifiers::NONE,
        )));
        assert!(state.is_held(KeyCode::Up, KeyLocation::Standard));
        assert!(state.is_held(KeyCode::Left, KeyLocation::Standard));

        // Focus loss clears every held key at once, simulating alt-tabbing away while keys are
        // down: the release that would normally clear them is delivered to whichever window/app
        // gains focus instead, never to us.
        state.apply_event(&Event::FocusLost);
        assert!(!state.is_held(KeyCode::Up, KeyLocation::Standard));
        assert!(!state.is_held(KeyCode::Left, KeyLocation::Standard));
        assert!(state.held().next().is_none());
    }

    #[test]
    fn test_key_state_clear() {
        let mut state = KeyState::new();
        state.apply(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        state.apply(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(state.held().count(), 2);

        state.clear();
        assert!(state.held().next().is_none());
        assert!(!state.is_held(KeyCode::Left, KeyLocation::Standard));
        assert!(!state.is_held(KeyCode::Right, KeyLocation::Standard));

        // Clearing an already-empty state is a harmless no-op.
        state.clear();
        assert!(state.held().next().is_none());
    }

    #[test]
    fn test_key_state_held_is_in_first_pressed_order() {
        let mut state = KeyState::new();
        state.apply(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        state.apply(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        state.apply(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(
            state.held().collect::<Vec<_>>(),
            vec![
                (KeyCode::Left, KeyLocation::Standard),
                (KeyCode::Up, KeyLocation::Standard),
                (KeyCode::Right, KeyLocation::Standard),
            ]
        );

        // Releasing and re-pressing a key moves it to the back: it is first-pressed order, not
        // insertion-slot order.
        state.apply(KeyEvent::with_kind(
            KeyCode::Left,
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));
        state.apply(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(
            state.held().collect::<Vec<_>>(),
            vec![
                (KeyCode::Up, KeyLocation::Standard),
                (KeyCode::Right, KeyLocation::Standard),
                (KeyCode::Left, KeyLocation::Standard),
            ]
        );
    }

    #[test]
    fn test_key_state_release_of_unpressed_key_is_a_no_op() {
        let mut state = KeyState::new();
        state.apply(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));

        // Releasing a key that was never pressed must not disturb keys that are actually held.
        state.apply(KeyEvent::with_kind(
            KeyCode::Down,
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));
        assert!(!state.is_held(KeyCode::Down, KeyLocation::Standard));
        assert!(state.is_held(KeyCode::Up, KeyLocation::Standard));
        assert_eq!(state.held().count(), 1);
    }

    #[test]
    fn test_key_state_double_press_without_release_does_not_duplicate() {
        let mut state = KeyState::new();
        state.apply(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        state.apply(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(state.held().count(), 1);

        // A single release still clears it: the dedup guard didn't push a second entry that a
        // release would need to match twice.
        state.apply(KeyEvent::with_kind(
            KeyCode::Up,
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));
        assert!(!state.is_held(KeyCode::Up, KeyLocation::Standard));
        assert!(state.held().next().is_none());
    }
}
