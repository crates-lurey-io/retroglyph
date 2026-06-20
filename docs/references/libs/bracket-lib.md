# bracket-lib (formerly RLTK)

## Summary

bracket-lib is a Rust roguelike toolkit (originally called RLTK) by Herbert Wolverson. It provides a
virtual CP437/ASCII terminal with GPU-backed rendering, layered console system, tile/sprite support,
and bundled algorithms (pathfinding, FOV, noise, dice). The API is well-designed for grid-based
games and has excellent learning materials, but active development has stalled since late 2022 and
the crate is accumulating unresolved issues on modern platforms.

## Language

Rust. Pure Rust implementation (no C/C++ bindings). MIT licensed.

- Crate: `bracket-lib` 0.8.7 on crates.io (last published 2022-10-04)
- GitHub: <https://github.com/amethyst/bracket-lib> (~1.6k stars, 122 forks)
- ~350k downloads on crates.io for bracket-terminal alone

## Architecture

bracket-lib is split into several independent crates, all re-exported by the `bracket-lib`
meta-crate:

| Crate                      | Purpose                                                               |
| -------------------------- | --------------------------------------------------------------------- |
| `bracket-terminal`         | Virtual terminal/console, game loop, input, rendering backends        |
| `bracket-pathfinding`      | A\* and Dijkstra maps                                                 |
| `bracket-geometry`         | Points, lines, circles, Bresenham, distance algorithms                |
| `bracket-color`            | RGB/HSV color system with named colors and blending                   |
| `bracket-noise`            | Port of FastNoise (Perlin, Simplex, etc.)                             |
| `bracket-random`           | Dice-oriented RNG, parses RPG strings like `3d6+12`                   |
| `bracket-algorithm-traits` | Shared traits (BaseMap, Algorithm2D, Algorithm3D) for pathfinding/FOV |

You can depend on individual crates or pull in everything via `bracket-lib`.

## Terminal/Console Layer (bracket-terminal)

This is the core rendering component and the most relevant for comparison.

### Console types

Five distinct console types, each suited to different use cases:

- **SimpleConsole**: Dense grid buffer. Every cell has a glyph, fg, and bg. Best for full-screen map

  layers.

- **SimpleConsole (no bg)**: Same but skips background rendering, allowing layers below to show

  through.

- **SparseConsole**: Stores only the cells you explicitly set. Ideal for entities, characters,

  overlays.

- **FancyConsole**: Like sparse, but supports fractional coordinates and per-character rotation.
- **SpriteConsole**: Renders sprites from a sprite sheet in pixel coordinates (described as "in its

  infancy").

- **VirtualConsole**: Not rendered directly. Stores large amounts of text for windowed viewing

  (logs, docs).

### Layering

Consoles stack as layers, rendered in initialization order. Each layer can have different:

- Grid dimensions (e.g., 80x50 map layer, 80x25 HUD layer)
- Tile sizes (8x8, 16x16, 32x32)
- Font/tileset files

Example: map tiles on layer 0 (32x32 dungeon font), entity sprites on layer 1 (no background), text
HUD on layer 2 (8x8 terminal font). This is one of bracket-lib's strongest design points.

### Font and tileset handling

- Default: embedded 8x8 CP437 font. Also includes VGA 8x16.
- Custom fonts via `with_font("filename.png", char_width, char_height)`.
- Font files are PNG sprite sheets arranged in CP437 layout (16x16 grid of glyphs).
- `with_font_bg()` variant allows specifying a background color to treat as transparent.
- `to_cp437('X')` converts Rust chars to CP437 indices.
- Supports runtime font switching (`ctx.set_active_font()`).
- Unicode support exists but is slow; requires loading a large font atlas. Noted as needing

  `--release` mode.

- REXPaint (.xp) file loading built in for importing sprite art.
- `embedded_resource!` / `link_resource!` macros for embedding fonts in the binary (important for

  WASM).

### Rendering backends

| Backend                   | Feature flag    | Notes                                                      |
| ------------------------- | --------------- | ---------------------------------------------------------- |
| OpenGL (default)          | (default)       | Full feature support including post-processing effects     |
| WebGL/WASM                | (auto-detected) | Compile target `wasm32-unknown-unknown`                    |
| WebGPU/Vulkan/Metal       | `webgpu`        | Everything except post-processing. Requires `resolver = 2` |
| Crossterm                 | `cross_term`    | Native terminal. No graphical features                     |
| Curses (ncurses/pdcurses) | `curses`        | Native \*nix terminal. No graphical features               |

The graphical backends (OpenGL, WebGPU) render CP437 as sprites, guaranteeing identical appearance
across platforms regardless of system fonts.

### Input handling

- Simple mode: `ctx.key` returns the currently pressed `VirtualKeyCode` enum. Mouse position via

  `ctx.mouse_pos()`.

- Advanced mode (`with_advanced_input(true)`): Stream of keyboard/mouse events, including

  `is_pressed()` for key state queries.

- Mouse click position automatically maps to console grid coordinates.

### Other terminal features

- Post-processing: Scanlines, screen burn, CRT effects (OpenGL only).
- `DrawBatch`: Batched rendering commands with z-ordering for efficient draw call submission.
- `TextBlock`: Builder for word-wrapped, formatted text blocks.
- `with_automatic_console_resize(true)`: Window resize recalculates grid dimensions instead of

  scaling.

- FPS cap, vsync control, fullscreen toggle.
- Screenshot support (`ctx.screenshot("file.png")`).

### Game loop model

bracket-terminal owns the main loop. You implement `GameState` trait with a
`tick(&mut self, ctx: &mut BTerm)` method called every frame. No async, no ECS integration built in
(though `specs` feature flag adds Component derives). This is a simple, imperative model, not an
engine.

## Games and Projects Built With It

### Books and tutorials

- **Hands-on Rust** (PragProg, Herbert Wolverson): Commercial book teaching Rust via game dev,

  builds a dungeon crawler. [Source](https://hands-on-rust.com/)

- **Rust Roguelike Tutorial**: Comprehensive free tutorial, 70+ chapters building a roguelike from

  scratch. One of the most complete roguelike tutorials in any language.
  [Source](https://bfnightly.bracketproductions.com/rustbook/)

### Games and sample projects (from README)

- [Innit](https://github.com/Micutio/innit): Cellular automaton simulation
- [Shotcaller](https://github.com/amethyst/shotcaller): MOBA-like game by Amethyst org
- [rouge](https://github.com/bofh69/rouge): Roguelike
- [miners](https://github.com/carsin/miners): Mining game
- [my-little-robots](https://github.com/baszalmstra/my-little-robots): Robot programming game
- [Terrain-Generator](https://github.com/Havegum/Terrain-Generator): Procedural terrain
- [rs-gliphus](https://github.com/Bobox214/rs-gliphus): Roguelike
- [blademaster](https://github.com/Maxgy/blademaster) and

  [text-rts](https://github.com/Maxgy/text-rts)

### Community projects

- [rust-rl](https://github.com/Llywelwyn/rust-rl): Roguelike with WASM web version, uses symmetric

  shadowcasting

- [Blackspire](https://github.com/pragmatic-rustacean/Blackspire): Dungeon crawler with Legion ECS +

  bracket-lib rendering

- [bracket_ratatui](https://github.com/gold-silver-copper/bracket_ratatui): Integration bridge

  between bracket-lib and ratatui

- Numerous game jam entries and tutorial followers (many repos on GitHub following the roguelike

  tutorial)

### Author's own games

- Nox Futura, One Knight in the Dungeon (mentioned on Patreon page)

## Strengths

1. **Best-in-class roguelike tutorial ecosystem.** The free 70+ chapter roguelike tutorial and the

   Hands-on Rust book make bracket-lib the most documented path into Rust roguelike development. No
   other Rust library comes close for guided learning.
   [Source](https://bfnightly.bracketproductions.com/rustbook/)

1. **Clean, ergonomic API.** `BTermBuilder::simple80x50().with_title("Game").build()` gets you a

   working window in 10 lines of code. The builder pattern for configuring consoles, layers, and
   fonts is well-designed.
   [Source](https://github.com/amethyst/bracket-lib/tree/master/bracket-terminal)

1. **Multi-backend portability.** Same code runs on OpenGL, WebGPU (Vulkan/Metal), WebGL (WASM),

   crossterm, and ncurses. The graphical backends render CP437 as sprites, so the game looks
   identical everywhere. WASM support is a standout feature.
   [Source](https://docs.rs/crate/bracket-terminal/latest)

1. **Layered console system.** Multiple consoles with independent grid sizes, tilesets, and

   transparency composited together. Mix CP437 text, graphical tiles, and sprites in one window.
   This is more flexible than BearLibTerminal's single-layer-with-composition model.
   [Source](https://bfnightly.bracketproductions.com/bracket-lib/consoles.html)

1. **Bundled roguelike algorithms.** A\*, Dijkstra maps, field-of-view, Bresenham lines, noise

   generation, dice rolling. All Rust-native with trait-based integration (`BaseMap`,
   `Algorithm2D`). Not just a terminal library but a toolkit.
   [Source](https://github.com/amethyst/bracket-lib)

1. **Modular crate design.** Use just `bracket-pathfinding` without the terminal, or just

   `bracket-noise`. The crates are independently useful.
   [Source](https://crates.io/crates/bracket-lib)

1. **DrawBatch system.** Batched rendering with z-order sorting. Collect draw commands and submit

   them efficiently, avoiding per-cell draw overhead.
   [Source](https://github.com/amethyst/bracket-lib/tree/master/bracket-terminal)

## Weaknesses and Pain Points

1. **Effectively unmaintained.** Last crates.io publish was October 2022. The author acknowledged

   going on "long paternity leave" in March 2022 (issue #261) and said the API is "pretty much
   frozen for compatibility with the book." Commit activity has not meaningfully resumed. Open
   issues accumulate without response. [Source](https://github.com/amethyst/bracket-lib/issues/261)

1. **Broken on modern platforms.** Multiple open issues report crashes on Wayland (mouse hover

   crash, issue #318), runtime errors with Rust 2024 edition (issue #373), and
   `unsafe precondition violated: slice::from_raw_parts` crashes with recent Rust nightly
   (StackOverflow, Jan 2025). The pinned dependencies are aging.
   [Source](https://github.com/amethyst/bracket-lib/issues/373)

1. **Alpha transparency is broken.** Black pixels with alpha get culled instead of rendered. Issue

   #197, open since 2021, with 3 upvotes and no fix. This affects anyone using custom tilesets with
   transparency. [Source](https://github.com/amethyst/bracket-lib/issues/197)

1. **Sprite system is immature.** The author describes it as "in its infancy." SpriteConsole exists

   but is underdocumented and limited compared to the text/tile consoles.
   [Source](https://bfnightly.bracketproductions.com/bracket-lib/ex_bterm.html)

1. **Owns the main loop.** `main_loop(context, state)` takes control. No way to drive rendering from

   your own loop, which limits integration with other systems (async runtimes, custom event loops,
   ECS frameworks that want to own the schedule). BearLibTerminal's polling model is more flexible
   here. [Source](https://github.com/amethyst/bracket-lib/tree/master/bracket-terminal)

1. **Sparse documentation beyond the tutorial.** The usage manual is incomplete ("early work has

   begun on writing a manual"). API docs on docs.rs exist but are thin. Users on Devtalk have
   complained about not knowing how to use features like sprites without reverse-engineering
   examples. [Source](https://devtalk.com/t/hands-on-rust-adequate-docs-for-bracket-lib/13185)

1. **High CPU usage reported.** The "State of Game Dev in Rust 2024" article notes 97% CPU to render

   ASCII characters when running the Hands-on Rust example code, suggesting performance issues in
   the rendering pipeline.
   [Source](https://games.brettchalupa.com/devlog/the-state-of-game-dev-in-rust-2024/)

1. **Crossterm backend panics.** Index out of bounds when using tile-based consoles with the

   crossterm backend (issue #257, open since 2022, still active as of Oct 2025). The terminal
   backends don't support all features the graphical backends do.
   [Source](https://github.com/amethyst/bracket-lib/issues/257)

1. **Curses backend broken.** Compilation error on `fitscreen` field (issue #329, open since Jan

   2023). [Source](https://github.com/amethyst/bracket-lib/issues/329)

## Comparison with BearLibTerminal

| Aspect             | bracket-lib                                              | BearLibTerminal                                                  |
| ------------------ | -------------------------------------------------------- | ---------------------------------------------------------------- |
| Language           | Pure Rust                                                | C with bindings (Rust via `doryen-rs` or `bear-lib-terminal-rs`) |
| Maintenance        | Stalled (last release 2022)                              | Also stalled (original C lib inactive)                           |
| Grid model         | Multiple console layers with independent grids           | Single window, composition via `TK_LAYER`                        |
| Tile support       | Font-as-tileset (PNG sprite sheets), early SpriteConsole | Native tile composition, better alpha handling                   |
| Backends           | OpenGL, WebGPU, WASM, crossterm, curses                  | OpenGL only                                                      |
| WASM               | Yes, first-class                                         | No                                                               |
| Main loop          | Library-owned (`main_loop()`)                            | User-owned (polling model)                                       |
| Algorithms         | Bundled (A\*, Dijkstra, FOV, noise, dice)                | None (terminal only)                                             |
| Tutorial ecosystem | Exceptional (book + 70-chapter free tutorial)            | Minimal                                                          |
| API style          | Rust-native builder pattern                              | C-style function calls                                           |
| Unicode            | Possible but slow                                        | Better native support                                            |
| Post-processing    | Scanlines, screen burn (OpenGL)                          | None                                                             |

bracket-lib is the more complete toolkit if you want algorithms + terminal in one package, with
better WASM support and learning resources. BearLibTerminal has a simpler, more flexible composition
model for tiles and a user-controlled main loop. Both suffer from stalled maintenance.

## Sources

- Kept:
  - [GitHub README](https://github.com/amethyst/bracket-lib) -- primary source for architecture,

    crate structure, feature flags

  - [bracket-terminal README](https://github.com/amethyst/bracket-lib/tree/master/bracket-terminal)

    -- backend details, minimal example, feature comparison

  - [Usage Guide: Consoles](https://bfnightly.bracketproductions.com/bracket-lib/consoles.html) --

    console types, layer system, builder API

  - [Usage Guide: Examples](https://bfnightly.bracketproductions.com/bracket-lib/ex_bterm.html) --

    full list of terminal examples with descriptions

  - [Issue #261: "Is this dead?"](https://github.com/amethyst/bracket-lib/issues/261) -- author's

    response on maintenance status

  - [Issue #373: Runtime errors](https://github.com/amethyst/bracket-lib/issues/373) -- modern

    Rust/Wayland breakage

  - [Issue #197: Alpha transparency](https://github.com/amethyst/bracket-lib/issues/197) -- tileset

    rendering bug

  - [crates.io: bracket-terminal](https://crates.io/crates/bracket-terminal) -- download stats,

    version history

  - [State of Game Dev in Rust 2024](https://games.brettchalupa.com/devlog/the-state-of-game-dev-in-rust-2024/)

    -- external perspective on bracket-lib and ecosystem

  - [Devtalk: bracket_lib docs](https://devtalk.com/t/hands-on-rust-adequate-docs-for-bracket-lib/13185)

    -- user frustration with docs

- Dropped:
  - Individual tutorial-follower repos (michael-suggs, palmergs, jossse69) -- derivative, no unique

    insight

  - Patreon page -- just membership tiers, no technical content
  - libhunt comparison pages -- aggregator noise, no original analysis

## Gaps

- **Performance benchmarks**: No rigorous benchmarks found comparing bracket-terminal rendering

  performance to alternatives (doryen-rs, macroquad, raw crossterm). The 97% CPU report is
  anecdotal.

- **Fork activity**: Did not investigate whether any maintained fork exists. Worth checking GitHub

  forks for active development.

- **bracket-bevy**: There's a Bevy integration (`bracket-bevy`) mentioned in issues but its status

  and compatibility with current Bevy are unknown.

- **Exact commit history**: GitHub's commits page failed to render (auth wall). The last crates.io

  publish (0.8.7, Oct 2022) is the best proxy for last meaningful release.
