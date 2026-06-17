# Research: rot.js (ROguelike Toolkit in JavaScript)

## Summary

rot.js is the dominant browser-based roguelike toolkit, written in TypeScript and distributed as ES
modules or a pre-built ES5 bundle via npm (`rot-js`). It provides a canvas-based terminal display
with multiple backends (rect, hex, tile, tile-gl, term), map generators, FOV, pathfinding, lighting,
scheduling, and RNG. It is modeled after libtcod. The library is considered "feature-complete" by
its author and sees limited active development. Its display system works well for turn-based ASCII
games but has documented performance issues with graphical tiles and real-time rendering.

## Language

- **Written in TypeScript** (source in `/src`), compiled to ES2015 modules in `/lib` and a pre-built
  ES5 bundle in `/dist`.
- npm package: `rot-js` (3k+ GitHub stars).
- Can be used from plain JavaScript, TypeScript, or as a `<script>` tag with a global `ROT`
  namespace.
- Works in browsers and Node.js (using the `"term"` backend for terminal output).

## Display / Rendering System

### Architecture

The display uses a **backend pattern** with an abstract `Backend` base class and five
implementations:

| Backend    | Layout value | Renderer                               | Use case                                                       |
| ---------- | ------------ | -------------------------------------- | -------------------------------------------------------------- |
| **Rect**   | `"rect"`     | Canvas 2D `fillText`                   | Default. ASCII/Unicode characters in a grid.                   |
| **Hex**    | `"hex"`      | Canvas 2D                              | Hexagonal grid layout.                                         |
| **Tile**   | `"tile"`     | Canvas 2D `drawImage`                  | Sprite-based tiles from a tileset image.                       |
| **TileGL** | `"tile-gl"`  | WebGL2                                 | GPU-accelerated tile rendering with shader-based colorization. |
| **Term**   | `"term"`     | ANSI escape codes via `process.stdout` | Node.js terminal output.                                       |

### Core API

The main entry point is `ROT.Display`. Minimal setup:

```typescript
const display = new ROT.Display({ width: 80, height: 25 });
document.body.appendChild(display.getContainer());

display.draw(x, y, '@', '#ff0', '#000'); // char, fg, bg
display.drawOver(x, y, '.', '#888', null); // overlay without replacing bg
display.drawText(x, y, 'Hello %c{red}world'); // inline color formatting
display.clear();
```

The draw call is `display.draw(x, y, ch, fg, bg)` where `ch` can be a string or string array (for
overlapping glyphs at the same cell). Display data is stored in a flat map keyed by `"x,y"` strings.

### Dirty Tracking

The display uses a dirty-flag system:

- `false` = nothing to redraw
- `true` = redraw everything
- `object` = set of dirty cell keys

On each `requestAnimationFrame` tick, only dirty cells are redrawn. The `_tick` method calls
`this._backend.schedule(this._tick)` for continuous rendering.

### DisplayOptions

```typescript
interface DisplayOptions {
  width: number; // grid columns (default: 80)
  height: number; // grid rows (default: 25)
  transpose: boolean;
  layout: 'rect' | 'hex' | 'tile' | 'tile-gl' | 'term';
  fontSize: number; // default: 15
  spacing: number; // default: 1
  border: number; // default: 0
  forceSquareRatio: boolean;
  fontFamily: string; // default: "monospace"
  fontStyle: string; // e.g. "bold"
  fg: string; // default foreground: "#ccc"
  bg: string; // default background: "#000"
  tileWidth: number; // default: 32
  tileHeight: number; // default: 32
  tileMap: { [key: string]: [number, number] }; // char -> [x,y] in tileset
  tileSet: HTMLCanvasElement | HTMLImageElement | HTMLVideoElement | ImageBitmap | null;
  tileColorize: boolean; // runtime tinting of tiles
}
```

### Font / Text Rendering (Rect backend)

- Uses Canvas 2D `fillText` with `textAlign: "center"` and `textBaseline: "middle"`.
- Font is set as `"${fontStyle} ${fontSize}px ${fontFamily}"`.
- Cell spacing is calculated from `ctx.measureText("W").width * spacing`.
- `forceSquareRatio` makes cells square by taking `max(spacingX, spacingY)`.
- `computeFontSize(availWidth, availHeight)` auto-calculates font size to fill a container.
- The Rect backend has an optional `cache` mode that pre-renders each unique char+fg+bg combo to an
  offscreen canvas.
- `drawText()` supports inline color formatting: `%c{red}` for foreground, `%b{blue}` for
  background, `%c{}` to reset. Handles CJK full-width characters.

### Tile Rendering

Two tile backends exist:

**Canvas 2D Tiles (`"tile"`):**

- Takes a `tileSet` (spritesheet image) and a `tileMap` mapping characters to `[x, y]` pixel offsets
  in the sheet.
- Each cell can have multiple overlapping tiles by passing an array of characters.
- `tileColorize: true` enables runtime tinting using canvas composite operations (`source-atop` for
  foreground tint, `destination-over` for background). This is slow because each colorized tile
  requires drawing to a temporary canvas and compositing back.

**WebGL Tiles (`"tile-gl"`):**

- Uses WebGL2 with custom vertex/fragment shaders.
- Tileset is uploaded as a texture with `NEAREST` filtering.
- Colorization is done in the fragment shader (tint blending), which is faster than the Canvas 2D
  approach.
- Uses `gl.scissor` for per-cell clearing.
- Falls back to `alert()` on WebGL init failure (not great UX).

**Tile setup example:**

```javascript
const tileSet = document.createElement('img');
tileSet.src = 'tilemap.png';

const display = new ROT.Display({
  layout: 'tile',
  tileWidth: 16,
  tileHeight: 16,
  tileSet: tileSet,
  tileMap: {
    '@': [0, 0], // player at pixel offset (0,0) in sheet
    '.': [16, 0], // floor at (16,0)
    '#': [32, 0], // wall at (32,0)
  },
});
```

### Mouse/Touch Input

`display.eventToPosition(event)` converts DOM mouse/touch events to grid coordinates `[x, y]`,
returning `[-1, -1]` for clicks outside the canvas. It handles canvas scaling (CSS size vs pixel
size) and uses the first touch for multi-touch events.

## Input Handling

rot.js provides **no built-in input system**. It exports:

- **`ROT.KEYS`**: A large constant object mapping `VK_*` names to key codes (e.g., `VK_LEFT: 37`,
  `VK_RETURN: 13`). These are based on deprecated `keyCode` values, not modern `KeyboardEvent.key`
  or `.code`.
- **`ROT.DIRS`**: Directional vectors for 4-way, 8-way, and 6-way (hex) movement. E.g., `DIRS[8][0]`
  is `[0, -1]` (north).

Input handling is entirely the developer's responsibility using standard DOM events:

```javascript
// From the official tutorial
Player.prototype.act = function () {
  Game.engine.lock();
  window.addEventListener('keydown', this);
};

Player.prototype.handleEvent = function (e) {
  var diff = ROT.DIRS[8][keyMap[e.keyCode]];
  // ... move player ...
  window.removeEventListener('keydown', this);
  Game.engine.unlock();
};
```

The engine's `lock()`/`unlock()` pattern enables async turn-based flow: lock when waiting for player
input, unlock when the turn is done.

## Engine / Scheduling

- **`ROT.Scheduler`**: Three variants: `Simple` (round-robin), `Speed` (actors have speed values),
  `Action` (actors return duration of their action).
- **`ROT.Engine`**: Wraps a scheduler, repeatedly calls `actor.act()`. Supports recursive locking
  for async operations.
- The scheduler uses an `EventQueue` (min-heap) internally.

## Map Generation

- **Dungeon**: `Digger` (room-and-corridor), `Rogue` (BSP-like rooms)
- **Cellular**: Cellular automata with configurable birth/survival rules
- **Maze**: `DividedMaze`, `EllerMaze`, `IceyMaze`
- **Arena**: Simple open room
- All generators use a callback `(x, y, value) => void` pattern.

## FOV

- Three algorithms: `DiscreteShadowcasting`, `PreciseShadowcasting`, `RecursiveShadowcasting`
- Callback-based: `fov.compute(cx, cy, radius, callback)` where `callback(x, y, r, visibility)`
  receives visible cells.
- Works with hex grids.

## Pathfinding

- `ROT.Path.AStar` and `ROT.Path.Dijkstra`
- Callback-based passability check: `(x, y) => boolean`
- Supports 4-way, 8-way, and 6-way topologies.

## Games Built With rot.js

### Notable

1. **Untrusted** (8k+ GitHub stars) - A meta-JavaScript adventure game where players edit the game's
   own source code to progress. Uses rot.js for its ASCII display. The most widely known rot.js
   game. [GitHub](https://github.com/AlexNisnevich/untrusted)

2. **The Royal Wedding** - Described as "the most beautiful roguelike" by one developer; cited as
   what drew people to rot.js. [RogueBasin](http://roguebasin.com/index.php/The_Royal_Wedding)

### 7DRL and Other Games

3. **No'hanz** - A turn-based roguelike about traps (polymorph, duplicate, wrath). 7DRL 2019, rated
   4.6/5. [itch.io](https://st33d.itch.io/nohanz)
4. **Blackfeather** - Fantasy dungeon crawler, 7DRL 2015.
   [GitHub](https://github.com/Starstew/Blackfeather)
5. **Lirael's Library** - TypeScript 7DRL 2022. [GitHub](https://github.com/JanLopata/lirael-7drl)
6. **Copy Frogue**, **Lyon's Den**, **Sleeping Beauty**, **Goldfish**, **FunhouseRL**, **RailRL**,
   **SpectRL**, **Fantastic Dungeons**, **FeederRL**, among 20+ others listed on RogueBasin.

## Strengths

1. **Complete roguelike toolkit** - Covers display, map generation, FOV, lighting, pathfinding,
   noise, RNG, and turn scheduling in one package. You can build a playable roguelike without any
   other library.

2. **Modeled after libtcod** - Familiar API for anyone coming from the traditional roguelike
   development community. The cell-based `draw(x, y, ch, fg, bg)` pattern is intuitive for
   grid-based games.

3. **Multiple display backends** - Text, tiles (Canvas 2D and WebGL), hex, and terminal output, all
   behind the same API. Switching from ASCII to graphical tiles requires only changing options, not
   rewriting rendering code.

4. **Interactive manual** - The [interactive manual](https://ondras.github.io/rot.js/manual/) has
   live, editable examples for every feature. This is well-regarded in the community as one of the
   best ways to learn the library.

5. **Hex support throughout** - Hex grids work with the display, FOV, pathfinding, and map
   generation. Not an afterthought.

6. **Small and focused** - No external dependencies. The minified bundle is small. Does one thing
   (roguelike infrastructure) well.

7. **Browser-native** - Just a `<canvas>` element. No WebGL required for the default backend. Works
   in any modern browser.

8. **Proven track record** - Used in hundreds of 7DRL entries and notable games like Untrusted.
   Battle-tested over 10+ years.

## Weaknesses / Limitations

1. **Tile rendering performance is poor** - Colorized Canvas 2D tiles at 80x40 with 8x8 tiles run at
   ~700ms per frame (~1.4 FPS). The `globalCompositeOperation` approach for colorization is
   fundamentally slow. The TileGL backend helps but is not a complete fix.
   [Issue #152](https://github.com/ondras/rot.js/issues/152)

2. **FOV inconsistencies** - Shadowcasting produces different visibility results depending on the
   observer's position relative to the same geometry.
   [Issue #218](https://github.com/ondras/rot.js/issues/218)

3. **Shadowcasting performance for real-time** - `_getCircle` allocates memory on every call. For
   real-time (non-turn-based) games computing FOV every frame, this is a bottleneck. Caching was
   suggested but not implemented. [Issue #110](https://github.com/ondras/rot.js/issues/110)

4. **No input system** - Only provides a `KEYS` constant with deprecated `keyCode` mappings. No
   input manager, no key binding system, no gamepad support. The tutorial recommends raw
   `addEventListener("keydown")` with `handleEvent`.

5. **"Feature-complete" / low maintenance** - The author considers it feature-complete. Issues and
   PRs receive slow responses. The API is stable but there's little evolution. Last significant
   activity was the TypeScript rewrite.

6. **Strictly cell-based display** - No support for smooth scrolling, sub-cell positioning,
   animations, particle effects, or transitions. Every visual element must fit into the grid. No
   concept of layers, cameras, or viewports.

7. **String-key data model** - Display data is stored in a flat object keyed by `"x,y"` strings.
   This works but is not memory-efficient for large maps and involves string allocation/parsing
   overhead.

8. **WebGL backend has rough edges** - TileGL sets `window.gl` as a global (debugging artifact left
   in), uses `alert()` for WebGL initialization failures, and requires WebGL2 with no fallback.

9. **API uses deprecated web standards** - The `KEYS` constant uses `keyCode` values, which are
   deprecated in favor of `KeyboardEvent.key`/`.code`. The `handleEvent` pattern shown in tutorials,
   while valid, is uncommon in modern JS.

10. **No UI primitives** - No built-in support for menus, message logs, health bars, or inventory
    screens. Everything beyond the grid display must be built from scratch or done with DOM elements
    alongside the canvas.

## API Design Notes

- The API is procedural and callback-heavy. Map generators call `callback(x, y, value)` for each
  cell. FOV calls `callback(x, y, r, visibility)` for visible cells.
- The `Engine.lock()`/`unlock()` pattern for async turns is clever but can be confusing. It predates
  async/await and Promises.
- Color formatting in `drawText` uses a custom `%c{color}` syntax rather than something like ANSI
  codes.
- The library exports everything from a single entry point:
  `import { Display, Map, FOV, Path, ... } from "rot-js"`.
- TypeScript types are available but some are loose (e.g., `DisplayData` is a tuple type
  `[number, number, string | string[] | null, string, string]`).

## Sources

- Kept: [GitHub repo](https://github.com/ondras/rot.js) - Primary source for architecture, source
  code, options
- Kept:
  [Source: display.ts](https://raw.githubusercontent.com/ondras/rot.js/master/src/display/display.ts) -
  Core display API, dirty tracking, draw methods
- Kept:
  [Source: types.ts](https://raw.githubusercontent.com/ondras/rot.js/master/src/display/types.ts) -
  DisplayOptions interface
- Kept:
  [Source: tile.ts](https://raw.githubusercontent.com/ondras/rot.js/master/src/display/tile.ts) -
  Canvas 2D tile backend, colorization logic
- Kept:
  [Source: tile-gl.ts](https://raw.githubusercontent.com/ondras/rot.js/master/src/display/tile-gl.ts) -
  WebGL tile backend, shaders
- Kept:
  [Source: rect.ts](https://raw.githubusercontent.com/ondras/rot.js/master/src/display/rect.ts) -
  Text rendering backend, cache mode
- Kept:
  [Source: constants.ts](https://raw.githubusercontent.com/ondras/rot.js/master/src/constants.ts) -
  KEYS, DIRS exports
- Kept: [RogueBasin page](http://roguebasin.com/index.php/Rot.js) - Games list, feature summary
- Kept: [RogueBasin tutorial](https://roguebasin.com/index.php/Rot.js_tutorial) - Official tutorial
  showing input/engine patterns
- Kept: [Issue #152](https://github.com/ondras/rot.js/issues/152) - Tile rendering performance
  (700ms/frame)
- Kept: [Issue #110](https://github.com/ondras/rot.js/issues/110) - FOV performance for real-time
- Kept: [Issue #218](https://github.com/ondras/rot.js/issues/218) - FOV inconsistencies
- Kept: [Untrusted](https://github.com/AlexNisnevich/untrusted) - Most notable rot.js game
- Dropped: digitalthriveai.com tutorial - SEO-heavy rehash of docs, no original content
- Dropped: antineutrino.net blog post - Light overview, already covered by primary sources

## Gaps

- **npm download statistics** were not directly confirmed (npmjs.com page loaded but stats were not
  clearly visible in the fetch).
- **Comparison to alternatives** (e.g., wglt, malwoden, haxe-based libraries) was not researched, as
  it wasn't requested.
- **Performance of the TileGL backend specifically** vs Canvas 2D tiles was not benchmarked; issue
  #152 primarily discusses Canvas 2D tile perf.
- **Community size and activity level** beyond GitHub could not be verified (the Google Groups forum
  link may be dead).
