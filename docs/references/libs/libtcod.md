# libtcod (The Doryen Library)

> The classic roguelike development library. Originally created by Jice for *The Chronicles of Doryen*, now maintained by HexDecimal (Kyle Benesch). BSD licensed. ~1.2k GitHub stars.

- **Repository**: https://github.com/libtcod/libtcod
- **Documentation**: https://libtcod.readthedocs.io/en/latest/
- **Legacy docs (more complete)**: https://libtcod.github.io/docs/
- **Language**: C and C++ (core library), with official Python bindings via [python-tcod](https://github.com/libtcod/python-tcod)
- **Dependencies**: SDL3 (as of v2.0.0; SDL2 before that), utf8proc, zlib, lodepng
- **License**: BSD
- **First release**: 2008-01-05 (v1.0)
- **Latest release**: 2026-01-06 (v2.2.2)

## What It Is

libtcod is a batteries-included toolkit for building traditional roguelikes. Unlike BearLibTerminal (which focuses purely on terminal output and input), libtcod bundles a true-color console emulator with a comprehensive set of roguelike algorithms: field-of-view, pathfinding, noise generation, BSP dungeon partitioning, heightmaps, name generation, and more. It is the single most influential library in the roguelike development ecosystem, powering the most popular roguelike tutorials and countless games.

## Language and API Design

### Dual C/C++ API

libtcod provides both a C API (`TCOD_*` functions) and a C++ API (`TCODConsole`, `TCODMap`, etc.). The C API uses opaque handles and procedural style. The C++ API wraps these in classes.

Starting from v1.19 (2021), a modern C++ API was introduced under the `tcod::` namespace with proper RAII semantics, `std::string_view`, `std::optional` colors, and value types. The older `TCODConsole::initRoot` global-state approach was deprecated in favor of explicit context objects.

```cpp
// Modern API (v1.19+)
auto root_console = tcod::Console{80, 25};
auto tileset = tcod::load_tilesheet("font.png", {32, 8}, tcod::CHARMAP_TCOD);

TCOD_ContextParams params{};
params.console = root_console.get();
params.tileset = tileset.get();
params.vsync = true;
params.sdl_window_flags = SDL_WINDOW_RESIZABLE;
params.window_title = "My Roguelike";

auto context = tcod::new_context(params);
```

```cpp
// Legacy API (still works, deprecated)
TCOD_console_set_custom_font("terminal.png", TCOD_FONT_LAYOUT_TCOD, 32, 8);
TCOD_console_init_root(80, 25, "My Roguelike", false, TCOD_RENDERER_SDL2);
```

### Python Bindings (python-tcod)

python-tcod is the official Python port, now at v21.x. It's the primary way most beginners encounter libtcod, thanks to the famous "Complete Roguelike Tutorial using python+libtcod" on RogueBasin. python-tcod uses NumPy arrays for high-performance console manipulation and has its own event system wrapping SDL. The older `libtcodpy` ctypes bindings are deprecated and removed as of libtcod 2.0.

Historical bindings existed for Lua and C#, but these are no longer maintained.

## Feature Set

### Console System

- **True color console**: Each cell has independent 24-bit foreground and background colors (16 million colors). No palette limitations.
- **Off-screen consoles**: Create multiple console buffers, blit between them with alpha blending (both foreground and background alpha).
- **Background blend modes**: SET, NONE, LIGHTEN, DARKEN, SCREEN, COLOR_DODGE, COLOR_BURN, ADD, BURN, OVERLAY, ALPHA(n), ADDALPHA(n), and more.
- **Sub-cell resolution**: `blit2x` renders using Unicode block elements for pseudo-doubled resolution.
- **Print functions**: Printf-style text rendering with alignment (left/right/center), word wrapping within rectangles, height calculation without rendering.
- **Color control codes**: Inline foreground/background color switching within printed strings.
- **REXPaint integration**: Native load/save of `.xp` files (REXPaint's format). Magic pink background for transparency.

### Tileset / Font Handling

- **Bitmap fonts**: Load from PNG images with configurable character layouts (ASCII in columns, ASCII in rows, CP437, or libtcod's custom TCOD layout).
- **BDF font support**: Added in v1.16.0-alpha.6 (2020).
- **Greyscale/colored tiles**: Greyscale fonts are tinted by the cell's foreground color. Colored tiles are rendered as-is.
- **Dynamic character mapping**: `mapAsciiCodeToFont` lets you assign any Unicode code point to any position in the font bitmap at runtime.
- **Dynamic font updates**: `TCODSystem::updateChar` can modify font tile content at runtime from an image.
- **Tileset objects** (v1.19+): `tcod::Tileset` and `tcod::load_tilesheet` provide a modern, RAII-based tileset API.
- **No layer system**: Unlike BearLibTerminal, libtcod does not natively support multiple tile layers per cell. You achieve layering through off-screen console blitting with alpha.

### Rendering Backend

The rendering backend has gone through significant evolution:

| Era | Renderers | Notes |
|-----|-----------|-------|
| v1.0-1.4 | SDL1 software | Original renderer |
| v1.5.1 | + GLSL, OpenGL (fixed pipeline) | "FPS increased 880% on true color sample" |
| v1.6.0 (2016) | Upgraded to SDL2 | SDL1 removed |
| v1.8.0 (2018) | + SDL2 renderer | New SDL2-based renderer |
| v1.9.0 (2018) | + OpenGL2 renderer | GLSL-based, required only GL 2.0 |
| v1.21.0 (2022) | SDL2 rewritten with `SDL_RenderGeometry` | Major performance improvement |
| v1.23.1 (2022) | All renderers forced to SDL2 | OpenGL renderers had rare artifacts, GLAD removed |
| v2.0.0 (2025) | Switched to SDL3 | OpenGL renderers previously removed in PR #137 |

The OpenGL renderers were a persistent source of bugs: texture atlas bleeding on certain GPU/driver combinations, inconsistent alpha blending, shader compilation failures. The maintainer eventually consolidated everything to the SDL2/SDL3 renderer. A `TCOD_RENDERER_XTERM` was added in v1.20.0 for terminal-based rendering.

**Dirty rect optimization**: The renderer tracks which cells changed between frames via a `cache_console`, reducing SDL render calls for large consoles with few changes per frame.

### Input System

libtcod's input handling has been through multiple generations:

1. **v1.0-1.5**: Custom keyboard/mouse API via `TCOD_console_check_for_keypress` / `TCOD_console_wait_for_keypress` / `TCOD_mouse_get_status`
2. **v1.6.0**: Adapted to SDL2's text input model (`TCODK_TEXT` event for `SDL_TEXTINPUT`). Combined into `TCOD_sys_check_for_event` / `TCOD_sys_wait_for_event`.
3. **v1.17.0 (2021)**: All libtcod event handling deprecated. Users directed to use SDL2 events directly.
4. **v1.19.0 (2021)**: Added `TCOD_Context::convert_event_coordinates` for pixel-to-tile coordinate conversion of SDL events.
5. **v2.0.0 (2025)**: SDL3 events.

The recommended modern approach is a raw SDL event loop with `context->convert_event_coordinates(event)` for mouse tile mapping. Helper functions like `tcod::sdl2::process_event` exist to convert SDL events back to libtcod's `TCOD_key_t`/`TCOD_mouse_t` for porting older code.

**Contrast with BearLibTerminal**: BearLibTerminal provides its own clean, abstracted input queue (`terminal_read`, `terminal_has_input`, `terminal_peek`) that hides SDL entirely. libtcod has moved in the opposite direction, exposing SDL directly and deprecating its own abstractions.

### Roguelike Algorithm Toolkits

This is where libtcod stands apart from BearLibTerminal (which has zero algorithmic tools):

- **Field of View**: 7 algorithms: Basic (ray casting), Diamond, Shadow casting, Permissive (9 levels), Restrictive (MRPAS), and Symmetric Shadowcast. The FOV module has been extracted to a standalone library `libtcod-fov`.
- **Pathfinding**: A* and Dijkstra, with custom cost callbacks or map-based. Supports diagonal movement with configurable cost.
- **BSP (Binary Space Partition)**: Recursive splitting of rectangles for dungeon generation. Supports traversal callbacks.
- **Noise generation**: Perlin, Simplex, and Wavelet noise in 1-4 dimensions. FBM and turbulence functions. Vectorized variants added later.
- **Heightmap toolkit**: Full heightmap generation and manipulation (rain erosion, kernel transforms, midpoint displacement, noise-based generation).
- **Name generator**: Syllable-based name generation with 20+ predefined syllable sets and custom definition support.
- **RNG**: Mersenne Twister and Complementary Multiply With Carry (CMWC). Gaussian distribution support.
- **Line drawing**: Bresenham line algorithm (both callback-based and iterator-based in C++).
- **Image toolkit**: PNG load/save, rotation, scaling, sub-cell resolution blitting.
- **File parser**: Custom config file format parser (largely historical).
- **Compression toolkit**: zlib-based serialization (deprecated).

## Notable Games and Projects

### Cogmind (Grid Sage Games)

Cogmind is the most prominent commercial roguelike associated with libtcod's ecosystem, but the relationship is nuanced. Developer Josh Ge (Kyzrati) built "Rogue Engine X" (REX), a custom game engine that uses libtcod as its rendering foundation. REX extends libtcod with features like dynamic terminal swapping, multi-size glyphs (wide tiles for square map cells, quad tiles for zoom), and dirty-rect optimization. REXPaint, the widely-used ASCII art editor, was also built on REX/libtcod.

Cogmind demonstrates what's possible when libtcod's console system is pushed to its limits: dynamic lighting, particle effects, smooth animations, and a polished UI, all within a terminal-style grid.

### Tutorial Ecosystem

libtcod's largest impact is through tutorials. The "Complete Roguelike Tutorial, using python+libtcod" on RogueBasin is arguably the most influential roguelike development resource ever created, spawning ports to C++, Rust, and other languages. Thousands of roguelikes were started from these tutorials.

### Other Notable Projects

- **The Chronicles of Doryen**: The original game libtcod was built for (by Jice)
- **Pyromancer**: A 2009 7DRL game by HexDecimal (current maintainer), notable for impressive lighting/visual effects
- **Umbra**: A libtcod-based game engine used by several older projects (by Mingos/others)
- Countless 7DRL competition entries and r/roguelikedev projects

**Note**: Caves of Qud and DCSS do NOT use libtcod. Caves of Qud uses Unity; DCSS has its own rendering.

## Strengths

1. **Batteries included**: No other roguelike library comes close to the breadth of built-in algorithms. FOV, pathfinding, BSP, noise, heightmaps, and name generation, all in one package.

2. **Tutorial ecosystem**: The python+libtcod tutorial series is the on-ramp for a huge portion of the roguelike development community. No other library has this level of beginner documentation.

3. **True color from day one**: 24-bit color per cell was novel when libtcod launched in 2008. The extensive background blend modes (overlay, screen, dodge, burn, etc.) enable sophisticated visual effects.

4. **Active maintenance**: Despite being 18+ years old, libtcod is actively maintained. The migration to SDL3 in 2025, ongoing API modernization, and splitting into standalone libraries show continued investment.

5. **REXPaint integration**: Native `.xp` file support makes it trivial to use REXPaint for designing UI layouts, map prefabs, and ASCII art that load directly into libtcod consoles.

6. **Cross-platform**: Windows, Linux, macOS, and even experimental browser support. Available via Vcpkg, CMake FetchContent, and as a submodule.

7. **FOV algorithm collection**: 7 different FOV algorithms with different trade-offs is unmatched. The standalone `libtcod-fov` library is useful even outside libtcod.

8. **Off-screen console blitting**: The ability to compose multiple consoles with alpha blending is a powerful UI building tool, even without explicit layer support.

## Weaknesses and Criticisms

1. **Monolithic design**: The maintainer (HexDecimal) explicitly acknowledges this in [issue #147](https://github.com/libtcod/libtcod/issues/147): "Libtcod's size makes it difficult to port, maintain, and document. It has too many things at once." Plans to split into `libtcod-fov`, `libtcod-terminal`, `libtcod-pathfinding`, `libtcod-noise` are in progress but incomplete.

2. **Documentation gap**: The latest docs (v2.2.2) are self-described as "incomplete." Most users are directed to the 1.6.4 docs from 2017. This creates confusion about which API to use: the old deprecated API that's well-documented, or the new modern API that isn't.

3. **API churn and deprecation waves**: The library has gone through massive API changes: v1.5 to v1.6 (SDL2 migration, input model change), v1.19 (context objects, new C++ API, deprecated root console), v2.0 (SDL3, removed libtcodpy). Each transition left a trail of deprecated functions and confused users. The v2.1.0 changelog deprecates 15+ console functions at once.

4. **No native layer system**: Each cell has one foreground character, one fg color, one bg color. Compositing multiple tiles per cell requires manual off-screen console blitting. BearLibTerminal's native layer system is significantly more convenient for this.

5. **Input system instability**: Three complete rewrites of the input system, ultimately punting to "just use SDL directly." This is pragmatic but means libtcod no longer abstracts input at all, unlike BearLibTerminal's clean `terminal_read()` API.

6. **Renderer history of instability**: Years of OpenGL renderer bugs (atlas bleeding, shader failures, alpha inconsistencies) before the maintainer gave up and forced everything to SDL2. The v1.23.1 changelog explicitly states: "Forced all renderers to RENDERER_SDL2 to fix rare graphical artifacts with OpenGL."

7. **C++ build complexity**: As a C++ library, it requires matching runtime versions. The README warns about distributing Visual Studio runtimes. SDL3 as a dependency adds build complexity. Contrast with BearLibTerminal's single-DLL distribution model.

8. **No tile layers or composition**: For games that want character + floor tile + item tile per cell, libtcod requires manual console blitting workflows. BearLibTerminal handles this with its layer system natively.

9. **Font handling limitations**: Fonts must be loaded from specially arranged sprite sheets. No built-in TrueType/vector font rendering (BDF support was added in 2020). BearLibTerminal supports TrueType fonts natively.

## Version Evolution Summary

| Version | Year | Milestone |
|---------|------|-----------|
| 1.0 | 2008 | Initial release: console, FOV, noise |
| 1.1 | 2008 | Added noise and FOV toolkits |
| 1.3 | 2008 | Mouse support, file parser |
| 1.4.0 | 2008 | Pathfinding, BSP, heightmap, PNG fonts, simplex/wavelet noise |
| 1.5.0 | 2010 | Name generator, Dijkstra pathfinding, Unicode support, GLSL/OpenGL renderers |
| 1.5.1 | 2012 | Colored tiles, GLSL renderer ("880% FPS increase"), REXPaint file formats |
| 1.6.0 | 2016 | **SDL2 migration** (SDL1 removed), text input events |
| 1.7.0 | 2018 | Semantic versioning adopted |
| 1.8.0 | 2018 | UTF-8 print functions, SDL2 renderer, C99/C++14 standard |
| 1.9.0 | 2018 | OpenGL2 renderer |
| 1.19.0 | 2021 | **Context API** (deprecated root console), modern C++ API, C++17 |
| 1.21.0 | 2022 | SDL2 renderer rewritten with `SDL_RenderGeometry`, can build without SDL |
| 1.23.1 | 2022 | All renderers forced to SDL2, OpenGL removed |
| 2.0.0 | 2025 | **SDL3 migration**, libtcodpy removed, API-only versioning |
| 2.2.2 | 2026 | Current release |

## Comparison with BearLibTerminal

| Aspect | libtcod | BearLibTerminal |
|--------|---------|-----------------|
| **Focus** | Full roguelike toolkit | Terminal output + input only |
| **Language** | C/C++ core, Python bindings | C core, bindings for 8+ languages |
| **Color** | 24-bit true color | 32-bit (RGBA with alpha) |
| **Tile layers** | None (use console blitting) | Native layer system per cell |
| **Font support** | Bitmap sheets, BDF | Bitmap sheets, TrueType, auto-tiling |
| **Input** | Deprecated own, uses SDL directly | Clean abstracted API (`terminal_read`) |
| **FOV/pathfinding** | 7 FOV algorithms, A*/Dijkstra | None |
| **Noise/BSP/etc.** | Full suite of algorithms | None |
| **Rendering backend** | SDL3 | SDL2, OpenGL |
| **Build/distribution** | CMake, Vcpkg, complex | Single DLL/SO, trivial |
| **Maintenance** | Active (SDL3, ongoing work) | Abandoned since ~2017 |
| **Tutorial ecosystem** | Massive (RogueBasin tutorials) | Small |
| **API stability** | Many breaking changes over time | Stable (frozen) |

### Key Architectural Differences

- **libtcod** treats the console as one layer of a larger roguelike toolkit. It owns the window, the event loop, and provides algorithms. The trend is toward exposing SDL directly rather than abstracting it.
- **BearLibTerminal** treats the console as its sole concern and does it thoroughly: layers, composition, Unicode, TrueType, and a clean abstraction over the platform. It deliberately excludes algorithms, leaving those to the developer.

For a new library inspired by both, the lesson is: BearLibTerminal's console/rendering model is more developer-friendly (layers, TrueType, clean input), while libtcod's algorithmic toolkits are invaluable but should be separate, composable libraries (which is exactly what libtcod's maintainer is working toward with the split).

## Sources

- [GitHub: libtcod/libtcod](https://github.com/libtcod/libtcod) - Repository, README, CHANGELOG
- [libtcod docs v2.2.2](https://libtcod.readthedocs.io/en/latest/) - Console, Context, Upgrading guides
- [libtcod docs v1.6.4](https://libtcod.github.io/docs/) - Complete legacy documentation
- [RogueBasin: Doryen library](https://www.roguebasin.com/index.php?title=Libtcod) - Feature list, project info
- [Issue #147: Split libtcod](https://github.com/libtcod/libtcod/issues/147) - Maintainer's critique of monolithic design
- [PR #137: Remove OpenGL renderers](https://github.com/libtcod/libtcod/pull/137) - Renderer consolidation
- [Grid Sage Games blog](https://www.gridsagegames.com/blog/) - Cogmind's engine architecture posts
- [RogueBasin output libraries comparison](https://github.com/Chizaruu/roguebasin/blob/main/wiki/output_libraries.md)