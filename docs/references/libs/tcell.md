# Research: tcell (Go Terminal Library)

## Summary

tcell is a pure-Go, cell-based terminal abstraction library (similar in concept to BearLibTerminal
but targeting real terminals). It sits at a low level, providing the plumbing (screen buffer, input
events, color, Unicode) that higher-level frameworks like tview build on. It is the dominant
low-level terminal library in the Go ecosystem, powering major tools like fzf, micro, lf, termshark,
aerc, and browsh.

## Language

Go. Pure Go with no CGO dependency. Supports Linux, macOS, FreeBSD, Solaris, Windows (10+), Plan 9,
and WebAssembly.

## Key Features and Strengths

1. **Rich Unicode and grapheme cluster support** -- tcell works internally with UTF-8 and handles

   wide characters, combining characters, and multi-rune grapheme clusters. The v3 `Put()` API
   accepts a full string and returns the displayed width, making CJK and emoji handling
   straightforward. It also converts to/from non-Unicode locales automatically.
   [README](https://github.com/gdamore/tcell)

1. **Comprehensive input handling** -- Supports keyboard events (including modern protocols like

   Kitty keyboard protocol and XTerm CSI-u for distinguishing e.g. Ctrl-I vs Tab), mouse tracking
   (buttons, motion, wheel, click-drag), and bracketed paste. Environment variable overrides
   (`TCELL_KEYBOARD_PROTOCOL`, `TCELL_MOUSE`) let users recover from bad terminal behavior without
   app changes. [README](https://github.com/gdamore/tcell),
   [Issue #671](https://github.com/gdamore/tcell/issues/671)

1. **True cross-platform support** -- Pure Go, no CGO. Runs on all mainstream Go platforms. Windows

   support uses modern VT mode (Windows 10 1703+). WASM support exists for running tcell apps in a
   browser. CI tests run on Linux, macOS, Windows, and WebAssembly.
   [README](https://github.com/gdamore/tcell)

1. **24-bit color** -- Supports true color via `COLORTERM=truecolor`, `-truecolor`/`-direct` TERM

   suffixes, or auto-detection. Gracefully degrades to 256-color or 8-color ANSI palettes on older
   terminals. [README](https://github.com/gdamore/tcell)

1. **Clean, minimal API (v3)** -- The v3 API is notably streamlined. Drawing uses `Put()`,

   `PutStr()`, `PutStrStyled()`. Events come through a standard Go channel (`EventQ()`), which
   integrates naturally with `select` statements. Styles are composed with a builder pattern
   (`StyleDefault.Foreground(...).Background(...)`). The `Color` type was shrunk to 32 bits for
   memory efficiency. [CHANGESv3](https://github.com/gdamore/tcell/blob/main/CHANGESv3.md),
   [TUTORIAL](https://github.com/gdamore/tcell/blob/main/TUTORIAL.md)

1. **v3 dropped terminfo entirely** -- Instead of relying on terminfo databases (which were

   unreliable for modern features like 24-bit color, styled underlines, and advanced mouse modes),
   v3 classifies terminals into a few capability classes and negotiates features at startup. This
   simplified the codebase and improved support for modern terminal emulators.
   [CHANGESv3](https://github.com/gdamore/tcell/blob/main/CHANGESv3.md)

1. **Performance-conscious rendering** -- Minimizes data sent to the terminal by avoiding redundant

   sequences and skipping unchanged cells on refresh. [README](https://github.com/gdamore/tcell)

## Notable Projects Built With tcell

### UI Frameworks

- **[tview](https://github.com/rivo/tview)** -- Rich widget toolkit (tables, forms, trees, lists,

  flexbox layout). The most popular higher-level framework built on tcell.

- **[cview](https://code.rocketnine.space/tslocum/cview)** -- Fork of tview with additional

  features.

- **[gowid](https://github.com/gcla/gowid)** -- Compositional terminal widgets inspired by Python's

  urwid.

- **[awesome-gocui](https://github.com/awesome-gocui/gocui)** -- Console UI library.
- **[gruid-tcell](https://github.com/anaseto/gruid-tcell)** -- tcell driver for the gruid grid-based

  UI/game framework.

### Major Tools

- **[fzf](https://github.com/junegunn/fzf)** -- The ubiquitous command-line fuzzy finder.
- **[micro](https://github.com/zyedidia/micro)** -- Terminal text editor with syntax highlighting.
- **[lf](https://github.com/gokcehan/lf)** -- Terminal file manager.
- **[aerc](https://git.sr.ht/~sircmpwn/aerc)** -- Terminal email client.
- **[termshark](https://termshark.io)** -- Terminal Wireshark interface (built on gowid/tcell).
- **[browsh](https://github.com/browsh-org/browsh)** -- Web browser that renders in the terminal.
- **[WTF](https://github.com/senorprogrammer/wtf)** -- Personal information dashboard.
- **[aretext](https://github.com/aretext/aretext)** -- Minimalist vim-like text editor.
- **[ov](https://github.com/noborus/ov)** -- Terminal pager.

### Games

- **[proxima5](https://github.com/gdamore/proxima5)** -- Space shooter by tcell's author.
- **[uchess](https://github.com/tmountain/uchess)** -- UCI chess client.
- **[go-tetris](https://github.com/aaronriekenberg/go-tetris)** -- Tetris (native + WASM).
- **[hero.go](https://github.com/barisbll/hero.go)** -- 2D monster shooter.
- **[go-life](https://github.com/sachaos/go-life)** -- Conway's Game of Life.

[Full gallery](https://github.com/gdamore/tcell/wiki/Gallery)

## Weaknesses and Limitations

1. **Low-level by design, no widgets** -- tcell is intentionally a terminal abstraction, not a UI

   framework. You get cells, colors, and events. Building anything resembling a text input,
   scrollable list, or layout system requires either a framework like tview or rolling your own. One
   developer noted that all events and draw calls are globally scoped, making it "messy quickly" to
   implement context-dependent behavior.
   [justindev](https://justindev.io/building-tuis-with-go.html)

1. **Screen flicker on resize** -- Multiple GitHub issues report intermittent flicker during

   redraws, particularly on terminal resize. Synchronized output (DCS sequences) was requested to
   address this but full support varies by terminal.
   [Issue #797](https://github.com/gdamore/tcell/issues/797),
   [Issue #576](https://github.com/gdamore/tcell/issues/576)

1. **Wide character edge cases** -- Full-width characters at screen boundaries can cause rendering

   glitches. A workaround for one wide-char bug (#988) introduced another where full-width chars in
   the second-to-last column disappear. [Issue #1008](https://github.com/gdamore/tcell/issues/1008)

1. **Multi-rune emoji rendering inconsistencies** -- Overwriting single-width characters with

   multi-rune emoji produces incorrect rendering on some terminals (iTerm2, Alacritty) while working
   fine on others (Ghostty, Kitty). This is partly a terminal emulator problem, but tcell cannot
   fully paper over it. [Issue #976](https://github.com/gdamore/tcell/issues/976)

1. **No key release events** -- Terminal limitations mean tcell can only report key press (and

   repeat) events. Key release is unavailable, which limits real-time game input compared to
   libraries like BearLibTerminal that can detect key-up.
   [TUTORIAL](https://github.com/gdamore/tcell/blob/main/TUTORIAL.md)

1. **Terminal inconsistency is inherent** -- Text styling, clipboard (OSC 52), and capability

   negotiation behave differently across terminal emulators. tcell provides env-var overrides as
   escape hatches, but users still hit issues where their specific terminal misreports capabilities.
   [Issue #539](https://github.com/gdamore/tcell/issues/539),
   [Issue #926](https://github.com/gdamore/tcell/issues/926)

1. **Smaller community than Bubble Tea** -- tcell has ~5.2k GitHub stars vs Bubble Tea's ~43k.

   Bubble Tea's Elm architecture and pre-built component ecosystem (Bubbles, Lip Gloss) attract more
   newcomers. tcell's documentation, while functional, is sparser.
   [LibHunt comparison](https://www.libhunt.com/compare-tcell-vs-bubbletea)

1. **v3 breaking changes** -- The v2-to-v3 migration requires updating every app (SetCell -> Put,

   PollEvent -> EventQ channel, Rune() -> Str(), terminfo removal, etc.). While changes are mostly
   mechanical, the ecosystem is still transitioning.
   [CHANGESv3](https://github.com/gdamore/tcell/blob/main/CHANGESv3.md)

## Comparison to BearLibTerminal (context for roguelike development)

| Aspect           | tcell                                                                                 | BearLibTerminal                                        |
| ---------------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| Target           | Real terminals (xterm, iTerm, Windows Terminal)                                       | Virtual window (SDL/OpenGL)                            |
| Language         | Go (pure)                                                                             | C with bindings for many languages                     |
| Rendering        | Cell-based, limited to terminal capabilities                                          | Cell-based with tile layers, custom fonts, compositing |
| Input            | Key press only (no key release), mouse                                                | Key press + release, mouse                             |
| Unicode          | Full grapheme cluster support                                                         | Basic Unicode, tile-based rendering                    |
| Color            | Up to 24-bit true color (terminal dependent)                                          | Full 32-bit RGBA                                       |
| Distribution     | Runs in any terminal, no window needed                                                | Requires graphical environment                         |
| Game suitability | Good for roguelikes that target terminal; limited by terminal refresh and input model | Purpose-built for roguelikes with richer rendering     |

## Sources

- Kept: [gdamore/tcell README](https://github.com/gdamore/tcell) -- primary source, comprehensive

  feature list

- Kept: [tcell TUTORIAL.md](https://github.com/gdamore/tcell/blob/main/TUTORIAL.md) -- API usage

  patterns, event model, limitations

- Kept: [CHANGESv3.md](https://github.com/gdamore/tcell/blob/main/CHANGESv3.md) -- v3 API evolution,

  design decisions

- Kept: [tcell Wiki Gallery](https://github.com/gdamore/tcell/wiki/Gallery) -- complete project

  listing

- Kept: [justindev - Building TUIs with Go](https://justindev.io/building-tuis-with-go.html) --

  first-hand developer experience choosing tcell over Bubble Tea

- Kept: [LibHunt tcell vs bubbletea](https://www.libhunt.com/compare-tcell-vs-bubbletea) --

  community size comparison

- Kept: GitHub Issues [#797](https://github.com/gdamore/tcell/issues/797),

  [#976](https://github.com/gdamore/tcell/issues/976),
  [#1008](https://github.com/gdamore/tcell/issues/1008),
  [#576](https://github.com/gdamore/tcell/issues/576) -- real-world rendering bugs

- Dropped: Applied Go TUI overview -- outdated, surface-level comparison
- Dropped: StackOverflow tcell question -- basic beginner question, no insight
- Dropped: Earthly Blog TUI tutorial -- generic tutorial, not tcell-specific depth
- Dropped: ConfigCrate bubbletea article -- bubbletea-focused, not tcell

## Gaps

- **Benchmarks**: No formal performance benchmarks comparing tcell's rendering throughput to

  alternatives (Bubble Tea's cell renderer, notcurses, etc.) were found.

- **v3 adoption rate**: Unclear how many major projects (fzf, micro, lf) have migrated from v2 to

  v3.

- **Sixel / image protocol support**: tcell does not appear to support Sixel graphics or Kitty image

  protocol. This matters for roguelikes wanting tile rendering in-terminal.

- **Thread safety details**: The tutorial and docs don't deeply document thread safety guarantees

  beyond the event channel pattern.
