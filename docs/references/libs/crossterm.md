# Reference: crossterm

- **Language:** Rust (pure Rust, no C/FFI dependencies on Unix beyond libc)
- **Repository:** <https://github.com/crossterm-rs/crossterm>
- **License:** MIT
- **Latest stable:** 0.28.1 (mid-2024); 0.29 exists but versioning on crates.io shows ~1 year since
  last publish
- **Operates on:** Real terminal emulators (not pseudo-terminal windows). Writes ANSI escape
  sequences (or WinAPI calls on older Windows) to stdout/stderr.

## What it is

Crossterm is a cross-platform terminal manipulation library for Rust. It provides a unified API for
cursor movement, styled output, terminal control (alternate screen, raw mode, scrolling, sizing),
and event reading (keyboard, mouse, resize, focus, paste). It targets real terminal emulators on
Windows (down to Windows 7 via WinAPI fallback), Linux, macOS, and other Unix systems.

It is the dominant backend for Rust TUI frameworks. Ratatui defaults to crossterm, and most Rust TUI
applications use it directly or indirectly.

## Notable projects built with it

Crossterm is foundational infrastructure. Hundreds of Rust TUI applications depend on it, either
directly or through Ratatui.

**Frameworks using crossterm as a backend:**

- [Ratatui](https://github.com/ratatui/ratatui) -- the primary Rust TUI framework (fork of tui-rs).
  Crossterm is its default backend.
- [Cursive](https://github.com/gyscos/Cursive) -- ncurses-style TUI framework with a crossterm
  backend option.

**Tools (direct or via Ratatui):**

- [Broot](https://dystroy.org/broot/) -- file manager/navigator
- [gitui](https://github.com/extrawurst/gitui) -- Git TUI
- [Yazi](https://github.com/sxyazi/yazi) -- async terminal file manager
- [bottom](https://github.com/ClementTsang/bottom) -- system monitor
- [spotify-player](https://github.com/aome510/spotify-player) -- Spotify TUI client
- [trippy](https://github.com/fujiapple852/trippy) -- network diagnostic tool
- [bandwhich](https://github.com/imsnif/bandwhich) -- network utilization by process
- [dua](https://github.com/Byron/dua-cli) -- disk usage analyzer
- [lazyjj](https://github.com/Cretezy/lazyjj) -- Jujutsu VCS TUI
- [serie](https://github.com/lusingander/serie) -- Git commit graph visualizer
- [termscp](https://github.com/veeso/termscp) -- SCP/SFTP/FTP terminal file transfer

**Games (direct or via Ratatui):**

- [Rusty-rain](https://github.com/cowboy8625/rusty-rain) -- Matrix rain effect
- [Chess-tui](https://github.com/thomas-mauran/chess-tui) -- terminal chess
- [Battleship.rs](https://github.com/deepu105/battleship-rs) -- terminal Battleship
- [plastic](https://github.com/Amjad50/plastic) -- NES emulator with Ratatui UI
- [Maze TUI](https://github.com/agl-alexglopez/maze-tui) -- maze algorithm visualizations

The [awesome-ratatui](https://github.com/ratatui/awesome-ratatui) list catalogs 200+ applications,
all of which depend on crossterm transitively.

## Strengths

### 1. True cross-platform terminal abstraction

Supports Windows (Console Host, Windows Terminal), Linux, macOS, and BSDs from a single API. On
Windows 10+ it uses ANSI escape codes; on older Windows it falls back to WinAPI calls. This is
crossterm's primary differentiator over termion (Unix-only).
[GitHub README](https://github.com/crossterm-rs/crossterm)

### 2. Command-based API with flush control

The `Command` trait and `queue!`/`execute!` macros give fine-grained control over when bytes are
flushed to the terminal. `queue!` batches commands for a single flush (better performance for
full-screen redraws), while `execute!` flushes immediately. Commands work on any `impl Write`, so
you can target stdout, stderr, or a buffer.
[docs.rs](https://docs.rs/crossterm/latest/crossterm/index.html)

### 3. Synchronized output support

Crossterm implements the `SynchronizedUpdate` trait (added in v0.26.1), which wraps output in DCS
sequences that tell the terminal to hold rendering until the update is complete. This eliminates
flicker/tearing on terminals that support the protocol (most modern ones do).
[Release notes](https://github.com/crossterm-rs/crossterm/releases)

### 4. Comprehensive input handling

Supports keyboard events (key press, release, repeat), mouse events (click, drag, scroll, position),
terminal resize events, focus gain/loss events, and bracketed paste. Offers both blocking `read()`
and non-blocking `poll()` + `read()`, plus an async `EventStream` (via the `event-stream` feature
flag with `futures::Stream`).
[docs.rs event module](https://docs.rs/crossterm/latest/crossterm/event/index.html)

### 5. Kitty keyboard protocol support

Supports the Kitty keyboard enhancement protocol (`PushKeyboardEnhancementFlags`), which provides
disambiguated key events, key release/repeat events, and extended modifier reporting on terminals
that implement it. This is the modern approach to fixing the fundamental ambiguity problems of
legacy terminal key encoding.
[Release notes v0.25](https://github.com/crossterm-rs/crossterm/releases)

### 6. Minimal dependency footprint

Core dependencies are just `bitflags` and `parking_lot`. Event handling adds `mio`, `signal-hook`,
and `libc` on Unix, `winapi` on Windows. The `events` feature can be disabled entirely for a very
thin layer. Dependencies are well-justified in the README.
[GitHub README](https://github.com/crossterm-rs/crossterm)

### 7. Pure Rust

No C library bindings (unlike ncurses-based approaches). This simplifies cross-compilation and
avoids system library version issues.

### 8. Thread-safe

All types are `Send + Sync`. This matters for applications that want to handle input on one thread
and render on another.

## Weaknesses and limitations

### 1. Modifier key reporting is fragmented across terminals

This is crossterm's most persistent pain point. Modifier keys (Ctrl, Shift, Alt) combined with
certain keys (Enter, Backspace, brackets) produce inconsistent or missing modifier flags depending
on the terminal emulator. A long-standing tracking issue (#685) lists dozens of broken combinations.
The Kitty keyboard protocol fixes this, but only for terminals that implement it. On legacy
terminals, `Ctrl+Enter` and `Shift+Enter` are indistinguishable from plain `Enter` in many
emulators. [GitHub #685](https://github.com/crossterm-rs/crossterm/issues/685)

### 2. Event system is all-or-nothing

Crossterm's event poll/read captures all event types. There is no built-in way to subscribe to only
specific events (e.g., only resize events while letting another mechanism handle keyboard input).
Issue #967 describes the problem: if you want to use crossterm only for resize detection while
reading stdin directly for input, the event loop interferes. Filtering support exists internally but
is not exposed. [GitHub #967](https://github.com/crossterm-rs/crossterm/issues/967)

### 3. No built-in signal handling (SIGTSTP, SIGINT, SIGTERM)

Crossterm does not handle Unix job control signals. `Ctrl+Z` (SIGTSTP) in raw mode leaves the
terminal in a broken state because crossterm doesn't restore terminal settings before suspending.
Users must bring in additional crates (`signal-hook`, `ctrlc`) and wire up their own handlers. Issue

# 494 from dua's maintainer (Byron) documents this well. Issue #554 requested adding signal events to

the event loop. Neither has been resolved.
[GitHub #494](https://github.com/crossterm-rs/crossterm/issues/494),
[GitHub #554](https://github.com/crossterm-rs/crossterm/issues/554)

### 4. No cell-based rendering abstraction

Crossterm operates at the escape-sequence level: move cursor, set color, print text. It has no
concept of a screen buffer, cells, or diffing. If you want efficient full-screen rendering (only
redraw changed cells), you need a higher-level library like Ratatui on top. This is by design, but
it means crossterm alone is not sufficient for a game or complex TUI without significant
boilerplate.

### 5. Windows WinAPI edge cases can panic

On Windows Terminal, certain mouse event flags could cause panics in `crossterm_winapi` (issue

# 588). While specific bugs get fixed, the WinAPI codepath is less battle-tested than the ANSI

codepath and has historically been the source of crashes rather than graceful error returns.
[GitHub #588](https://github.com/crossterm-rs/crossterm/issues/588)

### 6. Release cadence has slowed

70 versions published since 2018, but the last stable release (0.28.1) was about a year ago as of
mid-2025. The project is maintained primarily by one person (Timon Post). Active development
continues, but the pace of releases has slowed compared to 2019-2022. Open issues number 100+.
[crates.io](https://crates.io/crates/crossterm/versions)

### 7. Windows dependencies leak into non-Windows Cargo.lock

Due to how Cargo resolves dependencies, Windows-specific crates (winapi, crossterm_winapi) appear in
`Cargo.lock` even on Linux builds. This causes problems for embedded/Yocto build systems. A
`windows` feature flag was added in v0.28 to allow disabling this, but it's enabled by default.
[GitHub #766](https://github.com/crossterm-rs/crossterm/issues/766)

### 8. No pixel-level or graphical rendering

Crossterm works with character cells. It does not support Sixel graphics, Kitty image protocol, or
any sub-cell rendering. For image display in terminals, separate crates like `ratatui-image` are
needed.

## How it handles key areas

### Terminal rendering

Crossterm writes ANSI escape sequences (or WinAPI calls) to a `Write` target. It provides commands
for cursor positioning, text styling (16/256/RGB colors, bold, italic, underline, etc.), screen
clearing, scrolling, and alternate screen switching. The `queue!` macro batches multiple commands
into a single buffer write, and `flush()` sends them all at once. Synchronized output (DCS
sequences) prevents flicker. There is no built-in double-buffering or cell diffing; that's left to
frameworks like Ratatui.

### Input handling

Uses `mio` for event-readiness polling on Unix, WinAPI `ReadConsoleInput` on Windows. Provides
`poll(Duration)` for non-blocking checks and `read()` for blocking reads. The `EventStream` feature
provides an async `futures::Stream<Item = Result<Event>>`. Events include key presses (with modifier
detection), mouse actions (press, release, move, drag, scroll with position), terminal resize, focus
changes, and pasted text (bracketed paste). Key events carry `KeyCode`, `KeyModifiers`, and
optionally `KeyEventKind` (press/release/repeat) when the Kitty protocol is active.

### Cross-platform support

This is crossterm's core value proposition. The same Rust code runs on:

- **Windows 10+**: ANSI escape codes (enables virtual terminal processing)
- **Windows 7-8.1**: WinAPI calls (Console Host API)
- **Linux/macOS/BSD**: Standard ANSI escape codes via termios
- **Tested on**: Windows Terminal, Console Host, Ubuntu Terminal, Konsole, Kitty, Alacritty, macOS
  Terminal, iTerm2 (implied by issue reports)

Platform differences are abstracted behind the `Command` trait. The trade-off: some features (e.g.,
256/RGB color) only work on modern terminals, and modifier key behavior varies by terminal emulator
rather than by OS.

## Comparison to alternatives

| Feature                 | crossterm            | termion          | termwiz          |
| ----------------------- | -------------------- | ---------------- | ---------------- |
| Windows support         | Yes (WinAPI + ANSI)  | No (Unix only)   | Yes              |
| Async event stream      | Yes (`event-stream`) | No               | Yes              |
| Kitty keyboard protocol | Yes                  | No               | Yes              |
| Maintainer              | Timon Post           | Redox OS team    | Wez Furlong      |
| Used by Ratatui         | Default backend      | Optional backend | Optional backend |
| Cell buffer / diffing   | No                   | No               | Yes (Surface)    |
| Pure Rust               | Yes                  | Yes              | Yes              |

Crossterm won the ecosystem race primarily because of Windows support and its adoption as Ratatui's
default backend. termion is simpler but Unix-only. termwiz (from WezTerm's author) has a
higher-level surface abstraction but a smaller user base.

## Relevance as a reference

For any project building terminal abstractions in Rust, crossterm is the standard reference for:

- How to structure a command/escape-sequence API (`Command` trait pattern)
- Cross-platform terminal I/O abstraction (ANSI vs WinAPI)
- Event parsing from raw terminal byte streams (the `parse.rs` module)
- Kitty keyboard protocol integration
- Synchronized output for flicker-free rendering

It intentionally stops short of cell-based rendering, leaving that to higher-level crates. This
makes it a good study of the "thin abstraction" approach, where the library handles terminal I/O
primitives and leaves screen management to the application or framework layer.

## Sources

- [GitHub README](https://github.com/crossterm-rs/crossterm) -- feature list, tested terminals,
  dependency justification, "used by" section
- [docs.rs API documentation](https://docs.rs/crossterm/latest/crossterm/) -- command API, event
  module, feature flags
- [GitHub Issues #685](https://github.com/crossterm-rs/crossterm/issues/685) -- modifier key
  tracking issue, documents terminal fragmentation
- [GitHub Issues #967](https://github.com/crossterm-rs/crossterm/issues/967) -- event filtering
  limitations
- [GitHub Issues #494](https://github.com/crossterm-rs/crossterm/issues/494) -- SIGTSTP/job control
  signal handling gap
- [GitHub Issues #554](https://github.com/crossterm-rs/crossterm/issues/554) -- signal event request
- [GitHub Issues #588](https://github.com/crossterm-rs/crossterm/issues/588) -- Windows Terminal
  mouse event panic
- [GitHub Issues #766](https://github.com/crossterm-rs/crossterm/issues/766) -- Windows dependency
  leaking into Linux builds
- [GitHub Releases](https://github.com/crossterm-rs/crossterm/releases) -- changelog, synchronized
  output, Kitty protocol additions
- [crates.io versions](https://crates.io/crates/crossterm/versions) -- release cadence data
- [awesome-ratatui](https://github.com/ratatui/awesome-ratatui) -- ecosystem catalog of projects
  using crossterm via Ratatui
- [Ratatui README](https://github.com/ratatui/ratatui) -- confirms crossterm as default backend
