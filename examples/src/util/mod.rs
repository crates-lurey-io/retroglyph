//! Shared utilities for rg examples.

#![allow(unreachable_pub, dead_code)]

pub mod perf;
pub mod timestep;

/// WASM-only support for driving a [`Terminal<Headless>`](retroglyph_core::Terminal)
/// from a browser `requestAnimationFrame` loop instead of a canvas/window.
///
/// See [`__wasm_headless_entry!`](crate::__wasm_headless_entry) for the
/// generated `#[wasm_bindgen]` entry points. This module only holds the
/// pieces that don't need to be generated per-example: the FFI key decoder
/// and the shared state container the generated code stores in a
/// `thread_local`.
///
/// Gated on the `wasm-headless` feature only (not also `target_arch =
/// "wasm32"`): [`decode_key`] itself has no wasm-only dependencies, so it
/// stays testable on the host target. Only the generated `#[wasm_bindgen]`
/// entry points in `__wasm_headless_entry!` are wasm32-only.
#[cfg(feature = "wasm-headless")]
pub mod wasm_headless {
    /// Decode an FFI-friendly `(code, mods)` pair into a
    /// [`KeyEvent`](retroglyph_core::event::KeyEvent).
    ///
    /// `retroglyph_core::event` types are not `wasm_bindgen`-friendly (no
    /// `#[wasm_bindgen]` on the enum, no stable C-like repr contract), so the
    /// browser side calls in with two plain integers instead of a rich event
    /// type crossing the FFI boundary directly:
    ///
    /// - `code`: for printable characters, the Unicode scalar value (as you'd
    ///   get from JS `event.key.codePointAt(0)` for a single-codepoint key).
    ///   Special keys use codepoints above `0x0010_FFFF` (outside the valid
    ///   `char` range), one per [`KeyCode`](retroglyph_core::event::KeyCode)
    ///   variant that isn't `Char`/`F`. Function keys `F1..=F24` use
    ///   `SPECIAL_F0 + n`.
    /// - `mods`: a bitmask matching
    ///   [`KeyModifiers`](retroglyph_core::event::KeyModifiers)'s internal
    ///   layout: bit 0 = shift, bit 1 = control, bit 2 = alt.
    ///
    /// Returns `None` for codes that don't map to a known key (the caller
    /// should silently drop the event rather than panic — a malformed or
    /// future JS build shouldn't be able to crash the wasm module).
    #[must_use]
    pub fn decode_key(code: u32, mods: u8) -> Option<retroglyph_core::event::KeyEvent> {
        use retroglyph_core::event::{KeyCode, KeyEvent, KeyModifiers};

        // Special keys live just above the valid `char` range (0x0 ..=
        // 0x10_FFFF, minus the surrogate gap) so they can never collide with
        // a real codepoint coming from `codePointAt`.
        const SPECIAL_BASE: u32 = 0x0011_0000;
        const BACKSPACE: u32 = SPECIAL_BASE;
        const ENTER: u32 = SPECIAL_BASE + 1;
        const LEFT: u32 = SPECIAL_BASE + 2;
        const RIGHT: u32 = SPECIAL_BASE + 3;
        const UP: u32 = SPECIAL_BASE + 4;
        const DOWN: u32 = SPECIAL_BASE + 5;
        const HOME: u32 = SPECIAL_BASE + 6;
        const END: u32 = SPECIAL_BASE + 7;
        const PAGE_UP: u32 = SPECIAL_BASE + 8;
        const PAGE_DOWN: u32 = SPECIAL_BASE + 9;
        const TAB: u32 = SPECIAL_BASE + 10;
        const BACK_TAB: u32 = SPECIAL_BASE + 11;
        const DELETE: u32 = SPECIAL_BASE + 12;
        const INSERT: u32 = SPECIAL_BASE + 13;
        const ESCAPE: u32 = SPECIAL_BASE + 14;
        const SPECIAL_F0: u32 = SPECIAL_BASE + 100;

        let key_code = match code {
            BACKSPACE => KeyCode::Backspace,
            ENTER => KeyCode::Enter,
            LEFT => KeyCode::Left,
            RIGHT => KeyCode::Right,
            UP => KeyCode::Up,
            DOWN => KeyCode::Down,
            HOME => KeyCode::Home,
            END => KeyCode::End,
            PAGE_UP => KeyCode::PageUp,
            PAGE_DOWN => KeyCode::PageDown,
            TAB => KeyCode::Tab,
            BACK_TAB => KeyCode::BackTab,
            DELETE => KeyCode::Delete,
            INSERT => KeyCode::Insert,
            ESCAPE => KeyCode::Escape,
            SPECIAL_F0..=0xFFFF_FFFF if u8::try_from(code - SPECIAL_F0).is_ok() =>
            {
                #[allow(clippy::cast_possible_truncation)]
                KeyCode::F((code - SPECIAL_F0) as u8)
            }
            c => KeyCode::Char(char::from_u32(c)?),
        };

        let mut modifiers = KeyModifiers::NONE;
        if mods & 0b001 != 0 {
            modifiers |= KeyModifiers::SHIFT;
        }
        if mods & 0b010 != 0 {
            modifiers |= KeyModifiers::CONTROL;
        }
        if mods & 0b100 != 0 {
            modifiers |= KeyModifiers::ALT;
        }

        Some(KeyEvent::new(key_code, modifiers))
    }

    #[cfg(test)]
    mod tests {
        use super::decode_key;
        use retroglyph_core::event::{KeyCode, KeyModifiers};

        #[test]
        fn decodes_plain_char() {
            let ev = decode_key(u32::from('a'), 0).unwrap();
            assert_eq!(ev.code, KeyCode::Char('a'));
            assert_eq!(ev.modifiers, KeyModifiers::NONE);
        }

        #[test]
        fn decodes_arrow_keys() {
            assert_eq!(decode_key(0x0011_0002, 0).unwrap().code, KeyCode::Left);
            assert_eq!(decode_key(0x0011_0003, 0).unwrap().code, KeyCode::Right);
            assert_eq!(decode_key(0x0011_0004, 0).unwrap().code, KeyCode::Up);
            assert_eq!(decode_key(0x0011_0005, 0).unwrap().code, KeyCode::Down);
        }

        #[test]
        fn decodes_escape() {
            assert_eq!(decode_key(0x0011_000e, 0).unwrap().code, KeyCode::Escape);
        }

        #[test]
        fn decodes_modifiers() {
            let ev = decode_key(u32::from('c'), 0b011).unwrap();
            assert!(ev.modifiers.contains(KeyModifiers::SHIFT));
            assert!(ev.modifiers.contains(KeyModifiers::CONTROL));
            assert!(!ev.modifiers.contains(KeyModifiers::ALT));
        }

        #[test]
        fn decodes_function_keys() {
            assert_eq!(decode_key(0x0011_0064, 0).unwrap().code, KeyCode::F(0));
            assert_eq!(decode_key(0x0011_006e, 0).unwrap().code, KeyCode::F(10));
        }

        #[test]
        fn rejects_surrogate_codepoint() {
            // 0xD800 is a lone surrogate half; not a valid `char`, and below
            // SPECIAL_BASE, so it should fail to decode rather than panic.
            assert!(decode_key(0xD800, 0).is_none());
        }
    }
}

/// Shared FFI pointer-event decoding for the `wasm-headless` and
/// `wasm-terminal` browser entry points.
///
/// Both entry modes drive the game from JS, so pointer input (mouse clicks,
/// and taps/drags on mobile, which the templates translate from
/// `pointerdown`/`pointermove`/`pointerup`) has to cross the FFI boundary
/// the same way key events do. Mirrors the `(code, mods)` philosophy of
/// `wasm_headless::decode_key`: plain integers in, a rich
/// [`Event`](retroglyph_core::event::Event) out, `None` for anything
/// malformed rather than a panic.
#[cfg(any(feature = "wasm-headless", feature = "wasm-terminal"))]
pub mod wasm_pointer {
    use retroglyph_core::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use retroglyph_core::grid::Pos;

    /// `kind` value for a press (finger down / left button down).
    pub const KIND_DOWN: u8 = 0;
    /// `kind` value for a release (finger up / left button up).
    pub const KIND_UP: u8 = 1;
    /// `kind` value for movement while pressed (drag).
    pub const KIND_MOVE: u8 = 2;
    /// `kind` value for a scroll-up tick (mouse wheel / two-finger scroll).
    pub const KIND_SCROLL_UP: u8 = 3;
    /// `kind` value for a scroll-down tick.
    pub const KIND_SCROLL_DOWN: u8 = 4;

    /// Decode an FFI-friendly `(x, y, kind)` triple into a mouse [`Event`].
    ///
    /// `x`/`y` are cell coordinates (the JS side converts pointer pixels to
    /// cells, since only it knows the rendered cell size). Everything maps
    /// to the left button: mobile browsers only have one "button", and the
    /// examples only use left-click anyway. Returns `None` for an unknown
    /// `kind` so a newer JS build can't crash an older wasm module.
    #[must_use]
    pub const fn decode_mouse(x: u16, y: u16, kind: u8) -> Option<Event> {
        let kind = match kind {
            KIND_DOWN => MouseEventKind::Down(MouseButton::Left),
            KIND_UP => MouseEventKind::Up(MouseButton::Left),
            KIND_MOVE => MouseEventKind::Moved,
            KIND_SCROLL_UP => MouseEventKind::ScrollUp,
            KIND_SCROLL_DOWN => MouseEventKind::ScrollDown,
            _ => return None,
        };
        Some(Event::Mouse(MouseEvent {
            kind,
            position: Pos { x, y },
            pixel_position: None,
            modifiers: KeyModifiers::NONE,
        }))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn decodes_down_up_move() {
            for (kind, expect) in [
                (KIND_DOWN, MouseEventKind::Down(MouseButton::Left)),
                (KIND_UP, MouseEventKind::Up(MouseButton::Left)),
                (KIND_MOVE, MouseEventKind::Moved),
                (KIND_SCROLL_UP, MouseEventKind::ScrollUp),
                (KIND_SCROLL_DOWN, MouseEventKind::ScrollDown),
            ] {
                let Some(Event::Mouse(m)) = decode_mouse(3, 4, kind) else {
                    panic!("kind {kind} should decode");
                };
                assert_eq!(m.kind, expect);
                assert_eq!((m.position.x, m.position.y), (3, 4));
                assert!(m.pixel_position.is_none());
            }
        }

        #[test]
        fn rejects_unknown_kind() {
            assert!(decode_mouse(0, 0, 200).is_none());
        }
    }
}
