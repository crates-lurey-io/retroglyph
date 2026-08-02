// The JS half of `retroglyph_terminal_wasm::decode_key_event`: xterm.js's `onData` callback
// reports keys as the literal VT100/xterm escape sequences a real terminal would receive on
// stdin (not structured key events), since that's what a terminal emulator's data model is. This
// table maps those sequences back to the `code`/`mods` pair `decode_key_event` (and the
// `wasm_terminal_push_key`/`wasm_terminal_example_push_key`/`wasm_app_push_key` FFI functions
// built on it) expects.
//
// Shipped from this crate (not re-derived per consumer) so every downstream game gets the same
// mapping. See retroglyph#684.
//
// Assumes the host `Terminal` is left in its default input mode: application-cursor-keys off (so
// arrows/Home/End use the `CSI` forms below, not the `SS3` application forms) and no
// `modifyOtherKeys`. F13-F24 have no single de facto sequence across terminal emulators without
// `modifyOtherKeys` and are not covered here.

/** `code` values at or above this select a named key; below it, `code` is a Unicode codepoint. */
export const NAMED_KEY_BASE = 0x00110000;

/** Mirrors `retroglyph_terminal_wasm::key_codes`. */
export const key_codes = {
  BACKSPACE: NAMED_KEY_BASE,
  ENTER: NAMED_KEY_BASE + 1,
  LEFT: NAMED_KEY_BASE + 2,
  RIGHT: NAMED_KEY_BASE + 3,
  UP: NAMED_KEY_BASE + 4,
  DOWN: NAMED_KEY_BASE + 5,
  HOME: NAMED_KEY_BASE + 6,
  END: NAMED_KEY_BASE + 7,
  PAGE_UP: NAMED_KEY_BASE + 8,
  PAGE_DOWN: NAMED_KEY_BASE + 9,
  TAB: NAMED_KEY_BASE + 10,
  BACKTAB: NAMED_KEY_BASE + 11,
  DELETE: NAMED_KEY_BASE + 12,
  INSERT: NAMED_KEY_BASE + 13,
  ESCAPE: NAMED_KEY_BASE + 14,
  F1: NAMED_KEY_BASE + 100,
};

/** `code` for `Fn` (`1 <= n <= 24`, though only F1-F12 have a mapped sequence below). */
export function functionKeyCode(n) {
  return key_codes.F1 + (n - 1);
}

/** `mods` bitmask bits, matching `retroglyph_core::event::KeyModifiers`'s layout. */
export const MOD_SHIFT = 0b0001;
export const MOD_CONTROL = 0b0010;
export const MOD_ALT = 0b0100;
export const MOD_SUPER = 0b1000;

// VT100/xterm CSI and SS3 sequences -> { code, mods }, cursor-key-mode-off, no modifyOtherKeys.
// Verified against xterm.js's own default InputHandler mapping
// (https://github.com/xtermjs/xterm.js/blob/master/src/common/InputHandler.ts) and xterm's
// ctlseqs reference (https://invisible-island.net/xterm/ctlseqs/ctlseqs.html).
const SEQUENCE_TO_KEY = {
  '\r': { code: key_codes.ENTER, mods: 0 },
  '\x7f': { code: key_codes.BACKSPACE, mods: 0 },
  '\x1b': { code: key_codes.ESCAPE, mods: 0 },
  '\t': { code: key_codes.TAB, mods: 0 },
  '\x1b[Z': { code: key_codes.BACKTAB, mods: 0 },
  '\x1b[A': { code: key_codes.UP, mods: 0 },
  '\x1b[B': { code: key_codes.DOWN, mods: 0 },
  '\x1b[C': { code: key_codes.RIGHT, mods: 0 },
  '\x1b[D': { code: key_codes.LEFT, mods: 0 },
  '\x1b[H': { code: key_codes.HOME, mods: 0 },
  '\x1b[F': { code: key_codes.END, mods: 0 },
  '\x1b[1~': { code: key_codes.HOME, mods: 0 },
  '\x1b[4~': { code: key_codes.END, mods: 0 },
  '\x1b[2~': { code: key_codes.INSERT, mods: 0 },
  '\x1b[3~': { code: key_codes.DELETE, mods: 0 },
  '\x1b[5~': { code: key_codes.PAGE_UP, mods: 0 },
  '\x1b[6~': { code: key_codes.PAGE_DOWN, mods: 0 },
  '\x1bOP': { code: functionKeyCode(1), mods: 0 },
  '\x1bOQ': { code: functionKeyCode(2), mods: 0 },
  '\x1bOR': { code: functionKeyCode(3), mods: 0 },
  '\x1bOS': { code: functionKeyCode(4), mods: 0 },
  '\x1b[15~': { code: functionKeyCode(5), mods: 0 },
  '\x1b[17~': { code: functionKeyCode(6), mods: 0 },
  '\x1b[18~': { code: functionKeyCode(7), mods: 0 },
  '\x1b[19~': { code: functionKeyCode(8), mods: 0 },
  '\x1b[20~': { code: functionKeyCode(9), mods: 0 },
  '\x1b[21~': { code: functionKeyCode(10), mods: 0 },
  '\x1b[23~': { code: functionKeyCode(11), mods: 0 },
  '\x1b[24~': { code: functionKeyCode(12), mods: 0 },
};

/**
 * Decodes one `data` string from xterm.js's `term.onData(data)` callback into a `{ code, mods }`
 * pair for `wasm_terminal_push_key`/`wasm_terminal_example_push_key`/`wasm_app_push_key` (see
 * `retroglyph_terminal_wasm::decode_key_event`'s doc comment for the encoding).
 *
 * Returns `null` for a sequence this table doesn't recognize (an unmapped escape sequence, or a
 * multi-codepoint burst xterm.js only sends from a paste -- forward those to
 * `wasm_terminal_push_paste`/`wasm_app_push_paste` instead, not this decoder).
 */
export function decodeKeyFromXtermData(data) {
  const mapped = SEQUENCE_TO_KEY[data];
  if (mapped) return mapped;
  if (data.length === 1) {
    const code = data.codePointAt(0);
    // xterm.js sends Ctrl+<letter> as the corresponding C0 control code (0x01-0x1a); surface
    // that as the plain letter + CONTROL modifier instead of an unmapped control character. 0x09
    // (Tab) and 0x0d (Enter) are excluded: those are handled by their own table entries above,
    // as themselves rather than as a Ctrl combo.
    if (code >= 0x01 && code <= 0x1a && code !== 0x09 && code !== 0x0d) {
      return { code: code - 0x01 + 'a'.codePointAt(0), mods: MOD_CONTROL };
    }
    // No modifier bits to set here: Shift is already baked into the codepoint xterm.js hands us
    // (e.g. 'A' vs 'a').
    return { code, mods: 0 };
  }
  return null;
}
