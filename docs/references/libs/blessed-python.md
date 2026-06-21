# Research: blessed (Python)

## Summary

Blessed is a Python terminal abstraction library (MIT, successor to blessings) that wraps terminfo into a single `Terminal` class, providing color/style output, keyboard input decoding, cursor positioning, and Unicode-aware string measurement. It is widely used (~19M monthly PyPI downloads) as a low-level building block; higher-level TUI frameworks like Enlighten and Dashing build on top of it. It operates on real terminals (not a pseudo-terminal window like BearLibTerminal), which makes it portable but subject to terminal emulator inconsistencies.

## Language

Python (3.7+). Works on Windows, macOS, Linux, and BSD.

## What it does

Blessed exposes one class, `Terminal`, which handles:

- **Color and style**: 16, 256, and 24-bit (true color) support. Colors can be referenced by X11 name (`term.red`, `term.darkkhaki`), RGB value (`term.color_rgb()`), or number. Compound formatting like `term.bold_red_on_white()` composes naturally. Styles degrade gracefully when piped to non-TTY outputs (sequences are stripped, text preserved).
- **Keyboard input**: `term.inkey()` in `cbreak()` or `raw()` context managers returns `Keystroke` objects with `.name`, `.code`, and the raw value. Handles multi-byte sequences, arrow keys, function keys, and the Kitty keyboard protocol.
- **Cursor and screen positioning**: `term.location(x, y)` context manager, `term.move_xy()`, `term.home`, `term.clear`, `term.fullscreen()`, `term.hidden_cursor()`, scroll regions.
- **Unicode width measurement**: Uses the author's own `wcwidth` library (also by jquast) to correctly measure CJK wide characters, emoji, and other multi-width codepoints. `term.length()`, `term.center()`, `term.wrap()`, and `term.truncate()` are all sequence-aware, meaning they ignore escape codes when computing display width.
- **Modern terminal features**: Sixel graphics, OSC 52 clipboard, hyperlinks (`term.link()`), OSC progress bar, kitty text sizing protocol, in-band resize notifications, color scheme detection.

## Tools and apps built with it

1. **Voltron** (github.com/snare/voltron) - extensible debugger UI toolkit for GDB/LLDB/VDB/WinDbg. Uses blessed for terminal rendering of register views, disassembly, memory dumps.
2. **cursewords** (github.com/thisisparker/cursewords) - terminal crossword puzzle solver with full keyboard navigation.
3. **Dashing** (github.com/FedericoCeratto/dashing) - library for building terminal dashboards with charts and gauges.
4. **Enlighten** (github.com/Rockhopper-Technologies/enlighten) - multi-progress-bar library that allows simultaneous output without redirection, built on blessed.
5. **Various bundled examples** - snake game (worms.py), cellular automata browser (cellestial.py), text editor (editor.py), mouse paint (mouse_paint.py), plasma demoscene effect (plasma.py).

## What it does well

1. **API simplicity** - One import, one class. The `Terminal` object gives you everything. Code that would take 20+ lines of raw curses/terminfo collapses to 2-3 lines. The before/after comparison in the docs is stark.
2. **Pipe-safe output** - Automatically strips escape sequences when stdout is redirected to a file or pipe. No branching logic needed in user code.
3. **Unicode measurement** - The `wcwidth` integration means `term.length()`, `term.center()`, `term.wrap()` all handle CJK, emoji, and ZWJ sequences correctly. Most terminal libraries get this wrong. The author maintains both blessed and wcwidth, so they stay in sync.
4. **Cross-platform** - Windows support (via the `jinxed` library, a pure-Python terminfo replacement using ctypes), macOS, Linux, BSD. Windows support added Dec 2019.
5. **Composable with other code** - Non-exclusive access to the terminal. You can mix blessed calls with raw curses, print statements, or any other output library. No global state takeover.
6. **Modern protocol support** - Kitty keyboard protocol, kitty text sizing, sixel graphics, OSC 52 clipboard, hyperlinks. Actively maintained (v1.44.0, last release days ago as of this research).
7. **Context managers for terminal modes** - `fullscreen()`, `hidden_cursor()`, `cbreak()`, `raw()` all restore terminal state on exit, even on exceptions. No more corrupted shells.
8. **Massive adoption** - ~19M monthly PyPI downloads, indicating heavy use as a dependency (Enlighten alone pulls it in for many projects).

## Where it falls short

1. **No widget system** - Blessed is a low-level library, not a framework. No buttons, text inputs, scroll views, layouts. If you want those, you need Textual, urwid, or npyscreen. You build your own rendering loop.
2. **No declarative UI or reactive model** - Unlike Textual (which has CSS-like styling, widget trees, and an event system), blessed gives you positioned print calls. Complex UIs require manual state management.
3. **Terminal emulator inconsistencies** - Because it targets real terminals via terminfo, behavior varies across emulators. Unicode width measurement is "very accurate" for most popular terminals but not all. Emoji rendering is especially inconsistent across terminals.
4. **PyInstaller packaging issues** - Known problem where `setupterm(kind='vtwin10')` fails when bundled with PyInstaller because the terminfo data files aren't included. Requires manual workarounds.
5. **No built-in async support** - `term.inkey()` is blocking (with optional timeout). No native asyncio integration for event loops. You need threads or polling for concurrent I/O.
6. **Key detection ambiguity** - As noted in a comparison with Curtsies, blessed's key detection can miss some edge cases with certain escape sequences. The Kitty keyboard protocol support mitigates this for modern terminals, but legacy terminals still rely on heuristic sequence parsing.
7. **Scale ceiling** - For large, complex TUI applications, the lack of a rendering engine (diffing, dirty rectangles, virtual screen buffer) means you're doing full redraws or manual partial updates. Textual's rendering is reportedly 5-10x faster for complex UIs.

## Comparison: blessed vs BearLibTerminal

| Aspect | blessed | BearLibTerminal |
|--------|---------|-----------------|
| **Approach** | Real terminal (stdout + terminfo) | Pseudo-terminal (opens its own window via SDL/OpenGL) |
| **Rendering surface** | User's actual terminal emulator | Custom window with a character cell grid |
| **Font control** | None (uses terminal's font) | Full control: bitmap fonts, TrueType, tilesets, multiple layers |
| **Color** | Depends on terminal (16/256/24-bit) | Full RGBA with alpha blending, always available |
| **Unicode** | Via wcwidth; rendering depends on terminal | Built-in UTF-8/UTF-16/UTF-32; rendering depends on loaded font |
| **Input** | Terminal escape sequences (with ambiguity) | Direct window events (unambiguous) |
| **Portability** | Runs in any terminal (SSH, tmux, etc.) | Requires a graphical display (X11/Wayland/Windows desktop) |
| **Use case** | CLI tools, server apps, remote sessions | Roguelike games, graphical-feeling terminal apps |
| **Maintenance** | Active (v1.44.0, June 2025) | Dormant (last PyPI release 2017, forks exist) |
| **Dependencies** | wcwidth, jinxed (Windows only) | SDL2/OpenGL native library |

The fundamental distinction: blessed works *inside* existing terminals, inheriting their capabilities and limitations. BearLibTerminal creates its own terminal-like window, giving total control over rendering but losing the ability to run in SSH sessions, tmux, or headless environments. For a roguelike-style game that only runs locally with a display server, BearLibTerminal provides more consistent visual output. For anything that needs to work in a real terminal, blessed is the correct choice.

## Sources

- Kept: [blessed GitHub repo](https://github.com/jquast/blessed) - primary source, feature list, 3rd-party examples
- Kept: [blessed docs - Introduction](https://blessed.readthedocs.io/en/latest/intro.html) - API overview, before/after comparison
- Kept: [blessed docs - Examples](https://blessed.readthedocs.io/en/latest/examples.html) - bundled example programs
- Kept: [blessed docs - Sizing & Alignment](https://blessed.readthedocs.io/en/latest/measuring.html) - Unicode measurement details
- Kept: [PyPI - blessed v1.44.0](https://pypi.org/project/blessed/) - version, metadata, dependencies
- Kept: [PyPI Stats](https://pypistats.org/packages/blessed) - download numbers (~19M/month)
- Kept: [wcwidth GitHub](https://github.com/jquast/wcwidth) - Unicode width library (same author)
- Kept: [BearLibTerminal GitHub fork](https://github.com/Andres6936/BearLibTerminal) - feature comparison
- Kept: [blessed vs textual - LibHunt](https://www.libhunt.com/compare-jquast--blessed-vs-textual) - stars/activity comparison
- Kept: [Key detection comparison (Curtsies vs Blessed)](https://ballingt.com/key-detection-code) - keyboard handling analysis
- Dropped: Generalist Programmer guide - rehashed docs with no original content
- Dropped: Botmonster Textual/Rich article - about Textual, not blessed
- Dropped: Ubuntu packaging page - just metadata

## Gaps

- **Blessed as a game engine foundation**: no benchmarks on frame rate or throughput for fullscreen redraw-heavy applications. The bundled examples (bounce.py, worms.py) are simple demos, not stress tests.
- **Comparison with Rich**: Rich (by Will McGugan) handles similar color/style output but with a different API model (renderables, console protocol). A direct API comparison would clarify when to use which.
- **Thread safety**: the docs don't address concurrent writes to a Terminal object from multiple threads. Unclear if this is safe or requires locking.
- **Accessibility**: no information found on screen reader compatibility or accessibility features.
