# Research: ANSI Terminal Backend Implementation

## Summary

An ANSI terminal backend renders a grid-based UI by writing escape sequences to a real terminal's
PTY, reusing the host terminal emulator for display. The core loop is: maintain a double-buffered
cell grid, diff current vs. previous frame, emit only changed cells as cursor-movement + SGR-color +
UTF-8 sequences, and wrap the frame in synchronized output markers to prevent flicker. Crossterm and
ratatui provide the canonical Rust reference implementation for this pattern. The main trade-off is
universal portability (works over SSH, inside tmux, on any terminal emulator) at the cost of visual
fidelity (no custom fonts, no pixel-level graphics, inconsistent Unicode rendering across
terminals).

## 1. How crossterm and termion work

### Raw mode and termios

Both libraries enter **raw mode** by manipulating the terminal's `termios` struct via
`tcgetattr`/`tcsetattr` (libc) or `rustix::termios`. Raw mode disables:

- Line buffering (canonical mode): input arrives byte-by-byte, not line-by-line.
- Echo: typed characters are not printed back.
- Signal processing: Ctrl+C sends raw bytes instead of SIGINT.
- Output post-processing: `\n` means cursor-down, not carriage-return + line-feed.

Crossterm saves the original `termios` state in a global `Mutex<Option<Termios>>` and restores it on
drop. Termion wraps stdout in `RawTerminal<W>` which restores state in its `Drop` impl, and also
exposes `suspend_raw_mode()`/`activate_raw_mode()` for temporary mode switches.
[crossterm raw mode](https://docs.rs/crossterm/latest/crossterm/terminal/index.html),
[termion raw.rs](https://github.com/redox-os/termion/blob/master/src/raw.rs)

### ANSI escape sequence output

Both libraries write ANSI/VT escape sequences to stdout (or any `Write` target). Crossterm uses a
**command pattern**: each operation (move cursor, set color, clear screen) is a struct implementing
`Command`, which writes escape bytes to a buffer. Commands can be executed immediately or queued for
batch flushing.

```text
// Crossterm command examples (what gets written to the writer):
\x1b[H         // CUP: move cursor to (1,1)
\x1b[2J        // ED: erase entire display
\x1b[38;2;R;G;Bm  // SGR: set foreground to RGB
\x1b[?1049h    // DECSET: switch to alternate screen
\x1b[?25l      // DECTCEM: hide cursor
```rust

Termion takes a different approach using Rust's `Display` trait: terminal operations are types that
implement `fmt::Display`, so you write them inline with `write!()` macros.
[crossterm docs](https://docs.rs/crossterm/), [termion docs](https://docs.rs/termion)

### Event source / input reading

Crossterm provides two Unix event source strategies (selected by feature flags):

1. **MIO-based (default)**: Uses the `mio` crate for async I/O polling. Registers three tokens: TTY

   input, SIGWINCH (resize), and a wake pipe for async cancellation.

1. **TTY-based (`use-dev-tty`)**: Uses direct `poll()` on file descriptors. Opens `/dev/tty` if

   stdin is not a TTY (for piped scenarios).

Both feed raw bytes into a `Parser` that accumulates bytes in a 256-byte buffer, attempts to parse
ANSI escape sequences, and queues structured `Event` values (key, mouse, resize) into a `VecDeque`.
The parser uses a `more` flag to decide whether to wait for additional bytes or treat the current
buffer as complete (relevant for disambiguating bare ESC from escape sequence prefixes).
[crossterm event system](https://deepwiki.com/crossterm-rs/crossterm/6.3-event-system-architecture),
[crossterm unix impl](https://deepwiki.com/crossterm-rs/crossterm/6.1-unix-implementation)

Termion uses a simpler `Events` iterator that reads from an `AsyncReader` (a thread that reads stdin
in the background) and parses escape sequences synchronously.

### Key difference: crossterm vs termion

| Aspect        | crossterm                        | termion                            |
| ------------- | -------------------------------- | ---------------------------------- |
| Platform      | Cross-platform (Unix + Windows)  | Unix only (Linux, macOS, Redox)    |
| API style     | Command structs, queue/execute   | `Display` trait, write!() macro    |
| Async support | Feature-flagged (`event-stream`) | Thread-based `AsyncReader`         |
| Maintenance   | Actively maintained              | Less active, community forks exist |

## 2. Cell diffing for efficient output

### Ratatui's double-buffer approach

Ratatui maintains two `Buffer` instances (current and previous). Each `Buffer` is a flat `Vec<Cell>`
covering the viewport, where each `Cell` stores a grapheme string (`CompactString`),
foreground/background color, and style modifiers.

The rendering cycle in `Terminal::draw()`:

1. Check terminal size, resize buffers if needed.
2. Create a `Frame` backed by the current buffer.
3. Call the user's render closure, which writes widgets into the buffer.
4. Call `Terminal::flush()`, which diffs current vs. previous and writes only changes.
5. Swap buffers (previous becomes current for next frame).

### The diff algorithm (`BufferDiff`)

Ratatui's `BufferDiff` is a zero-allocation iterator that walks both buffers in lockstep, yielding
`(x, y, &Cell)` for each cell that differs. Key behaviors:

- **Skip identical cells**: If `current == previous`, no output is emitted. This is the primary

  optimization: for a mostly-static UI, only a handful of cells change per frame.

- **Multi-width character handling**: When a cell has width > 1 (CJK, some emoji), the iterator

  skips the trailing placeholder cells. For VS16 emoji (containing U+FE0F), it explicitly clears
  trailing cells since some terminals don't handle this correctly.

- **CellDiffOption directives**: Cells can be marked `Skip` (never emit), `ForcedWidth` (override

  width calculation), or `None` (normal diff).

The backend then converts diff output to escape sequences:

```text
// For each changed cell (x, y, cell):
\x1b[{y+1};{x+1}H    // Move cursor to position (1-indexed)
\x1b[38;2;R;G;Bm      // Set foreground color
\x1b[48;2;R;G;Bm      // Set background color
{cell.symbol}          // Write the grapheme
```rust

The backend optimizes cursor movement: if the next changed cell is adjacent to the current cursor
position, no CUP sequence is emitted (the cursor naturally advances after printing a character).
[ratatui buffer/diff.rs](https://github.com/ratatui/ratatui/blob/1ce29d66/ratatui-core/src/buffer/diff.rs),
[ratatui rendering docs](https://ratatui.rs/concepts/rendering/under-the-hood/)

### Notcurses' approach for comparison

Notcurses uses a two-phase render/rasterize pipeline. **Rendering** flattens a z-ordered pile of
ncplanes into a single cell matrix using a depth-buffer algorithm (top-to-bottom, considering
alpha/transparency). **Rasterizing** diffs this matrix against the "lastframe" (damage map) and
generates an optimized escape sequence stream. The damage map is shared state updated on each
rasterize call. Notcurses also optimizes cursor movement and batches SGR state changes.
[notcurses_render(3)](https://notcurses.com/notcurses_render.3.html)

## 3. Color handling

### Color levels

Terminals support four color tiers:

| Level              | Sequences                    | Colors                        | Detection                       |
| ------------------ | ---------------------------- | ----------------------------- | ------------------------------- |
| No color           | None                         | Text only                     | `$NO_COLOR` set, or not a TTY   |
| ANSI 16            | `\x1b[30-37m`, `\x1b[90-97m` | 8 colors + 8 bright           | Default for `TERM=linux`        |
| ANSI 256           | `\x1b[38;5;Nm`               | 216 cube + 24 grays + 16 ANSI | `TERM` contains `256color`      |
| Truecolor (24-bit) | `\x1b[38;2;R;G;Bm`           | 16.7M                         | `$COLORTERM=truecolor` or query |

### Detection strategies

1. **Environment variables**: `$COLORTERM` (`truecolor`/`24bit`), `$TERM` (contains `256color`),

   `$NO_COLOR` (disable all color).

1. **DECRQSS query** (`\x1bP$qm\x1b\\`): Some terminals respond with their current SGR state,

   revealing truecolor support. The `termprofile` crate implements this.

1. **Terminal identification**: `$TERM_PROGRAM` identifies specific emulators with known

   capabilities.

1. **Conservative fallback**: Default to 256-color, degrade to 16 if the terminal is unrecognized.

### Graceful degradation

When truecolor is unavailable, map RGB values to the nearest ANSI 256 or 16 color. The
`ansi_colours` crate provides efficient RGB-to-256 mapping using perceptual color distance.
BurntSushi's `termcolor` crate provides a `WriteColor` trait with `Ansi` (escape sequences) and
`NoColor` (strips colors) implementations. [ansi_colours](https://crates.io/crates/ansi_colours),
[termcolor](https://github.com/BurntSushi/termcolor),
[termprofile](https://docs.rs/termprofile/latest/termprofile/)

### Implementation recommendation

Store colors internally as RGB. At render time, convert based on detected terminal capabilities:

```rust
enum ColorMode { TrueColor, Ansi256, Ansi16, NoColor }

fn emit_fg(color: Rgb, mode: ColorMode) -> String {
    match mode {
        ColorMode::TrueColor => format!("\x1b[38;2;{};{};{}m", color.r, color.g, color.b),
        ColorMode::Ansi256   => format!("\x1b[38;5;{}m", rgb_to_ansi256(color)),
        ColorMode::Ansi16    => format!("\x1b[{}m", rgb_to_ansi16(color)),
        ColorMode::NoColor   => String::new(),
    }
}
```

## 4. Unicode and wide character handling

### The core problem

Terminal cells are a monospaced grid. Most characters occupy 1 cell, but CJK ideographs, some emoji,
and certain symbols occupy 2 cells ("fullwidth"). Zero-width characters (combining marks, ZWJ)
occupy 0 cells. The renderer must know the display width of every grapheme to correctly position
content.

### Width calculation

The `unicode-width` crate (used by ratatui, crossterm, and most Rust terminal libraries) implements
Unicode Standard Annex #11 rules. It provides `UnicodeWidthChar::width()` and
`UnicodeWidthStr::width()`. The `cjk` feature flag (enabled by default) adjusts width for
ambiguous-width characters in CJK contexts.

For grapheme clusters (emoji sequences like 👨‍👩‍👧‍👦), `unicode-width` alone is insufficient; you need
grapheme segmentation (via `unicode-segmentation`) followed by width measurement. The
`unicode-display-width` crate handles this correctly, measuring grapheme clusters rather than
individual characters. [unicode-width](https://docs.rs/unicode-width/latest/unicode_width/),
[unicode-display-width](https://docs.rs/unicode-display-width/latest/unicode_display_width/)

### Rendering wide characters

When a character occupies 2 cells, the buffer must mark the second cell as a "continuation" or
"placeholder" cell. Ratatui stores the grapheme in the first cell and sets the trailing cell to a
special empty state with `CellWidth` metadata. During diffing, the trailing cell is skipped (the
terminal auto-advances the cursor past it when printing the wide character).

The tricky cases:

- **Overwriting a wide char with narrow chars**: Must explicitly clear both cells. If you write a

  single-width "a" at position 0 where a 2-wide "東" was, position 1 still shows the right half of
  the old character unless cleared.

- **VS16 emoji (e.g., ⌨️)**: Some emoji are made wide by a Variation Selector 16 (U+FE0F). Terminals

  inconsistently handle the trailing cell, so ratatui's diff explicitly clears trailing cells for
  VS16 sequences.

- **Terminal disagreement**: Terminals may disagree on the width of certain characters (emoji,

  ambiguous-width). There is no universal solution; the best approach is to use `unicode-width` and
  accept some terminals will render incorrectly.

## 5. Input handling differences from windowed mode

### Escape sequence ambiguity

In a windowed/GUI backend, input arrives as structured key events with modifiers. In a terminal,
input arrives as raw bytes that must be parsed:

- **Bare ESC vs. sequence start**: `\x1b` alone is the Escape key, but it's also the prefix for all

  escape sequences (`\x1b[A` = Up arrow). Parsers use timeouts (typically 50-100ms): if no bytes
  follow ESC within the timeout, treat it as a standalone Escape keypress. This is inherently
  fragile over high-latency connections (SSH).

- **Ctrl key collisions**: `Ctrl+I` = `\t` (Tab), `Ctrl+M` = `\r` (Enter), `Ctrl+H` = `\x08`

  (Backspace). These are indistinguishable in legacy mode.

- **No key-release events**: Traditional terminals only report key press, not release.
- **No modifier disambiguation**: `Ctrl+Shift+A` often sends the same bytes as `Ctrl+A`.

### Kitty keyboard protocol

The Kitty keyboard protocol (`CSI > flags u`) solves these ambiguities. Applications opt in with a
bitmask of flags:

| Flag                       | Bit | Feature                                 |
| -------------------------- | --- | --------------------------------------- |
| Disambiguate               | 1   | Distinguish Ctrl+I from Tab, etc.       |
| Report event types         | 2   | Press, repeat, release events           |
| Report alternate keys      | 4   | Report both logical and physical key    |
| Report all keys as escapes | 8   | Even unmodified keys use CSI u format   |
| Report associated text     | 16  | Include text generated by the key event |

The protocol uses a stack: `CSI > flags u` pushes flags, `CSI < u` pops. Key events encode as
`CSI keycode ; modifiers u` (or `CSI keycode ; modifiers : event-type u` for release/repeat). This
is supported by Kitty, WezTerm, Ghostty, foot, and recent versions of many other terminals.
[Kitty keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/),
[terminfo.dev](https://terminfo.dev/extensions/kitty-keyboard-protocol)

### xterm modifyOtherKeys

An older alternative: `CSI > 4 ; 2 m` (mode 2) makes xterm send modified keys as
`CSI 27 ; modifier ; keycode ~`. Mode 3 (xterm patch 398, April 2025) extends this to ALL
keypresses. tmux supports modifyOtherKeys but strips Kitty keyboard sequences.

### Implementation recommendation (2)

1. On startup, attempt to enable Kitty keyboard protocol (push flags).
2. Fall back to xterm modifyOtherKeys mode 2 if Kitty isn't supported.
3. Fall back to legacy parsing with ESC timeout.
4. On shutdown, pop the keyboard protocol stack / reset modes.

## 6. Synchronized output

### The problem

When an application sends multiple write() calls to update the screen, the terminal may render
intermediate states, causing visible flicker or tearing. This is especially bad when the output is
split across multiple PTY reads.

### DECSET 2026 (BSU/ESU)

The synchronized output protocol uses DEC private mode 2026:

```text
\x1b[?2026h    // BSU: Begin Synchronized Update
... frame content (cursor moves, colors, text) ...
\x1b[?2026l    // ESU: End Synchronized Update
```text

Between BSU and ESU, the terminal buffers all output and renders it as a single atomic frame.
Originally proposed by iTerm2 using DCS sequences, the community converged on the simpler `SM ?` /
`RM ?` syntax (DECSET/DECRST).

### Terminal support

Supported by: iTerm2, Kitty, WezTerm, Ghostty, Contour, foot, Windows Terminal, Alacritty (recent),
and others. The `terminfo.dev` compatibility matrix shows broad adoption in modern terminals.

Not supported by: some older terminal emulators, some terminal multiplexer pass-throughs. Terminals
that don't understand mode 2026 silently ignore it, so it's safe to always emit.

### tmux considerations

tmux may strip or not forward mode 2026 depending on version. Newer tmux versions (3.3+) pass it
through. For older tmux, the sequences are harmless (silently ignored by both tmux and the outer
terminal).
[synchronized output spec](https://github.com/contour-terminal/vt-extensions/blob/master/synchronized-output.md),
[terminfo.dev DECSET 2026](https://terminfo.dev/modes/decset-2026-synchronized-output)

### Implementation

Wrap every frame render in BSU/ESU:

```rust
fn render_frame(&mut self, diff: impl Iterator<Item = (u16, u16, &Cell)>) {
    self.write(b"\x1b[?2026h"); // BSU
    for (x, y, cell) in diff {
        self.move_cursor(x, y);
        self.set_style(cell);
        self.write(cell.symbol().as_bytes());
    }
    self.write(b"\x1b[?2026l"); // ESU
    self.flush();
}
```

## 7. Mouse support

### Tracking modes

Mouse support is enabled by sending DECSET sequences. These are stackable:

| Mode           | DECSET  | Reports                                           |
| -------------- | ------- | ------------------------------------------------- |
| X10            | `?9`    | Button press only (no release)                    |
| Normal (VT200) | `?1000` | Press + release                                   |
| Button event   | `?1002` | Press + release + drag (motion while button held) |
| Any event      | `?1003` | All motion (even without button)                  |

### Encoding formats

| Format         | DECSET    | Wire format                                                  | Coordinate limit |
| -------------- | --------- | ------------------------------------------------------------ | ---------------- |
| X10/X11 legacy | (default) | `\x1b[M CbCxCy` (3 raw bytes, value+32)                      | 223 columns/rows |
| UTF-8          | `?1005`   | Like X10 but coordinates UTF-8 encoded                       | ~2047            |
| SGR            | `?1006`   | `\x1b[<button;col;row M/m` (decimal, 'M'=press, 'm'=release) | Unlimited        |
| urxvt          | `?1015`   | `\x1b[button;col;row M`                                      | Unlimited        |

**Always use SGR (1006)**. It's the modern standard: no coordinate limits, distinguishes press ('M')
from release ('m'), and is supported by all modern terminals.
[terminfo.dev SGR mouse](https://terminfo.dev/modes/decset-1006-sgr-mouse)

### Pixel-level mouse (SGR-Pixels)

DECSET `?1016` provides pixel-level coordinates instead of cell coordinates. Useful if you need
sub-cell precision. Not widely supported yet.

### Implementation (2)

```rust
fn enable_mouse(&mut self) {
    self.write(b"\x1b[?1000h");  // Normal tracking (press+release)
    self.write(b"\x1b[?1002h");  // Button event tracking (drag)
    self.write(b"\x1b[?1003h");  // Any event tracking (hover)
    self.write(b"\x1b[?1006h");  // SGR encoding
}

fn disable_mouse(&mut self) {
    self.write(b"\x1b[?1006l");
    self.write(b"\x1b[?1003l");
    self.write(b"\x1b[?1002l");
    self.write(b"\x1b[?1000l");
}
```

Parse SGR mouse events: `\x1b[<button;col;row M` (press) or `\x1b[<button;col;row m` (release).
Button field encodes: 0=left, 1=middle, 2=right, 64=scroll-up, 65=scroll-down. Add 4 for Shift, 8
for Alt, 16 for Ctrl.

## 8. How ratatui/crossterm/notcurses implement terminal rendering

### Ratatui's `Backend` trait

Ratatui defines a `Backend` trait that abstracts the terminal interface:

```rust
pub trait Backend {
    type Error: Error;
    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
        where I: Iterator<Item = (u16, u16, &'a Cell)>;
    fn hide_cursor(&mut self) -> Result<(), Self::Error>;
    fn show_cursor(&mut self) -> Result<(), Self::Error>;
    fn get_cursor_position(&mut self) -> Result<Position, Self::Error>;
    fn set_cursor_position<P>(&mut self, position: P) -> Result<(), Self::Error>;
    fn clear(&mut self) -> Result<(), Self::Error>;
    fn size(&mut self) -> Result<Size, Self::Error>;
    fn window_size(&mut self) -> Result<WindowSize, Self::Error>;
    fn flush(&mut self) -> Result<(), Self::Error>;
    // ... scroll, cursor_style, etc.
}
```

The `draw()` method receives an iterator of `(x, y, &Cell)` tuples (the diff output) and is
responsible for emitting the corresponding escape sequences. The `CrosstermBackend` implementation
batches cursor moves + style sets + character writes, using crossterm's `queue!` macro for deferred
execution, then `flush()` writes everything at once.

### Crossterm backend rendering flow

1. `Terminal::draw()` calls the user closure to populate the buffer.
2. `Terminal::flush()` computes `BufferDiff` between current and previous buffers.
3. For each changed cell, the CrosstermBackend:
   - Emits `MoveTo(x, y)` if cursor isn't already there.
   - Emits `SetForegroundColor`, `SetBackgroundColor`, style modifiers if they changed from the last

     emitted cell.

   - Writes the cell's symbol string.
4. Final `io::Write::flush()` pushes all queued bytes to the terminal.
### Notcurses rendering pipeline

Notcurses separates rendering from rasterizing:

1. **Render phase** (`ncpile_render`): Flattens the z-ordered ncplane stack using a depth-buffer

   algorithm. At each (x, y), walks planes top-to-bottom to determine the visible EGC, foreground,
   background, and style. Handles alpha blending between layers.

1. **Rasterize phase** (`ncpile_rasterize`): Compares the rendered matrix against the "lastframe"

   damage map. Generates an optimized escape sequence stream: skips unchanged cells, batches
   adjacent changes, minimizes cursor movement, and coalesces SGR state changes. Writes the stream
   and updates the lastframe.

Multiple piles can be rendered concurrently (each pile is a separate z-stack). Only rasterization is
serialized (one write stream to the terminal at a time).
[notcurses_render(3)](https://notcurses.com/notcurses_render.3.html)

## 9. Limitations vs. a windowed backend

| Capability                   | ANSI Terminal Backend                                           | Windowed Backend (GPU/software)           |
| ---------------------------- | --------------------------------------------------------------- | ----------------------------------------- |
| **Font rendering**           | Host terminal's font only                                       | Custom fonts, sizes, fallback chains      |
| **Graphics**                 | Text/Unicode only (or Sixel/Kitty graphics protocol for images) | Arbitrary pixel rendering, tiles, sprites |
| **Color accuracy**           | Depends on terminal's color scheme                              | Full sRGB/linear control                  |
| **Cell size**                | Fixed by terminal, unknown to app                               | Known, controllable                       |
| **Refresh rate**             | Terminal's repaint rate (often ~60fps but uncontrollable)       | vsync-controlled, predictable             |
| **Input fidelity**           | Escape sequence parsing, ambiguities                            | Direct key/mouse events from OS           |
| **Unicode consistency**      | Varies by terminal (width, emoji, ligatures)                    | Controlled by your renderer               |
| **Transparency/compositing** | Terminal-dependent                                              | Full alpha, layering                      |
| **Custom glyphs / tiles**    | Not possible without image protocols                            | Arbitrary bitmap/vector rendering         |
| **Scrollback**               | Terminal provides it (may conflict with alternate screen)       | Application-controlled                    |

### Terminal inconsistencies to watch for

- **Wide character width disagreement**: Terminal measures "🤷" as 2 cells, but some older terminals

  render it as 1, causing misalignment.

- **Emoji variation selectors**: VS16 (U+FE0F) can change a character from narrow to wide, but not

  all terminals respect this.

- **Undercurl/underline styles**: `SGR 4:3` (curly underline) is supported by Kitty, WezTerm,

  Ghostty, but not all terminals.

- **Bracketed paste**: Terminal may or may not support it; must be detected and handled.
- **Alternate screen buffer**: `\x1b[?1049h` is widely supported but behavior with scrollback

  varies.

## 10. Trade-offs

### Advantages of ANSI terminal backend

1. **Works everywhere**: SSH, tmux, screen, Docker containers, CI environments, serial consoles. No

   GPU, no display server, no windowing system required.

1. **Zero dependencies on graphics stack**: No OpenGL/Vulkan/Metal, no font rasterizer, no window

   manager integration.

1. **Users choose their terminal**: Font, color scheme, opacity, tabs, splits are all controlled by

   the user's preferred terminal emulator.

1. **Small binary size**: Terminal I/O adds minimal code compared to a full rendering engine.
1. **Accessibility**: Terminal emulators often have built-in screen reader support.1. **Copy/paste for free**: The terminal provides native text selection.
### Disadvantages

1. **Visual ceiling**: Cannot render anything beyond the character grid. No anti-aliased custom

   fonts, no sub-cell positioning, no tile-based game graphics (without Sixel/Kitty image protocol
   hacks).

1. **Inconsistent rendering**: The same escape sequences look different across terminals, especially

   regarding color, Unicode width, and underline styles.

1. **Input limitations**: Legacy keyboard handling loses modifier information. Even with Kitty

   protocol, tmux strips it. ESC ambiguity over SSH is unavoidable in legacy mode.

1. **tmux/multiplexer tax**: Features like synchronized output, Kitty keyboard, graphics protocols

   may be stripped or mangled by the multiplexer layer. Applications must detect and degrade.

1. **No reliable capability detection**: Unlike a GPU backend where you query OpenGL extensions,

   terminal capabilities are discovered through fragile heuristics ($TERM, $COLORTERM, DECRQSS
   queries, $TERM_PROGRAM).

1. **Performance ceiling**: For very large/complex UIs, the escape sequence stream can become a

   bottleneck, especially over SSH. Cell diffing mitigates this but doesn't eliminate it.

### When to use which

| Scenario                                    | Recommendation                                                    |
| ------------------------------------------- | ----------------------------------------------------------------- |
| TUI app (editor, dashboard, CLI tool)       | ANSI terminal backend                                             |
| Game with pixel graphics                    | Windowed backend                                                  |
| Remote development tool                     | ANSI terminal (SSH requirement)                                   |
| Application needing custom fonts/tiles      | Windowed backend                                                  |
| Cross-platform CLI shipped as single binary | ANSI terminal backend                                             |
| Application with both modes                 | Implement both backends behind a trait (like ratatui's `Backend`) |

## Sources

### Kept

- [crossterm docs](https://docs.rs/crossterm/) - Primary Rust terminal library docs, command

  pattern, raw mode API

- [crossterm Unix implementation (DeepWiki)](https://deepwiki.com/crossterm-rs/crossterm/6.1-unix-implementation) -

  Detailed architecture: TTY/mio event sources, file descriptor handling, parser

- [crossterm raw mode (DeepWiki)](https://deepwiki.com/crossterm-rs/crossterm/3.1-raw-mode) - Raw

  mode internals, termios manipulation

- [termion source](https://github.com/redox-os/termion) - Alternative Rust terminal library, simpler

  architecture

- [ratatui rendering docs](https://ratatui.rs/concepts/rendering/under-the-hood/) - Double-buffer,

  widget rendering, flush/diff cycle

- [ratatui buffer/diff.rs](https://github.com/ratatui/ratatui/blob/1ce29d66/ratatui-core/src/buffer/diff.rs) -

  Zero-allocation diff iterator implementation

- [ratatui Backend trait](https://docs.rs/ratatui/latest/ratatui/backend/trait.Backend.html) -

  Backend abstraction interface

- [notcurses_render(3)](https://notcurses.com/notcurses_render.3.html) - Render/rasterize pipeline,

  cell algorithm, damage map

- [Kitty keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/) - Primary spec for

  progressive keyboard enhancement

- [terminfo.dev Kitty keyboard](https://terminfo.dev/extensions/kitty-keyboard-protocol) -

  Compatibility matrix, testing methodology

- [terminfo.dev synchronized output](https://terminfo.dev/modes/decset-2026-synchronized-output) -

  DECSET 2026 spec, terminal support matrix

- [contour-terminal synchronized output spec](https://github.com/contour-terminal/vt-extensions/blob/master/synchronized-output.md) -

  Detailed spec document

- [terminfo.dev SGR mouse](https://terminfo.dev/modes/decset-1006-sgr-mouse) - SGR mouse mode spec,

  terminal support

- [terminfo.dev multiplexers](https://terminfo.dev/multiplexers) - tmux/screen pass-through problem,

  feature casualties

- [unicode-width crate](https://docs.rs/unicode-width/latest/unicode_width/) - UAX #11 width

  calculation for Rust

- [ansi_colours crate](https://crates.io/crates/ansi_colours) - RGB to ANSI 256 color mapping
- [termcolor (BurntSushi)](https://github.com/BurntSushi/termcolor) - Color output abstraction with

  NoColor/Ansi implementations

- [termprofile crate](https://docs.rs/termprofile/latest/termprofile/) - Terminal color capability

  detection via DECRQSS

### Dropped

- r3bl_ansi_color, rustyhues, terminal_style - Small wrapper crates, no architectural insight
- runefix-core - New/unproven crate, unicode-width is the standard
- Various duplicate notcurses/vt-extensions URLs - Same content at different commit SHAs
- DevPod SSH rendering issue - Bug report, not architectural reference
- microsoft/terminal slow rendering issue - Performance bug report, not design reference

## Gaps

1. **Sixel and Kitty graphics protocol integration**: Not covered in depth. Relevant if the backend

   wants to support inline images or tile rendering. Would need a separate research pass.

1. **terminfo/termcap database usage**: Crossterm mostly hardcodes ANSI sequences rather than

   querying terminfo. Whether to use terminfo for capability-dependent sequence selection (like
   notcurses does) is an architectural decision not fully explored.

1. **Benchmarks**: No concrete performance numbers for cell-diffing overhead or escape sequence

   throughput over SSH. Would need empirical testing.

1. **Windows Console API**: Crossterm's Windows backend uses the Console API instead of ANSI

   sequences on older Windows. Not relevant for a Unix-focused ANSI backend but worth noting for
   portability.

1. **Bracketed paste mode**: Not covered. `\x1b[?2004h` enables it; pasted text is wrapped in

   `\x1b[200~`...`\x1b[201~`. Important for text input fields.

1. **Focus events**: `\x1b[?1004h` enables focus in/out reporting. Useful for pausing rendering when

   unfocused.
