import init, {
  wasm_terminal_new,
  wasm_terminal_resize,
  wasm_terminal_push_key,
  wasm_terminal_take_output,
} from './pkg.js';

// `code` values above 0x110000 select a named key; see this crate's `key_codes` module for the
// full list (arrows, Home/End, F1-F24, etc).
const NAMED_KEY_BASE = 0x00110000;
const KEY_ENTER = NAMED_KEY_BASE + 1;
const KEY_BACKSPACE = NAMED_KEY_BASE;

// `mods` is a bitmask: SHIFT = 1, CONTROL = 2, ALT = 4, SUPER = 8.
function decodeXtermData(data) {
  if (data === '\r') return { code: KEY_ENTER, mods: 0 };
  if (data === '\x7f') return { code: KEY_BACKSPACE, mods: 0 };
  // A single printable character forwards as its Unicode codepoint; xterm.js already resolves
  // Shift into the codepoint itself (e.g. 'A' vs 'a'), so no SHIFT bit is needed here.
  if (data.length === 1) return { code: data.codePointAt(0), mods: 0 };
  return null;
}

async function main() {
  await init();

  const term = new Terminal({ cols: 80, rows: 24 });
  term.open(document.getElementById('screen'));

  const handle = wasm_terminal_new(term.cols, term.rows);

  term.onData((data) => {
    const key = decodeXtermData(data);
    if (key) wasm_terminal_push_key(handle, key.code, key.mods);
  });

  window.addEventListener('resize', () => {
    // Call whatever fit-to-container logic resizes `term` first (e.g. xterm.js's FitAddon), then
    // tell the backend to match.
    wasm_terminal_resize(handle, term.cols, term.rows);
  });

  function frame() {
    const ansi = wasm_terminal_take_output(handle);
    if (ansi) term.write(ansi);
    requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);
}

main();
