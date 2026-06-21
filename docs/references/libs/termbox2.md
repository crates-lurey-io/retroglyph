# Reference: termbox2

## Overview

termbox2 is a single-file C library for terminal I/O, positioned as a minimal alternative to ncurses. It provides a cell-based model for terminal rendering with roughly 12 core functions, no dependencies beyond libc, and built-in escape sequences for common terminals (no terminfo db required). The library is a rewrite of the original [termbox](https://github.com/termbox/termbox) by nsf, maintained by Adam Saponara.

- **Language**: C (single header, `termbox2.h`)
- **License**: MIT
- **Current version**: 2.7.0-dev
- **Repository**: https://github.com/termbox/termbox2

## Core API

The entire public API fits on a single screen:

```c
int tb_init();
int tb_shutdown();

int tb_width();
int tb_height();

int tb_clear();
int tb_present();

int tb_set_cursor(int cx, int cy);
int tb_hide_cursor();

int tb_set_cell(int x, int y, uint32_t ch, uintattr_t fg, uintattr_t bg);

int tb_peek_event(struct tb_event *event, int timeout_ms);
int tb_poll_event(struct tb_event *event);

int tb_print(int x, int y, uintattr_t fg, uintattr_t bg, const char *str);
int tb_printf(int x, int y, uintattr_t fg, uintattr_t bg, const char *fmt, ...);
```

Additional functions exist (`tb_set_cell_ex`, `tb_extend_cell`, `tb_get_cell`, `tb_set_input_mode`, `tb_set_output_mode`, `tb_invalidate`, `tb_get_fds`, UTF-8 helpers) but the core loop is init/set_cell/present/poll_event/shutdown.

## Cell Model

The terminal screen is a flat 2D array of `tb_cell` structs:

```c
struct tb_cell {
    uint32_t ch;   // Unicode codepoint
    uintattr_t fg; // foreground color + style attributes
    uintattr_t bg; // background color + style attributes
#ifdef TB_OPT_EGC
    uint32_t *ech; // extended grapheme cluster (opt-in)
    size_t nech;
    size_t cech;
#endif
};
```

Key properties:
- **Double-buffered**: a back buffer (written to by the caller) and a front buffer (what was last sent to the terminal). `tb_present()` diffs the two and emits only changed cells.
- **One character per cell**: optimized for `wcwidth==1` codepoints. Wide characters (CJK) are handled by zeroing the following W-1 cells. Grapheme cluster support (combining marks) is opt-in via `TB_OPT_EGC`.
- **No layers**: one cell, one character, one fg/bg pair. No tile stacking.
- **Contiguous memory**: cells are indexable via `(y * width) + x`, accessible through `tb_cell_buffer()` (deprecated) or `tb_get_cell()`.

## Input Handling

Three event types via `tb_poll_event` / `tb_peek_event`:

| Event | Fields |
|---|---|
| `TB_EVENT_KEY` | `key` xor `ch`, `mod` (ALT, CTRL, SHIFT) |
| `TB_EVENT_RESIZE` | `w`, `h` |
| `TB_EVENT_MOUSE` | `key` (button), `x`, `y` |

Two input modes, set via `tb_set_input_mode`:
- **`TB_INPUT_ESC`** (default): bare Escape key generates `TB_KEY_ESC`
- **`TB_INPUT_ALT`**: bare Escape is interpreted as Alt modifier on the next key
- Mouse events are enabled by OR-ing `TB_INPUT_MOUSE`

Input parsing uses a trie of terminal-specific escape sequences, with built-in sequences for common terminals plus optional terminfo lookup.

## Color and Attribute Modes

Output mode is set via `tb_set_output_mode`:

| Mode | Range | Notes |
|---|---|---|
| `TB_OUTPUT_NORMAL` | 8 named colors + default | Always available |
| `TB_OUTPUT_256` | 0..255 | xterm-256color |
| `TB_OUTPUT_216` | 0..216 | 216-color subset |
| `TB_OUTPUT_GRAYSCALE` | 0..24 | 24 grays |
| `TB_OUTPUT_TRUECOLOR` | 0xRRGGBB | Requires `TB_OPT_ATTR_W >= 32` |

Style attributes (bold, underline, reverse, italic, blink, dim, bright) are packed into the fg/bg integer via bitwise OR. With `TB_OPT_ATTR_W=64`, additional styles are available: strikeout, double underline, overline, invisible.

The attribute width is a compile-time option (16/32/64-bit), trading memory per cell for feature range.

## Compile-Time Configuration

termbox2 uses `#define` options rather than runtime config:

| Option | Purpose |
|---|---|
| `TB_OPT_ATTR_W` | Attribute width: 16 (default), 32, or 64 |
| `TB_OPT_EGC` | Enable extended grapheme cluster support |
| `TB_OPT_PRINTF_BUF` | Size of printf buffer (default 4096) |
| `TB_OPT_READ_BUF` | Size of tty read buffer (default 64) |
| `TB_OPT_LIBC_WCHAR` | Use libc wcwidth/iswprint instead of built-in |

## Strengths

1. **Extreme simplicity**: the entire library is one header file. The API surface is ~12-15 functions. A complete program can be written in 20 lines. No build system complexity, no dependency management.

2. **No dependencies beyond libc**: unlike ncurses, there's no terminfo database requirement. Built-in escape sequences for xterm, linux console, rxvt, screen, etc. are generated via codegen.

3. **Predictable cell model**: the screen is a simple 2D array. You set cells, call present, done. No cursor movement state, no window/pad abstraction, no scrolling regions, no overlapping panels. What you write is what appears.

4. **Good error reporting**: every function returns an error code. Fine-grained error constants (`TB_ERR_INIT_OPEN`, `TB_ERR_TCGETATTR`, etc.) make debugging straightforward compared to ncurses's opaque failures.

5. **Wide language binding support**: FFI-friendly C ABI with demos in D, Go, Nim, PHP, Python, Ruby, Rust, Zig, plus community wrappers for Haskell, Crystal, Perl, Odin, JavaScript, Common Lisp, Chez Scheme.

6. **Double buffering with diffing**: `tb_present()` only emits cells that changed, keeping I/O minimal.

## Limitations

1. **No inline/partial-screen mode**: `tb_init` always switches to the alternate screen buffer and clears it. You cannot render a TUI inline within existing shell output (like fzf does). [Issue #74](https://github.com/termbox/termbox2/issues/74)

2. **Limited style attributes in default build**: with the default 16-bit attribute width, only 8 style bits are available and the bit-field is nearly full. Strikeout, double underline, and overline require 64-bit attributes. [Issue #56](https://github.com/termbox/termbox2/issues/56)

3. **No widgets, no layout**: termbox2 deliberately excludes widgets (text inputs, scroll bars, checkboxes) and layout engines. You get raw cells and nothing else. The [termbox-widgets](https://github.com/git-bruh/termbox-widgets) library and [Clay](https://github.com/nicbarker/clay) layout engine exist as external options, but the ecosystem is thin.

4. **No layers or compositing**: one character per cell, period. Overlapping UI elements (dialogs on top of content) require manually saving/restoring the underlying cells. The `tb_get_cell` function was added specifically to address this. [PR #65](https://github.com/termbox/termbox2/pull/65)

5. **Terminal compatibility edge cases**: mouse event parsing can leak partial escape sequences on some terminals (macOS Terminal.app). [Issue #108](https://github.com/termbox/termbox2/issues/108). Truecolor mode has had issues with background color after output mode changes. [Issue #51](https://github.com/termbox/termbox2/issues/51)

6. **No image/bitmap rendering**: strictly character cells. No sixel, no kitty graphics protocol, no pixel-level output.

7. **No scrollback/scrolling**: the library operates on the visible screen only.

8. **No Windows support** (yet): relies on POSIX termios. A Windows PR exists but is not merged as of writing.

## Projects Built With termbox2

Notable projects spanning text editors, feed readers, games, and system tools:

| Project | Description |
|---|---|
| [mle](https://github.com/adsr/mle) | Flexible terminal text editor (by termbox2's maintainer) |
| [ly](https://codeberg.org/fairyglade/ly) | TUI display manager for Linux/BSD |
| [newsraft](https://codeberg.org/newsraft/newsraft) | Terminal feed reader |
| [kew](https://codeberg.org/ravachol/kew) | Terminal music player |
| [ictree](https://github.com/NikitaIvanovV/ictree) | Interactive tree viewer |
| [Vgmi](https://github.com/RealMelkor/Vgmi) | Gemini protocol client |
| [matrix-tui](https://github.com/git-bruh/matrix-tui) | Matrix chat client |
| [lavat](https://github.com/AngelJumbo/lavat) | Lava lamp screensaver |
| [termbox-tetris](https://github.com/zacharygraber/termbox-tetris) | Tetris clone |
| [poe](https://sr.ht/~strahinja/poe/) | .po file editor |
| [TermCaster](https://github.com/tmpstpdwn/TermCaster) | Terminal raycaster engine |

## Comparison: termbox2 vs BearLibTerminal

Both libraries share the philosophy that ncurses is overcomplicated for what most programs need. Both provide a cell grid, simple input polling, and a small API. But they diverge sharply in scope and target audience.

### Fundamental Architecture

| Aspect | termbox2 | BearLibTerminal |
|---|---|---|
| **Target** | Real terminal emulators (xterm, etc.) | Pseudo-terminal window (own window via OpenGL) |
| **Rendering** | Writes escape sequences to tty | Renders tiles via OpenGL in its own window |
| **Output** | Characters in the terminal | Bitmap/vector tiles, TrueType fonts, images |
| **Distribution** | Single C header, no deps | Dynamic library (.dll/.so/.dylib) |

This is the core philosophical split: termbox2 runs inside your terminal. BearLibTerminal creates its own window that looks like a terminal. termbox2 is constrained by what the terminal emulator supports. BearLibTerminal has full pixel-level control.

### Cell Model

**termbox2**: one cell = one Unicode codepoint + fg + bg. That's it. Opt-in grapheme clusters. No layering, no tile stacking, no offsets.

**BearLibTerminal**: one cell can hold multiple stacked tiles, each with its own color and pixel offset. Cells exist across multiple layers. Tiles can be larger than one cell. Composition (alpha-blending multiple tiles) is a first-class feature.

```
termbox2:     cell -> (char, fg, bg)
BearLibTerminal: cell -> [layer0: [tile, tile, ...], layer1: [tile, ...], ...]
```

For a roguelike that wants a character walking over terrain with items, BearLibTerminal's model handles this natively (terrain on layer 0, items on layer 1, character on layer 2). termbox2 requires the application to manually composite everything into a single character per cell.

### API Surface

**termbox2** (~15 functions):
```
init, shutdown, width, height, clear, present,
set_cursor, hide_cursor, set_cell, poll_event, peek_event,
print, printf, set_input_mode, set_output_mode
```

**BearLibTerminal** (~30+ functions):
```
open, close, set, refresh, clear, clear_area,
layer, color, bkcolor, composition, crop,
put, put_ext, print, measure,
pick, pick_color, pick_bkcolor,
has_input, read, peek, state, delay
```

BearLibTerminal has roughly 2x the API surface. The added functions reflect its richer model: layer management, tile picking (reading back what's in a cell), per-tile offsets (`put_ext`), text measurement, state queries.

### Input Handling

| Feature | termbox2 | BearLibTerminal |
|---|---|---|
| Keyboard | Yes, with key constants + Unicode codepoint | Yes, with virtual key codes |
| Mouse | Buttons, scroll, position (opt-in) | Buttons, scroll, position, motion |
| Resize | Yes, as event | Yes, as event |
| ESC/Alt ambiguity | Configurable (ESC mode vs ALT mode) | Not applicable (own window) |
| Terminal quirks | Must handle escape sequence parsing | N/A (not a real terminal) |

termbox2's input is inherently messier because real terminals encode input as escape sequences with ambiguities (is `\x1b` an Escape key or the start of an Alt+key sequence?). BearLibTerminal sidesteps this by owning the window and getting clean OS-level input events.

### Font and Tileset Support

**termbox2**: uses whatever font the terminal emulator is configured with. No control over font rendering, tile size, or glyph appearance.

**BearLibTerminal**: full control. Load TrueType fonts at arbitrary sizes, load bitmap tilesets, assign tiles to Unicode code points with codepage mapping, control tile alignment and spacing. This is its defining feature for roguelike development.

### Philosophy Tradeoffs

| | termbox2 | BearLibTerminal |
|---|---|---|
| **Runs in** | Any terminal | Its own window |
| **SSH-friendly** | Yes | No |
| **tmux/screen** | Yes | No |
| **Tileset graphics** | No | Yes |
| **Alpha compositing** | No | Yes |
| **Pixel-level control** | No | Yes |
| **Dependencies** | libc only | OpenGL, freetype, etc. |
| **Build complexity** | Copy one header | CMake + link shared lib |
| **Portability** | Anywhere with a POSIX terminal | Windows, Linux, macOS (needs display server) |

termbox2's minimalism means it works everywhere a terminal exists: SSH sessions, containers, serial consoles, headless servers, embedded systems. BearLibTerminal requires a display server and OpenGL.

### When to Use Which

**termbox2** is the right choice for:
- TUI applications that run in existing terminals (editors, file managers, system tools)
- Programs that need to work over SSH or in tmux
- Projects where build simplicity matters (drop in a single header)
- Situations where the terminal emulator handles font rendering

**BearLibTerminal** is the right choice for:
- Roguelike games that want custom tilesets and tile compositing
- Applications that need pixel-level control over rendering
- Projects where running inside a real terminal is not a requirement
- Games that want layered rendering (terrain + items + characters)

### Shared DNA

Despite different targets, they share core design values:
- Cell-addressable grid as the fundamental abstraction
- Simple init/render/poll loop
- No attempt to be a widget toolkit
- No attempt to be ncurses-complete
- Clean C API with FFI-friendly bindings

Both reject the ncurses approach of exposing terminal capabilities as a complex abstraction layer. termbox2 says "you only need 12 functions." BearLibTerminal says "you only need a grid, layers, and tiles." Both are right for their domain.

## Sources

- [termbox2 GitHub repository](https://github.com/termbox/termbox2) - README, header file, issue tracker
- [termbox2.h raw header](https://raw.githubusercontent.com/termbox/termbox2/master/termbox2.h) - full API documentation in comments
- [BearLibTerminal GitHub](https://github.com/tommyettinger/BearLibTerminal) - feature list, API
- [BearLibTerminal design overview](http://foo.wyrd.name/en:bearlibterminal:design) - cell model, layers, tiles, tilesets
- [Issue #74: inline TUIs](https://github.com/termbox/termbox2/issues/74) - alternate screen limitation
- [Issue #56: additional styles](https://github.com/termbox/termbox2/issues/56) - attribute bit-field constraints
- [Issue #108: mouse escape sequences](https://github.com/termbox/termbox2/issues/108) - terminal compatibility
- [Issue #51: truecolor background](https://github.com/termbox/termbox2/issues/51) - output mode state bugs
- [PR #65: tb_get_cell](https://github.com/termbox/termbox2/pull/65) - front buffer read access for compositing workarounds