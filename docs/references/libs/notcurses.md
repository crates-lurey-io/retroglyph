# Notcurses

A modern, high-performance TUI library for terminal emulators. Written in C with bindings for C++, Python, Rust, Zig, Raku, Ada, Dart, Julia, and Nim. Not a drop-in ncurses replacement; it abandons the X/Open Curses API entirely and builds something more capable from scratch. Apache-2.0 licensed. ~5K GitHub stars.

- **Repo**: https://github.com/dankamongmen/notcurses
- **Author**: Nick Black (dankamongmen)
- **Docs**: https://notcurses.com (man pages), [dankwiki](https://nick-black.com/dankwiki/index.php?title=Notcurses)
- **Book**: [Hacking the Planet! with Notcurses](https://nick-black.com/htp-notcurses.pdf) (free PDF)
- **Current version**: 3.0.17

## Language and Bindings

Core library is **C** (C17), with C++-safe headers. Requires `libtinfo` from ncurses for terminfo but does not use ncurses itself.

| Language | Maintainer | Location |
|----------|-----------|----------|
| C++ | Marek Habersack, Nick Black | in-tree |
| Python | Nick Black, igo95862 | in-tree |
| Rust | Jose Luis Cruz | [libnotcurses-sys](https://github.com/dankamongmen/libnotcurses-sys) |
| Zig | Jakub Dundalek | [notcurses-zig-example](https://github.com/dundalek/notcurses-zig-example) |
| Raku | Matt Doughty | [Notcurses-Native](https://github.com/m-doughty/Notcurses-Native) |
| Ada | Jeremy Grosser | [notcursesada](https://github.com/JeremyGrosser/notcursesada) |
| Dart | Nelson Fernandez | [dart_notcurses](https://github.com/kascote/dart_notcurses) |
| Julia | Dheepak Krishnamurthy | [Notcurses.jl](https://github.com/kdheepak/Notcurses.jl) |
| Nim | Michael S. Bradley, Jr. | [nim-notcurses](https://github.com/michaelsbradleyjr/nim-notcurses) |

The library can be built without multimedia (`-DUSE_MULTIMEDIA=none`) and without C++ (`-DUSE_CXX=off`). A `notcurses-core` variant links without FFmpeg/OIIO dependencies.

## Architecture and Core Concepts

### The Cell Model (nccell)

The fundamental unit is `nccell`, a 16-byte struct:

```c
typedef struct nccell {
  uint32_t gcluster;          // 4B: EGC (inline if <=4 UTF-8 bytes, else pool index)
  uint8_t  gcluster_backstop; // 1B: always zero (NUL terminator for inline EGCs)
  uint8_t  width;             // 1B: column width of the EGC
  uint16_t stylemask;         // 2B: NCSTYLE_* attributes (bold, italic, etc.)
  uint64_t channels;          // 8B: fg/bg color, alpha, palette index, default flags
} nccell;
```

Each cell holds exactly one Extended Grapheme Cluster (EGC). If the UTF-8 encoding fits in 4 bytes (covers all of Unicode 13), it's stored inline in `gcluster`. Longer EGCs go into a per-plane `egcpool` (max 16 MiB). This is a key difference from ncurses' `cchar_t`, which uses a fixed-size `wchar_t` array.

The 64-bit `channels` field packs foreground and background into two 32-bit channels, each carrying 24-bit RGB color plus 2-bit alpha (opaque, blend, transparent, high-contrast). No "color pairs" concept exists; all color is direct 24-bit RGB, quantized down at render time for terminals with smaller palettes.

### Planes (ncplane)

All drawing happens on `ncplane` objects: rectangular virtual surfaces with their own framebuffer (matrix of `nccell`s), cursor, geometry, and a base cell used for positions without explicit content. Planes can be any size (larger than the screen is fine; off-screen regions just don't render). There's always a "standard plane" matching the terminal dimensions.

Planes are organized into **piles** (independent rendering contexts). Within a pile, planes have a total z-order. Planes within a pile form a forest (set of DAGs) through parent-child bindings: moving a parent moves its children. Different piles can be rendered/mutated concurrently from different threads.

There is no separate "pad" or "panel" type. The z-buffer is built-in.

### Rendering Pipeline

Rendering is a two-phase process:

1. **Render (compositing)**: Flattens a pile's z-ordered planes into a single matrix of cells. For each screen coordinate, the algorithm walks from the topmost intersecting plane downward, resolving:
   - The first non-empty EGC encountered becomes the glyph.
   - Foreground/background colors are resolved per alpha: opaque locks the color, transparent skips to the next plane, blend averages colors across planes, high-contrast auto-selects a contrasting fg color against the computed bg.
   - The walk stops when both EGC and colors are resolved, or all planes are exhausted.

2. **Rasterize (output)**: Takes the composited matrix, diffs it against the last-rendered state, and produces an optimized stream of UTF-8 characters and escape sequences. Only changed cells emit output. This is the "damage map" approach, introduced in v0.9.0.

`notcurses_render()` does both phases. `ncpile_render()` and `ncpile_rasterize()` can be called separately for finer control (render one pile while rasterizing another). The entire plane stack is locked during frame generation.

### Pixel Graphics (ncvisual)

`ncvisual` provides a virtual pixel framebuffer. Sources: RGBA/BGRA memory, ncplane content, or image/video files (via FFmpeg or OpenImageIO).

Multiple blitters convert pixel data to terminal output:

| Blitter | Resolution | Method |
|---------|-----------|--------|
| `NCBLIT_1x1` | 1x1 per cell | Background color spaces. Only option in ASCII mode. |
| `NCBLIT_2x1` | 2x1 per cell | Half-blocks (default). Best aspect ratio for most content. |
| `NCBLIT_2x2` | 2x2 per cell | Unicode quadrant blocks. Lossy with >2 colors per cell. |
| `NCBLIT_3x2` | 3x2 per cell | Unicode sextants. Highest quality for large images. |
| `NCBLIT_4x2` | 4x2 per cell | Braille patterns. Spotty font support. |
| `NCBLIT_PIXEL` | Native pixels | Sixel, Kitty graphics protocol, or Linux framebuffer. |

Notcurses auto-detects pixel protocol support and degrades gracefully. `NCVISUAL_OPTION_NODEGRADE` forces the requested blitter or fails. Pixel blitting integrates with the plane/cell model, so sprites (transparent pixel graphics over text) work.

Supported pixel protocols:
- **Sixel**: xterm, mlterm, foot, WezTerm, Contour
- **Kitty graphics protocol**: Kitty, WezTerm
- **Linux framebuffer**: Direct memory-mapped writes (not via terminal I/O)
- iTerm2 protocol was **removed** (iTerm2 doesn't support cells + graphics sharing a cell, breaking notcurses' transparency model)

### Unicode and Emoji

- UTF-8 only (or ASCII). No other encodings.
- Full EGC support via `libunistring`. Wide characters tracked correctly; splitting a wide glyph destroys it.
- Right-to-left text: not handled specially by notcurses (terminals apply their own heuristics).
- Drawing characters (box drawing, block elements, Braille) are critical for blitters; many terminals draw these directly rather than from fonts.
- `notcurses-info` lets you visually inspect your terminal's Unicode rendering.

### Input

Input arrives as 32-bit Unicode codepoints. Synthesized events (arrows, function keys, mouse) map into Unicode's Supplementary Private Use Area-B. Supports:
- Kitty keyboard protocol (key release events, modifier-only presses)
- GPM (console mouse)
- XTMODKEYS

No `ESCDELAY`; expects all bytes of an escape sequence at once.

## Included Tools and Demos

Nine executables ship with notcurses:

| Tool | Purpose |
|------|---------|
| `ncls` | `ls` variant that displays multimedia thumbnails inline |
| `ncneofetch` | System info display (neofetch-style) |
| `ncplayer` | Renders images and video to the terminal |
| `nctetris` | Tetris clone |
| `notcurses-demo` | Comprehensive demo of all library capabilities |
| `notcurses-info` | Prints terminal capabilities and Unicode rendering diagnostics |
| `notcurses-input` | Reads and decodes input events |
| `notcurses-tester` | Unit test driver |
| `tfman` | Terminal manual page browser |

The demo includes: animation, box drawing, chunli sprite demo, eagle rendering, fission effects, grid patterns, high-contrast text, jungle scene, Luigi sprite animation, outline rendering, panelreel, QR codes, reel widgets, sliding puzzles, trans flag, uniblock (Unicode coverage), video playback, witherworm, xray effects, yield patterns, and zoo animations.

## Projects Built With Notcurses

- **Selkie** (Raku): High-level retained-mode TUI framework built on Notcurses::Native, with widget tree, reactive store, theming
- **PubSub** (C++17): RSS/Atom/Meshtastic reader/publisher with notcurses TUI
- **tick_trader**: Financial trading terminal with notcurses backend option
- **Memory Arena Visualizer**: Interactive memory allocator visualization tool
- **notcurses-zig-example**: Zig TUI demo
- Various `src/poc/` and `src/pocpp/` proof-of-concept programs in the repo itself

## What It Does Well

1. **Performance**: Damage-map diffing means only changed cells produce output. The internal source comments describe it as "depth buffer blit of updated cells". Benchmarks show Kitty at ~680 FPS for rendering, substantially ahead of other terminals. The library is described (by its author) as "fast as shit."

2. **Pixel graphics integration**: Unlike most TUI libraries, notcurses treats pixel blitting (Sixel, Kitty) as a first-class feature integrated into the plane compositing model. Transparency works across pixel and text layers (sprites). Auto-detection and graceful degradation across blitter levels.

3. **True transparency and alpha blending**: Three-channel transparency (glyph, fg, bg) with four alpha modes (opaque, blend, transparent, high-contrast). High-contrast mode auto-selects readable foreground colors against computed backgrounds. This is unique among terminal libraries.

4. **Thread safety by design**: Concurrent mutation of different planes is always safe. Piles provide independent rendering contexts for true parallel rendering.

5. **Unicode-first cell model**: EGC-based cells with inline storage for common cases. No fixed-size character arrays. Width tracking handles CJK and emoji correctly.

6. **Multimedia**: First-class image and video rendering via FFmpeg/OIIO, with automatic blitter selection based on terminal capabilities.

7. **Terminal capability detection**: Aggressive runtime probing (XTGETTCAP, DA sequences) combined with terminfo. Assumes maximum capabilities and degrades, rather than assuming minimum and building up.

8. **Widgets**: Menus, selectors, multiselectors, tabs, progress bars, plots, readers, reels (panelreel), tree selectors, subproc widgets, and more.

## Where It Falls Short

1. **Terminal compatibility**: Hangs on startup if the terminal doesn't respond to interrogation queries (reported in VSCode terminals over SSH, WSL2). macOS Terminal.app leaks query responses as user input. `screen` doesn't work well. `mosh` looks bad. iTerm2 support was entirely removed due to incompatible graphics model.

2. **Complexity and learning curve**: The API is large and C-level. No higher-level declarative layout system (Selkie exists as a third-party attempt). The guidebook helps but the library demands understanding of planes, cells, channels, piles, and rendering phases.

3. **Static linking issues**: Reported problems with static linking (`pkg-config --static` needed, dependency resolution is fragile).

4. **Limited ecosystem**: Despite many language bindings, adoption is modest. Few large third-party applications compared to ncurses, crossterm, or even tcell. Most examples are the bundled demos.

5. **Not a drop-in replacement**: Porting from ncurses requires understanding architectural differences (no color pairs, no pads, no panels, different input model, different error handling at screen edges).

6. **Screen multiplexer issues**: Looks bad in `mosh`, messy in `screen`, `tmux` consumes bitmaps. This limits usability in many real-world SSH workflows.

7. **Platform gaps**: Windows support exists but requires ConPTY and UTF-8 beta settings. WSL2 has known issues. The library is most at home on Linux with a modern terminal.

8. **Maintenance pace**: Some open bugs (incorrect `O_CLOEXEC` usage, Kitty keyboard cleanup on stop) suggest maintenance is a one-person effort with limited bandwidth.

## Comparison with BearLibTerminal

BearLibTerminal and notcurses solve different problems despite both providing cell grids:

| Aspect | Notcurses | BearLibTerminal |
|--------|-----------|-----------------|
| **Target** | Real terminal emulators (xterm, Kitty, foot, etc.) | Pseudo-terminal window (opens its own OpenGL window) |
| **Rendering** | Terminal escape sequences via stdout | OpenGL-backed custom window |
| **Pixel graphics** | Sixel, Kitty protocol, Linux FB | Bitmap/vector font tilesets natively |
| **Font handling** | Relies on terminal's font config | Loads TrueType fonts and tilesets directly |
| **Use case** | CLI tools, TUI apps, terminal-native software | Roguelikes, tile-based games, self-contained apps |
| **Performance model** | Diff-based terminal output optimization | GPU-accelerated rendering |
| **Unicode** | Full EGC support, width-aware | UTF-8/UTF-16/UTF-32 strings |
| **Platform** | Linux, macOS, FreeBSD, Windows (ConPTY) | Windows, Linux, macOS (via SDL/OpenGL) |
| **Transparency** | Alpha blending through plane z-stack | Tile composition with offsets |
| **Multimedia** | FFmpeg/OIIO video + image | Static tilesets only |
| **Maintenance** | Active (v3.0.17, 2024+) | Unmaintained (last commit ~2018, community forks exist) |

Notcurses is fundamentally a **terminal library**: it writes to a real terminal and is constrained by what the terminal supports. BearLibTerminal creates its own window and has full control over rendering, making it simpler for games but unsuitable for shell integration, piping, SSH, or any terminal-native workflow.

For a project that needs to run inside a terminal (alongside shell commands, over SSH, in tmux), notcurses is the right choice. For a self-contained tile-based game that wants a retro terminal aesthetic without actual terminal constraints, BearLibTerminal (or its spiritual successors) may be simpler.

## Sources

- [GitHub README](https://github.com/dankamongmen/notcurses)
- [dankwiki: Notcurses](https://nick-black.com/dankwiki/index.php?title=Notcurses) - rendering algorithm, blitter table, benchmarks
- [notcurses_cell(3)](https://man.archlinux.org/man/notcurses_cell.3.en) - cell struct and EGC encoding
- [notcurses_render(3)](https://notcurses.com/notcurses_render.3.html) - render/rasterize pipeline
- [notcurses_plane(3)](https://www.notcurses.com/notcurses_plane.3.html) - plane model
- [doc/CURSES.md](https://github.com/dankamongmen/notcurses/blob/master/doc/CURSES.md) - differences from ncurses
- [TERMINALS.md](https://github.com/dankamongmen/notcurses/blob/master/TERMINALS.md) - terminal compatibility matrix
- [FOSDEM 2021 slides](https://archive.fosdem.org/2021/schedule/event/notcurses/attachments/slides/4479/export/events/attachments/notcurses/slides/4479/notcurses_fosdem_2021.pdf)
- [BearLibTerminal](https://github.com/cfyzium/bearlibterminal) - for comparison