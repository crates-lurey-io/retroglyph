# python-tcod (libtcod Python port)

- **Repository:** <https://github.com/libtcod/python-tcod>
- **PyPI:** `tcod` (install via `pip install tcod`)
- **Docs:** <https://python-tcod.readthedocs.io>
- **License:** BSD-2-Clause
- **Language:** Python (97.6%) wrapping C via cffi. The underlying C library is

  [libtcod](https://github.com/libtcod/libtcod).

- **Maintained by:** Kyle Benesch (HexDecimal), who also maintains libtcod itself
- **Stars:**~471 |**Python:**3.10+ |**Platforms:** Windows, macOS 10.9+, Linux (requires

  libsdl3)

- **Latest version:** 21.2.1 (as of mid-2025). Now on SDL3.

## Summary

python-tcod is the official Python port of libtcod, the most widely-used roguelike development
library. It provides a tile-based console emulator with true-color support, plus built-in FOV,
pathfinding (A\* and Dijkstra), BSP dungeon generation, noise generators, and SDL-based window/event
management. The library uses NumPy arrays extensively for high-performance bulk tile operations,
which is critical for Python performance. It is the backbone of the r/roguelikedev community
tutorial and is the default recommendation for Python roguelike development.

## What it is

python-tcod wraps the C library libtcod via Python's cffi. It is not just a thin binding; it
reimplements significant portions in Python and adds a modern Pythonic API on top of the C core. The
package ships prebuilt wheels so `pip install tcod` works without a C compiler on all major
platforms.

The library includes a backward-compatible `libtcodpy` module for legacy projects, but new code
should use the `tcod.*` namespace.

## Core modules

| Module              | Purpose                                                                                                                  |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| `tcod.console`      | Tile-based console grid: characters, fg/bg colors, blitting, drawing primitives, REXPaint import/export                  |
| `tcod.context`      | Window management via SDL. Renderers: SDL2, OpenGL, OpenGL2, xterm                                                       |
| `tcod.event`        | Full SDL event handling: keyboard, mouse, joystick, controller, window events. Includes `EventDispatch` pattern matching |
| `tcod.tileset`      | Load fonts/tilesets: tilesheet PNGs (CP437, custom), TrueType fonts, BDF bitmap fonts. Procedural block elements         |
| `tcod.map`          | Field-of-view computation with multiple algorithms                                                                       |
| `tcod.path`         | Pathfinding: A\*, Dijkstra, Dijkstra maps (dijkstra2d), hillclimbing. NumPy-based cost arrays                            |
| `tcod.bsp`          | Binary space partitioning for dungeon generation                                                                         |
| `tcod.noise`        | Perlin, Simplex, Wavelet noise generators                                                                                |
| `tcod.los`          | Line-of-sight (Bresenham)                                                                                                |
| `tcod.image`        | Image loading and manipulation                                                                                           |
| `tcod.random`       | Mersenne Twister and CMWC RNG                                                                                            |
| `tcod.render`       | Low-level SDL console rendering extension                                                                                |
| `tcod.sdl.audio`    | SDL audio playback and mixing                                                                                            |
| `tcod.sdl.video`    | SDL window/display management                                                                                            |
| `tcod.sdl.render`   | SDL renderer, textures, blend modes                                                                                      |
| `tcod.sdl.mouse`    | Mouse cursor management                                                                                                  |
| `tcod.sdl.joystick` | Joystick/controller support                                                                                              |

## Console rendering and tileset support

The console model is a 2D grid of tiles, each with a Unicode character code, foreground RGBA, and
background RGBA. Console data is exposed as NumPy structured arrays (`Console.rgb`, `Console.rgba`,
`Console.ch`, `Console.fg`, `Console.bg`), allowing vectorized operations over the entire grid.

Tileset loading supports:

- **Tilesheet PNGs** via `load_tilesheet()` with character maps (CP437, custom TCOD layout, or

  arbitrary)

- **TrueType fonts** via `load_truetype_font()`
- **BDF bitmap fonts** via `load_bdf()`
- **Procedural block elements** via `procedural_block_elements()` for box-drawing glyphs
- **Custom tile mapping** where any Unicode codepoint can be mapped to any tile graphic

The renderer uses SDL and supports multiple backends (SDL2, OpenGL, OpenGL2). The `tcod.context`
module handles window creation, presenting consoles, and converting pixel coordinates to tile
coordinates. Dynamically-sized consoles adapt to window resizes.

## Event system

`tcod.event` wraps SDL events into Python classes. Event types include `KeyDown`, `KeyUp`,
`MouseMotion`, `MouseButtonDown`, `MouseButtonUp`, `MouseWheel`, `TextInput`, `WindowResized`,
`Quit`, plus joystick/controller events.

The system supports both blocking (`tcod.event.wait()`) and non-blocking (`tcod.event.get()`) event
loops. An `EventDispatch` class provides a visitor pattern for routing events, and Python 3.10+
`match/case` structural pattern matching works directly with event classes.

Events carry both pixel and tile coordinates after calling `context.convert_event()`.

## Strengths / notable features

1. **Batteries-included for roguelikes.** FOV, pathfinding, BSP, noise, and line-of-sight are built

   in. You do not need separate libraries for the core algorithms of a traditional roguelike.

1. **NumPy integration is a major performance win.** Console data is exposed as NumPy structured

   arrays. Rendering an 80x60 map can be done with a single array assignment
   (`console.rgb[:] = tile_graphics[map_data]`) instead of nested Python loops. The tutorial author
   explicitly states: "if you want to write a Python roguelike, you **must** use NumPy" for
   performance.

1. **Mature and actively maintained.** First released in 2009, now at v21+. The maintainer

   (HexDecimal) is responsive, ships regular releases, and has modernized the API through many
   iterations. Recently ported from SDL2 to SDL3.

1. **Free-threaded Python support.** As of recent releases, deploys cp314t wheels for Python 3.14+

   free-threaded builds.

1. **Comprehensive tutorial ecosystem.** The r/roguelikedev Python tutorial (rogueliketutorials.com)

   uses python-tcod and is the most widely-followed roguelike tutorial. A newer tutorial is
   integrated into the official docs.

1. **REXPaint compatibility.** Can load and save REXPaint `.xp` files for map/UI design.

1. **Backward compatibility.** The `libtcodpy` module provides a drop-in replacement for legacy
   projects, easing migration to the modern API.

1. **Cross-platform.** Prebuilt wheels for Windows, macOS, and Linux. PyPy support.

## Weaknesses / where it falls short

1. **Steep learning curve, especially with NumPy.** The modern API requires understanding NumPy

   structured arrays, dtypes, and vectorized operations. Community feedback (the r/roguelikedev
   subreddit) notes the tutorial is intimidating for beginners. The tutorial author acknowledged:
   "the older tutorials are simpler and easier, and the newer one loses some of that."

1. **API churn and deprecation burden.** The API has evolved significantly over 15+ years. Many

   functions from the old `libtcodpy` style are deprecated, and the changelog shows frequent
   breaking changes (parameter reordering, renamed methods, dtype changes). The print methods alone
   have three generations of signatures. This creates confusion when following older guides or Stack
   Overflow answers.

1. **Tutorial quality issues.** The official tutorial (rogueliketutorials.com v2) has acknowledged

   problems: rushed refactoring sections that confuse readers, lack of explanations for design
   decisions, no animation coverage, and missing "extras" sections. The tutorial author admitted he
   had "never actually written a roguelike" when writing it.

1. **Coupled to SDL window management.** python-tcod owns the window and rendering pipeline. You

   cannot easily use it as a headless grid-computation library separate from its SDL context. If you
   want to render to a different surface (e.g., integrate with PyGame, Pyglet, or a web frontend),
   you are working against the grain.

1. **No built-in layer/composition system.** Unlike BearLibTerminal, there is no concept of layers.

   Compositing UI elements (menus, HUD, tooltips) over the game map requires manual console
   blitting. The `Console.blit()` method supports alpha but the workflow is more manual.

1. **Tile/font limitations.** All tiles in a tileset must be the same size (monospaced grid).

   Variable-width fonts or mixed tile sizes (e.g., a 16x16 character grid with 32x32 sprites
   overlaid) are not natively supported. Custom tiles must be mapped to Unicode codepoints.

1. **Python performance ceiling.** Despite NumPy, game logic (AI, simulation, entity processing) is

   still Python. For large-scale games with hundreds of entities, Python itself becomes the
   bottleneck, not tcod. Compared to a Rust or C++ roguelike library, there is an inherent speed
   cap.

1. **SDL3 migration breaking changes.** The recent port to SDL3 changed key constants (lowercase to

   uppercase), mouse events (int to float coordinates), and audio APIs. Projects pinned to older
   versions face a painful upgrade.

## Comparison with BearLibTerminal

| Aspect                | python-tcod                                                            | BearLibTerminal                                                |
| --------------------- | ---------------------------------------------------------------------- | -------------------------------------------------------------- |
| **Scope**             | Full roguelike toolkit (FOV, pathfinding, BSP, noise, console, events) | Pure terminal emulation (console + input only)                 |
| **Console model**     | NumPy structured arrays, RGBA per tile                                 | Cell-based API with `terminal_put()`, `terminal_print()`       |
| **Layers**            | No built-in layers; manual blit compositing                            | Built-in layer system for easy UI composition                  |
| **Font/tileset**      | Tilesheet PNG, TrueType, BDF, custom mapping                           | TrueType, bitmap, tilesets with per-layer font overrides       |
| **Tile composition**  | One glyph per cell (use blit for overlays)                             | Multiple glyphs per cell via layers with transparency          |
| **API simplicity**    | More complex (NumPy, contexts, structured dtypes)                      | Simpler string-configuration API (`terminal_set("font: ...")`) |
| **Algorithms**        | Built-in FOV, pathfinding, BSP, noise                                  | None; bring your own                                           |
| **Maintenance**       | Actively maintained (2025+, SDL3)                                      | Effectively abandoned (last release ~2017, author inactive)    |
| **Window management** | Full SDL integration, resizable windows, HiDPI                         | SDL-based but simpler configuration                            |
| **Python API**        | Native Python package via cffi                                         | C library with Python ctypes wrapper                           |
| **Performance**       | NumPy vectorized batch operations                                      | Per-cell function calls (slower for bulk updates in Python)    |
| **Unicode**| Full Unicode support                                                   | Full Unicode support                                           |**Key difference:** python-tcod is a complete roguelike development framework; BearLibTerminal is a |
focused terminal rendering library. BearLibTerminal is simpler for "just put tiles on screen," but
python-tcod provides far more out of the box. The critical practical issue is that BearLibTerminal
is unmaintained, while python-tcod is actively developed.

## Notable roguelikes and projects using libtcod/python-tcod

The C library libtcod has been used in many notable roguelikes. The Python port is primarily used by
hobbyist/jam projects and tutorial followers.

**Using libtcod (C/C++):**-**Cogmind** (Grid Sage Games) - commercial sci-fi roguelike. The original 7DRL (2012) used
  libtcod; the commercial version moved to a custom engine but was deeply influenced by libtcod.

- **Incursion** - updated port from Allegro to libtcod to fix longstanding bugs
- Numerous 7DRL jam entries and r/roguelikedev community projects

**Using python-tcod:**-**YARC** (Yet Another Rogue Clone) - faithful Rogue clone

- **Castle of the Eternal Night (COTEN)** - classic roguelike
- **The Lost Mind** - dungeon roguelike with strategic combat
- **PyRoPy** - roguelike being restored from Python 2/libtcodpy to Python 3/tcod
- **Mogru** - text-based roguelike
- Hundreds of tutorial-following projects from the annual r/roguelikedev event
- **Ultima Ratio Regum** - massive open-world project written in Python (uses libtcod)

Note: Caves of Qud, often mentioned alongside libtcod roguelikes, is built in Unity/C#, not libtcod.

## Sources

- Kept: [GitHub repo](https://github.com/libtcod/python-tcod) - primary source for language stats,

  status, README

- Kept: [ReadTheDocs](https://python-tcod.readthedocs.io/en/stable/) - API documentation, module

  listing, getting started examples

- Kept: [RogueBasin Doryen library page](https://roguebasin.com/index.php?title=Doryen_library) -

  history, feature list, port details

- Kept:

  [Tyler Standridge tutorial critique](https://tylerstandridge.com/posts/issues-with-the-roguelike-tutorial/) -
  first-hand account of tutorial problems, NumPy learning curve, community feedback

- Kept: [PyPI tcod](https://pypi.org/project/tcod/) - version, metadata
- Kept: [BearLibTerminal docs](http://foo.wyrd.name/en:bearlibterminal) - comparison reference
- Dropped: Generic roguelike tutorial pages on RogueBasin - duplicative of official docs
- Dropped: Individual small GitHub projects (COTEN, Mogru, etc.) - only used for examples list, not

  deep analysis
