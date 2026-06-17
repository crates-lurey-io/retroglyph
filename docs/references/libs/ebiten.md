# Reference: Ebitengine (Ebiten)

## Summary

Ebitengine is a 2D game engine for Go with ~13K GitHub stars, maintained by Hajime Hoshi since 2013.
It provides a minimal, opinionated API centered on image-to-image drawing with GPU-accelerated
rendering (OpenGL, Metal, DirectX, WebGL). It compiles to desktop (Windows/macOS/Linux/FreeBSD),
mobile (iOS/Android), WebAssembly, and even Nintendo Switch/Xbox. For roguelike and terminal-style
games, it works well as a tile renderer: you draw glyph sub-images from a tileset onto the screen
each frame, and the engine automatically batches draw calls. Several roguelike frameworks (ramen,
Grogue) and a full GPU terminal emulator (darktile) have been built on top of it.

## Language

Go. Pure Go on desktop and WASM; Cgo required for mobile, Switch, and Xbox.

## Core Architecture

### Game Loop

The API surface is intentionally small. You implement one interface with three methods:

```go
type Game interface {
    Update() error                                          // logic tick, default 60 TPS
    Draw(screen *ebiten.Image)                              // render frame
    Layout(outsideWidth, outsideHeight int) (int, int)      // logical screen size
}
```

`Update()` and `Draw()` are decoupled: Update runs at a fixed tick rate (configurable via
`ebiten.SetTPS`), Draw runs at display refresh rate. This separation is clean for turn-based games
like roguelikes where logic updates can be independent of rendering.
[Cheat Sheet](https://ebitengine.org/en/documents/cheatsheet.html)

### Rendering Pipeline

- **Image-centric**: Everything is `*ebiten.Image`. You draw images onto images. The screen itself
  is an `*ebiten.Image`.
- **SubImage for tilesets**: `img.SubImage(rect)` returns a view into a tileset. Drawing many
  SubImages from the same source image is automatically batched into one GPU draw call.
- **Automatic texture atlas**: Small images are packed into a shared 4096x4096 atlas internally,
  reducing texture switches.
- **Automatic draw batching**: Consecutive `DrawImage` calls to the same target, from images on the
  same atlas, with the same blend/filter settings, are merged into a single GPU command. This is the
  primary performance lever.
- **Geometry transforms via GeoM**: Translation, rotation, scaling via a 2D affine matrix.
- **ColorScale**: Per-draw RGBA color multiplication (useful for tinting glyphs).
- **Custom shaders (Kage)**: A Go-like shading language that compiles to GLSL, HLSL, and Metal
  Shading Language. Fragment shaders only. Useful for post-processing effects (CRT filters,
  lighting, fog of war). [Shader docs](https://ebitengine.org/en/documents/shader.html)
- **Graphics backends**: OpenGL (desktop Linux/Windows), Metal (macOS/iOS), DirectX (Windows), WebGL
  (WASM). Selected automatically.
  [DeepWiki: Graphics Driver Interface](https://deepwiki.com/hajimehoshi/ebiten/3.1-graphics-driver-interface)

### Input Handling

- `ebiten.IsKeyPressed(key)` for continuous press state.
- `inpututil.IsKeyJustPressed(key)` for single-frame press detection.
- Mouse position via `ebiten.CursorPosition()`.
- Gamepad and touch input supported.
- Community library [ebitengine-input](https://github.com/quasilyte/ebitengine-input) provides
  Godot-style action mapping (bind actions to keys/buttons, check actions instead of raw keys).

### Text Rendering

- `text/v2` package renders TrueType/OpenType fonts via Go's `font.Face`.
- [etxt](https://github.com/tinne26/etxt) is a community library for more advanced text layout.
- [bitmapfont](https://github.com/hajimehoshi/bitmapfont) provides a plug-and-play bitmap font with
  wide Unicode coverage.
- For terminal-style games, most projects use a PNG tileset of glyphs and draw via SubImage,
  bypassing the text system entirely.

## Roguelike and Terminal-Style Usage

### Ramen Console Emulator

[github.com/BigJk/ramen](https://github.com/BigJk/ramen) (67 stars, Apache-2.0)

A libtcod-inspired console emulator built on Ebiten. Provides:

- Grid of cells, each with foreground/background color and a glyph.
- PNG bitmap fonts (supports >256 chars, colored tiles).
- Sub-consoles for layered rendering.
- Inline color markup in strings: `[[f:#ff0000]]red text`.
- Component-based UI (TextBox, Button).
- REXPaint file parsing.

This is the closest thing to BearLibTerminal in the Go/Ebiten ecosystem. The same author (BigJk)
also created [End of Eden](https://github.com/BigJk/end_of_eden), a "Slay the Spire"-like roguelite
with both terminal and GL rendering modes.

### Grogue Tutorial Series

[callaway.dev/grogue-a-roguelike-tutorial-in-go-part-0](https://callaway.dev/grogue-a-roguelike-tutorial-in-go-part-0/)

A 13-part tutorial series adapting the classic Roguebasin/libtcod roguelike tutorial to Go +
Ebitengine. Covers dungeon generation, FOV, combat, inventory, saving/loading. Demonstrates the
tile-based rendering pattern: load a PNG tileset, use SubImage to pick glyphs, DrawImage to place
them on a grid.

### Other Roguelike Projects

- [kensonjohnson/roguelike-in-go](https://github.com/kensonjohnson/roguelike-in-go): Ebiten
  roguelike with WebAssembly build, playable via GitHub Pages.
- [photogabble/go-roguelike-tutorial](https://github.com/photogabble/go-roguelike-tutorial): Another
  roguelike tutorial implementation.
- [cscazorla/roguelike](https://github.com/cscazorla/roguelike): Basic roguelike following
  Roguebasin route map.

### Darktile: GPU Terminal Emulator

[github.com/liamg/darktile](https://github.com/liamg/darktile)

A full terminal emulator built on Ebitengine, proving it can handle rapid, per-cell character
rendering at terminal speeds. Features GPU rendering, Unicode, font ligatures, sixel graphics,
transparency. This is strong evidence that Ebiten's rendering is fast enough for grid-of-glyphs use
cases.

### Grid/Tilemap Support Libraries

- [egriden](https://github.com/greenthepear/egriden): Framework for grid-based games on Ebitengine.
- [go-tiled](https://github.com/lafriks/go-tiled): Tiled map editor (TMX) loader.
- [ldtkgo](https://github.com/SolarLune/ldtkgo): LDtk level editor loader.
- [grid](https://github.com/s0rg/grid): Generic 2D grid with pathfinding, ray/shadow casting,
  line-of-sight.
- [pathing](https://github.com/quasilyte/pathing): Efficient grid-based pathfinding.

## Strengths

1. **Minimal API, fast onboarding.** Three methods to implement. No inheritance hierarchies, no
   scene graphs, no node trees. You own the game loop. This is a feature for developers who want
   control.

2. **Automatic draw batching.** For tile-based rendering, drawing hundreds or thousands of SubImages
   from the same tileset source is batched into one or a few GPU calls automatically. The
   `examples/sprites` demo renders 10,000+ sprites in ~1 draw call.
   [Performance Tips](https://ebitengine.org/en/documents/performancetips.html)

3. **Cross-platform from one codebase.** Desktop + WASM + mobile + consoles. WASM support is
   particularly notable for roguelikes (playable in browser). No code changes needed, just
   `GOOS=js GOARCH=wasm go build`.

4. **Pure Go on desktop.** No C dependencies on desktop/WASM builds (uses purego for system calls).
   Simplifies builds and CI.

5. **Custom shaders (Kage).** Go-like syntax compiles to GLSL/HLSL/Metal. Useful for
   post-processing: CRT effects, fog of war overlays, lighting.
   [Kage tutorial](https://github.com/tinne26/kage-desk)

6. **Active ecosystem.** 13K stars, active Discord, multiple ECS frameworks (donburi, ark), GUI
   libraries (ebitenui), and the awesome-ebitengine list has 100+ entries.
   [awesome-ebitengine](https://github.com/sedyh/awesome-ebitengine)

7. **Stable and mature.** Actively maintained since 2013. v2 API is stable. Apache-2.0 licensed.

8. **Automatic texture atlas.** Small images are packed into shared atlas textures (4096x4096),
   reducing texture switches without manual sprite sheet management.

## Weaknesses and Limitations

1. **Higher baseline CPU usage.** An empty Ebitengine window uses 6-8% CPU due to the game loop
   polling at 60 TPS/FPS. This is a known issue (#3318). For comparison, Raylib-go idles at ~10% vs
   Ebiten's ~20% in similar hello-world benchmarks (#1703). For a roguelike that only needs to
   redraw on input, this is wasteful. Mitigation: `ebiten.SetScreenClearedEveryFrame(false)` and
   `SetTPS(ebiten.SyncWithFPS)` reduce overhead for idle apps.
   [Issue #3318](https://github.com/hajimehoshi/ebiten/issues/3318),
   [Issue #1703](https://github.com/hajimehoshi/ebiten/issues/1703)

2. **No built-in ECS or scene management.** Ebiten gives you a blank canvas. You need to build or
   import your own ECS (donburi, ark), scene manager (stagehand), input mapper, etc. This is by
   design but means more upfront wiring.

3. **Vector path rendering is slow.** Drawing many vector paths per frame (e.g., hundreds of filled
   shapes via `vector.DrawFilledRect`) is significantly slower than image-based drawing. For grid
   games, use pre-rendered tile images, not vector primitives.
   [Issue #3275](https://github.com/hajimehoshi/ebiten/issues/3275)

4. **No built-in text console/grid abstraction.** Unlike BearLibTerminal or libtcod, there's no "put
   char at cell (x,y)" API. You have to build this yourself or use ramen. The rendering primitive is
   "draw image onto image at pixel coordinates with transform."

5. **Kage shader limitations.** Fragment shaders only (no vertex or compute shaders). The Go-like
   syntax can be confusing for developers who know GLSL but not vice versa. No multi-pass pipeline
   in a single shader. [quasilyte blog](https://www.quasilyte.dev/blog/post/ebitengine-shaders/)

6. **Context loss handling adds complexity.** Ebitengine records draw commands to restore state
   after GPU context loss (common on mobile). This means certain patterns (cyclic drawing, modifying
   render sources after use) are expensive or disallowed.

7. **Image.At() and ReplacePixels are slow.** Reading pixels back from GPU or bulk-replacing pixels
   causes a pipeline flush. Fine for occasional use, but not for per-frame pixel manipulation.

8. **Not designed for general-purpose GUI.** The maintainer explicitly states it's not oriented
   toward application UI: no text shaping, no system font integration, no accessibility, no native
   widgets. [Discussion #2208](https://github.com/hajimehoshi/ebiten/discussions/2208)

## Tile/Grid Rendering Pattern

The standard approach for roguelike rendering in Ebitengine:

```go
// Load tileset once
tilesImage = ebiten.NewImageFromImage(decodedPNG)

// In Draw(), iterate visible cells
for y := 0; y < viewHeight; y++ {
    for x := 0; x < viewWidth; x++ {
        tile := gameMap[x][y]
        // Pick the glyph from the tileset
        sx := (tile % tilesPerRow) * tileSize
        sy := (tile / tilesPerRow) * tileSize
        sub := tilesImage.SubImage(image.Rect(sx, sy, sx+tileSize, sy+tileSize)).(*ebiten.Image)

        op := &ebiten.DrawImageOptions{}
        op.GeoM.Translate(float64(x*tileSize), float64(y*tileSize))
        // Optional: tint the glyph
        op.ColorScale.Scale(r, g, b, a)
        screen.DrawImage(sub, op)
    }
}
```

All these DrawImage calls from the same `tilesImage` are batched. For a typical 80x50 roguelike grid
(4000 tiles), this results in 1-2 GPU draw calls.
[tiles example](https://github.com/hajimehoshi/ebiten/blob/main/examples/tiles/main.go)

## Key Ecosystem Libraries for Roguelikes

| Library          | Purpose                         | URL                                                                                    |
| ---------------- | ------------------------------- | -------------------------------------------------------------------------------------- |
| ramen            | Console emulator (libtcod-like) | [github.com/BigJk/ramen](https://github.com/BigJk/ramen)                               |
| donburi          | ECS framework                   | [github.com/yohamta/donburi](https://github.com/yohamta/donburi)                       |
| ark              | Archetype ECS                   | [github.com/mlange-42/ark](https://github.com/mlange-42/ark)                           |
| ebitenui         | UI widgets                      | [github.com/ebitenui/ebitenui](https://github.com/ebitenui/ebitenui)                   |
| ebitengine-input | Action-based input mapping      | [github.com/quasilyte/ebitengine-input](https://github.com/quasilyte/ebitengine-input) |
| grid             | 2D grid with pathfinding, FOV   | [github.com/s0rg/grid](https://github.com/s0rg/grid)                                   |
| pathing          | Efficient grid pathfinding      | [github.com/quasilyte/pathing](https://github.com/quasilyte/pathing)                   |
| dngn             | Dungeon map generation          | [github.com/SolarLune/dngn](https://github.com/SolarLune/dngn)                         |
| etxt             | Advanced text rendering         | [github.com/tinne26/etxt](https://github.com/tinne26/etxt)                             |
| egriden          | Grid-based game framework       | [github.com/greenthepear/egriden](https://github.com/greenthepear/egriden)             |

## Notable Games Built with Ebitengine

- **AAAAXY** - Nonlinear 2D puzzle platformer in non-Euclidean geometry.
  [github](https://github.com/divVerent/aaaaxy)
- **Roboden** - Indirect-control RTS about robot colonies.
  [github](https://github.com/quasilyte/roboden-game)
- **OpenDiablo2** - ARPG engine supporting Diablo 2.
  [github](https://github.com/OpenDiablo2/OpenDiablo2)
- **End of Eden** - Slay the Spire-like roguelite, console + GL modes.
  [github](https://github.com/BigJk/end_of_eden)
- **Darktile** - GPU-rendered terminal emulator. [github](https://github.com/liamg/darktile)
- **Monovania** - Metroidvania game. [codeberg](https://codeberg.org/tslocum/monovania)
- **City Limits** - City-building sim. [codeberg](https://codeberg.org/tslocum/citylimits)
- **Worldwide** - GameBoy Color emulator. [github](https://github.com/pokemium/worldwide)

## Sources

- **Kept:**
  - [Ebitengine GitHub](https://github.com/hajimehoshi/ebiten) - Primary source, README, features,
    platform list
  - [Ebitengine Cheat Sheet](https://ebitengine.org/en/documents/cheatsheet.html) - Complete API
    overview
  - [Performance Tips](https://ebitengine.org/en/documents/performancetips.html) - Batching rules,
    optimization guidance
  - [awesome-ebitengine](https://github.com/sedyh/awesome-ebitengine) - Comprehensive ecosystem
    catalog
  - [Kage shader docs](https://ebitengine.org/en/documents/shader.html) - Official shader
    documentation
  - [quasilyte Kage blog](https://www.quasilyte.dev/blog/post/ebitengine-shaders/) - Practical
    shader experience and criticism
  - [Issue #3318](https://github.com/hajimehoshi/ebiten/issues/3318) - Empty app CPU usage analysis
  - [Issue #1703](https://github.com/hajimehoshi/ebiten/issues/1703) - CPU comparison with Raylib
  - [Discussion #2208](https://github.com/hajimehoshi/ebiten/discussions/2208) - Maintainer on GUI
    limitations
  - [BigJk/ramen](https://github.com/BigJk/ramen) - Console emulator for roguelikes
  - [Grogue tutorial](https://callaway.dev/grogue-a-roguelike-tutorial-in-go-part-0/) - Roguelike
    tutorial series
  - [darktile](https://github.com/liamg/darktile) - Terminal emulator proving grid rendering perf
  - [tinne26/efficient-ebitengine](https://github.com/tinne26/efficient-ebitengine) - Deep
    performance guide

- **Dropped:**
  - Jeremy Heckt's blog post about Dark Cave - Personal blog, minimal technical depth
  - Various small roguelike repos (cscazorla, photogabble) - Learning projects, no unique insights
  - DeepWiki auto-generated docs - Derivative of source code, less authoritative than official docs

## Gaps

- **Benchmarks for large tile grids**: No formal benchmarks found for rendering, say, 200x200 tile
  grids with per-cell color tinting. The 10K sprite example and darktile existence suggest it's
  fine, but hard numbers are missing.
- **Comparison with BearLibTerminal**: No direct comparison exists. BearLibTerminal provides a
  cell-based API directly; Ebitengine requires building that abstraction (or using ramen).
  BearLibTerminal is C with bindings; Ebitengine is pure Go.
- **Audio latency for roguelikes**: Not researched since not relevant to the rendering/input focus.
- **v3 roadmap**: No clear information on whether Ebitengine v3 will change the rendering model.
