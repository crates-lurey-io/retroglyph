# Roguelike Algorithms: Research Brief

## Summary

Roguelike games rely on a small, well-understood set of grid algorithms: field of view (FOV),
pathfinding, procedural map generation, noise functions, and line drawing. The Rust ecosystem has
mature crates for each (bracket-lib, pathfinding, noise-rs, fastnoise-lite), but none perfectly fits
a library that wants to be a focused terminal/grid toolkit without taking over the user's
architecture. The recommendation is to ship core geometric primitives (line drawing, grid utilities)
in the main crate and provide optional, trait-gated algorithm modules as separate workspace crates
or feature-gated modules, following the bracket-lib split model but with lighter trait requirements.

---

## 1. Field of View (FOV) Algorithms

FOV determines which tiles are visible from a given origin, accounting for walls. All major
algorithms divide the area into octants or quadrants and scan outward row-by-row.

### 1.1 Recursive Shadowcasting

The most popular roguelike FOV algorithm. Scans each octant row-by-row from the origin outward. When
a wall is hit, the algorithm calculates which cells in farther rows are "in shadow" and recursively
scans only the visible portion.

**Properties:** Fast (especially indoors), simple to implement, good pillar shadows. Not perfectly
symmetric.

### Pseudocode (one octant)

```rust
fn scan(row, start_slope, end_slope, radius, origin, is_opaque, mark_visible):
    if row > radius: return

    for col in range(row * start_slope, row * end_slope):
        tile = transform(row, col)  // map octant-relative to absolute coords

        if is_opaque(tile):
            if not is_opaque(prev_tile):
                // start of a wall section: recurse with narrowed end_slope
                scan(row + 1, start_slope, slope(row, col), ...)
            mark_visible(tile)
        else:
            if is_opaque(prev_tile):
                // end of a wall section: adjust start_slope
                start_slope = slope(row, col)
            mark_visible(tile)

        prev_tile = tile

    // if last tile was open, continue scanning next row
    if not is_opaque(prev_tile):
        scan(row + 1, start_slope, end_slope, ...)
```text

For each of 8 octants, call `scan(1, -1.0, 1.0, radius, ...)`. Slopes are calculated as `col / row`
(or fractional equivalents).

[Source: Bjorn Bergstrom, RogueBasin](http://www.roguebasin.com/index.php/FOV_using_recursive_shadowcasting)

### 1.2 Symmetric Shadowcasting (Albert Ford)

A refinement of recursive shadowcasting that guarantees perfect symmetry between floor tiles: if
floor A can see floor B, then B can always see A. Achieves this by:

- Modeling walls as diamonds inscribed in tiles (not squares)
- Using exact rational arithmetic (fractions) instead of floats
- Adding an `is_symmetric` check: a floor tile is revealed only if its center point falls within the

  scan sector

### Pseudocode (complete, CC0-licensed)

```rust
fn compute_fov(origin, is_blocking, mark_visible):
    mark_visible(origin)
    for quadrant in [North, East, South, West]:
        scan(Row { depth: 1, start_slope: -1, end_slope: 1 })

fn scan(row):
    prev_tile = None
    for tile in row.tiles():
        if is_wall(tile) or is_symmetric(row, tile):
            reveal(tile)
        if is_wall(prev_tile) and is_floor(tile):
            row.start_slope = slope(tile)
        if is_floor(prev_tile) and is_wall(tile):
            next_row = row.next()
            next_row.end_slope = slope(tile)
            scan(next_row)
        prev_tile = tile
    if is_floor(prev_tile):
        scan(row.next())

fn is_symmetric(row, tile):
    col >= row.depth * row.start_slope
    and col <= row.depth * row.end_slope

fn slope(tile):
    Fraction(2 * col - 1, 2 * row_depth)

// Row.tiles(): min_col = round_ties_up(depth * start_slope)
//              max_col = round_ties_down(depth * end_slope)
```rust

An iterative (non-recursive) version replaces the recursion with a stack/queue of rows.

**Properties:** Perfect floor-floor symmetry, expansive walls, expanding pillar shadows, no blind
corners, maps exactly to Bresenham line-of-sight. Comparable performance to standard shadowcasting.

[Source: Albert Ford](https://www.albertford.com/shadowcasting/)

### 1.3 Diamond-Wall Model

Treats walls as diamond shapes inscribed in their tiles. Produces constant-width pillar shadows (a
single line), which prevents stealth behind single-tile pillars. Blocks vision through diagonal
walls (good for games without diagonal movement). Decent symmetry.

**Properties:** Unique diagonal-wall blocking, poor pillar shadow gameplay.
[Source: RogueBasin comparative study](http://www.roguebasin.com/index.php/Comparative_study_of_field_of_view_algorithms_for_2D_grid_based_worlds)

### 1.4 Permissive FOV

A family of algorithms parameterized by "permissiveness" (0-8). Higher values reveal more tiles
through gaps. At permissiveness 8, it is perfectly symmetric with zero error. Lower values behave
more like shadowcasting. The tradeoff: higher permissiveness means worse pillar shadow gameplay but
better symmetry.

**Properties:** Tunable, but no single setting is perfect. More complex to implement than
shadowcasting.

[Source: Jonathon Duerig, Precise Permissive Field of View](http://www.roguebasin.com/index.php/Precise_Permissive_Field_of_View)

### 1.5 FOV Comparative Performance (Jice, 2009)

Benchmarks on various map sizes (libtcod implementations, C++):

| Algorithm  | Indoor 40x40 | Empty 100x100 | Outdoor 100x100 |
| ---------- | ------------ | ------------- | --------------- |
| Shadow     | 32 us        | 383 us        | 309 us          |
| Basic      | 51 us        | 589 us        | 242 us          |
| Diamond    | 67 us        | 925 us        | 318 us          |
| Permissive | 53-60 us     | 585-618 us    | 280-375 us      |
| Digital    | 277 us       | 3958 us       | 4255 us         |

**Conclusion:** Shadowcasting is fastest overall, especially for indoor maps. All algorithms except
Digital are fast enough for typical roguelike use (20x20 to 40x40 visible area). Symmetry is the
main differentiator for gameplay.

[Source: RogueBasin Comparative Study](http://www.roguebasin.com/index.php/Comparative_study_of_field_of_view_algorithms_for_2D_grid_based_worlds)

### 1.6 FOV Recommendation for rg

Implement **symmetric shadowcasting** (Albert Ford's variant) as the default. It has the best
overall properties: perfect symmetry, good performance, correct Bresenham correspondence, and clean
pseudocode to port. Optionally provide classic recursive shadowcasting as a faster alternative for
games that don't need symmetry.

---

## 2. Pathfinding

### 2.1 A\* (A-Star)

The standard shortest-path algorithm for single-source, single-target pathfinding on grids. Uses a
priority queue (binary heap) to explore nodes ordered by `f(n) = g(n) + h(n)` where `g` is
cost-so-far and `h` is a heuristic estimate to the goal.

### Key considerations

- Heuristic choice matters: Manhattan for 4-directional, Chebyshev/octile for 8-directional,

  Euclidean works but is slightly less efficient

- Binary heap is standard; more exotic structures (pairing heap, bucket queue) rarely help for

  typical roguelike map sizes

- bracket-pathfinding defaults to 65,536 iteration limit

### Pseudocode

```rust
fn a_star(start, goal, map):
    open = BinaryHeap::new()
    open.push(start, h(start, goal))
    came_from = {}
    g_score = {start: 0}

    while open is not empty:
        current = open.pop_min()
        if current == goal: return reconstruct_path(came_from, current)

        for (neighbor, cost) in map.get_exits(current):
            tentative_g = g_score[current] + cost
            if tentative_g < g_score.get(neighbor, INF):
                came_from[neighbor] = current
                g_score[neighbor] = tentative_g
                f = tentative_g + h(neighbor, goal)
                open.push(neighbor, f)

    return None  // no path
```text

### 2.2 Dijkstra Maps (Influence Maps / Distance Fields)

Not just pathfinding, but a general-purpose AI tool. A Dijkstra map is a grid where each cell holds
the distance to the nearest goal. Computed via floodfill: set goals to 0, iterate until stable (each
cell = min(neighbors) + 1).

**Uses (from Brogue's Brian Walker):**1.**Pathfinding:** Roll downhill toward any goal from any position

1. **Fleeing AI:** Multiply map by -1.2, re-scan. Monsters flee intelligently toward doors, not into

   corners

1. **Autoexplore:** Set unexplored tiles as goals, roll downhill
2. **Desire-driven AI:** Combine multiple Dijkstra maps (player distance, food, allies, items) with

   per-monster weight coefficients. Nine weighted sums produce complex behavior cheaply

1. **Dynamic routing:**Track distance-to-safety for terrain effects**Properties:** O(n) to compute (n = number of cells). Can be reused across all entities. Only

recompute when goals change.

[Source: Brian Walker, The Incredible Power of Dijkstra Maps](http://www.roguebasin.com/index.php/The_Incredible_Power_of_Dijkstra_Maps)

### 2.3 Jump Point Search (JPS)

An optimization of A\* for uniform-cost grids. Skips "uninteresting" nodes by jumping along straight
lines until hitting a wall or a "forced neighbor" (a tile that could not be reached more efficiently
from another direction). Reduces the number of nodes expanded by 10-30x on open maps.

### Constraints

- Only works on uniform-cost grids (all floor tiles cost 1)
- Requires 8-directional movement (standard JPS; variants exist for 4-dir)
- Does not work with variable terrain costs
- More complex to implement correctly

**When to use:** Large open maps where A*is too slow. Most roguelike maps are small enough that A*
is fine.

### 2.4 Flow Fields

Precompute a direction vector for every cell pointing toward the goal. Essentially a Dijkstra map
where instead of storing distance, you store the direction to roll downhill. Useful when many
entities need to pathfind to the same target simultaneously (RTS-style).

**When to use:** Many entities, same target, large maps. For typical roguelikes with a handful of
monsters, A\* or Dijkstra maps are simpler.

---

## 3. Map Generation

### 3.1 BSP (Binary Space Partition) Dungeons

The most common room-and-corridor dungeon generator:

1. Start with the full map rectangle
2. Recursively split into two sub-rectangles (alternating horizontal/vertical, random split

   position)

1. Stop when leaves are approximately room-sized
2. Place a randomly-sized room in each leaf
3. Connect sibling leaves with corridors (straight or Z-shaped)
4. Walk up the tree connecting parent regions

**Properties:** Guaranteed no overlapping rooms. Clean, traditional dungeon layouts. Controllable
room size distribution (homogeneous vs heterogeneous splits). Easy to add variation by allowing some
rooms to fill entire leaves.

[Source: RogueBasin, Basic BSP Dungeon Generation](http://www.roguebasin.com/index.php/Basic_BSP_Dungeon_generation)

### 3.2 Cellular Automata (Cave Generation)

Generates natural cave-like levels:

1. Fill map randomly (~40-45% walls)
2. Iterate the 4-5 rule: a tile becomes a wall if >= 5 of its 8 neighbors are walls
3. Repeat 4-5 iterations

### Improved rule (prevents large open areas)

```text
W'(p) = R1(p) >= 5 || R2(p) <= 2
```text

Where R1 = neighbor count in 3x3 area, R2 = neighbor count in 5x5 area. Run 4 iterations with this
rule, then 3 iterations with just `R1(p) >= 5` for smoothing.

**The isolated cave problem:** Cellular automata frequently produce disconnected regions. Solutions:

- Flood fill from a random open point, wall off unreached areas, retry if fill is too small
- Connect segments with carved corridors (looks unnatural)
- Horizontal blanking strip before iteration (prevents vertical wall formation)

[Source: RogueBasin, Cellular Automata Method](http://www.roguebasin.com/index.php/Cellular_Automata_Method_for_Generating_Random_Cave-Like_Levels)

### 3.3 Drunkard's Walk (Random Walk)

Simple algorithm for organic cave generation:

1. Start with a map full of walls
2. Place a "drunk" at a random position, mark it as floor
3. Move the drunk in a random cardinal direction
4. Mark the new position as floor
5. Repeat until a target percentage of the map is open (typically 40-50%)

### Variants

- Multiple drunks starting from different positions
- Weighted directions (bias toward center, away from edges)
- Tunneling drunkard (carves corridors of width > 1)

**Properties:** Always produces connected maps (if using a single drunk). Highly organic,
unpredictable shapes. No rooms unless combined with other techniques. Can be slow if the target fill
percentage is high and the drunk wanders over already-open tiles.

### 3.4 Wave Function Collapse (WFC)

A constraint-satisfaction algorithm adapted from quantum mechanics metaphor:

1. Define a tileset with adjacency rules (which tiles can neighbor which, on each side)
2. Start with every cell in a "superposition" of all possible tiles
3. Find the cell with lowest entropy (fewest remaining possibilities)
4. Collapse it to a random valid tile
5. Propagate constraints to neighbors, reducing their possibilities
6. Repeat until all cells are collapsed or a contradiction is found (backtrack/restart)

**Properties:** Produces levels that match a given style/tileset exactly. Very flexible. Slower than
other methods. Can fail and require restarts. Requires careful tileset design.

**Use case:** When you want maps that look like they were hand-designed but are procedurally
generated. Better for overworld/town generation than traditional dungeons.

---

## 4. Noise Functions

### 4.1 Perlin Noise

Ken Perlin's original gradient noise (1983). Generates smooth, continuous pseudorandom values. Used
for terrain heightmaps, texture generation, cloud patterns.

**Properties:** Produces visible grid-aligned artifacts at integer boundaries. Superseded by
improved algorithms but still widely used. Typically combined with fractal Brownian motion (fBm) for
multi-octave detail.

### 4.2 Simplex Noise

Ken Perlin's improved algorithm (2001). Uses a simplex grid (triangles in 2D, tetrahedra in 3D)
instead of a square grid.

**Advantages over Perlin:** Fewer artifacts, better isotropy, lower computational complexity in
higher dimensions (O(n^2) vs O(2^n)), smoother gradients.

**Patent issue:** The 3D implementation was patented (US Patent 6,867,776, expired 2022). This led
to the creation of OpenSimplex alternatives.

### 4.3 OpenSimplex / OpenSimplex2

Patent-free alternatives to simplex noise:

- **OpenSimplex** (2014, Kurt Spencer): Uses a different lattice structure to avoid the simplex

  patent. Slightly different visual character.

- **OpenSimplex2** (2019): Improved version with better performance and visual quality. Two

  variants: OpenSimplex2 (standard) and OpenSimplex2S (smoother).

### 4.4 Noise in Roguelikes

Noise functions are used for:

- Terrain elevation/biome maps (overworld generation)
- Cave feature placement (stalagmites, water pools)
- Temperature/moisture maps for biome selection
- Texture variation in tile rendering
- Cloud/weather effects

Most traditional dungeon-crawling roguelikes don't need noise. It becomes important for overworld or
open-world generation.

---

## 5. Line Drawing

### 5.1 Bresenham's Line Algorithm

The classic integer-only line drawing algorithm. Determines which cells a line from (x0,y0) to
(x1,y1) passes through on a grid. Uses only integer addition, subtraction, and bit shifting.

### Pseudocode (2)

```rust
fn bresenham(x0, y0, x1, y1) -> Vec<(i32, i32)>:
    points = []
    dx = abs(x1 - x0)
    dy = abs(y1 - y0)
    sx = if x0 < x1 then 1 else -1
    sy = if y0 < y1 then 1 else -1
    err = dx - dy

    loop:
        points.push((x0, y0))
        if x0 == x1 and y0 == y1: break
        e2 = 2 * err
        if e2 > -dy:
            err -= dy
            x0 += sx
        if e2 < dx:
            err += dx
            y0 += sy

    return points
```text

**Properties:** Integer-only, fast, deterministic. The standard for line-of-sight checks and
projectile paths. Symmetric shadowcasting (Albert Ford) maps exactly to Bresenham line-of-sight.

### 5.2 DDA (Digital Differential Analyzer)

A floating-point line algorithm that steps through the grid one cell at a time, tracking where the
line crosses cell boundaries.

### Pseudocode (3)

```rust
fn dda(x0, y0, x1, y1) -> Vec<(i32, i32)>:
    dx = x1 - x0
    dy = y1 - y0
    steps = max(abs(dx), abs(dy))
    x_inc = dx / steps
    y_inc = dy / steps
    x, y = x0, y0

    points = []
    for i in 0..=steps:
        points.push((round(x), round(y)))
        x += x_inc
        y += y_inc

    return points
```rust

**Properties:** Simpler to understand, uses floating point. Commonly used in raycasting
(Wolfenstein-style). For grid-based roguelikes, Bresenham is preferred due to integer-only math and
exact correspondence with FOV algorithms.

### 5.3 Supercover / Grid Traversal

A variant that returns ALL cells a line passes through (not just one per row/column). Important for
visibility checks where you need to know if a line clips through a wall tile's corner.

---

## 6. Existing Rust Crates

### 6.1 bracket-lib (bracket-pathfinding, bracket-algorithm-traits, etc.)

The most comprehensive Rust roguelike toolkit. A port of the Roguelike Toolkit (RLTK), split into
workspace crates:

| Crate                      | Contents                                                  |
| -------------------------- | --------------------------------------------------------- |
| `bracket-algorithm-traits` | `BaseMap` and `Algorithm2D` traits                        |
| `bracket-pathfinding`      | A\*, Dijkstra maps, FOV (recursive shadowcasting)         |
| `bracket-geometry`         | Points, lines (Bresenham), distance functions             |
| `bracket-noise`            | Port of FastNoise: Perlin, simplex, cellular, value noise |
| `bracket-random`           | Dice-style RNG (`3d6+12` parsing)                         |
| `bracket-terminal`         | Terminal rendering (OpenGL, WebGL, crossterm, curses)     |
| `bracket-lib`              | Meta-crate re-exporting everything                        |

### Trait design

```rust
// BaseMap: the minimum interface for pathfinding/FOV
trait BaseMap {
    fn is_opaque(&self, idx: usize) -> bool { false }
    fn get_available_exits(&self, idx: usize) -> SmallVec<[(usize, f32); 10]> { SmallVec::new() }
    fn get_pathing_distance(&self, idx1: usize, idx2: usize) -> f32 { 0.0 }
}

// Algorithm2D: grid indexing
trait Algorithm2D: BaseMap {
    fn dimensions(&self) -> Point;
    // Default implementations for point2d_to_index, index_to_point2d, in_bounds
}
```

**Strengths:** Proven in production (many roguelikes, a published book). Well-documented. Modular
workspace. **Weaknesses:** Uses `usize` indices throughout (not typed grid positions). `BaseMap`
conflates pathfinding and FOV concerns into one trait. `SmallVec` in the trait signature is
opinionated. The terminal crate is tightly coupled to its own rendering model.

Downloads: bracket-pathfinding ~180K all-time. Last release v0.8.7.

[Source: crates.io](https://crates.io/crates/bracket-pathfinding),
[GitHub](https://github.com/amethyst/bracket-lib)

### 6.2 pathfinding (evenfurther)

A general-purpose pathfinding library, not roguelike-specific:

### Algorithms included

- A*, Dijkstra, BFS, DFS, IDA*, Fringe search
- Yen's K-shortest paths
- Edmonds-Karp (max flow), Kuhn-Munkres (assignment)
- Topological sort, strongly connected components
- Matrix utilities

**API style:** Functional/closure-based, no trait requirements:

```rust
use pathfinding::prelude::astar;
let result = astar(&start, |p| p.successors(), |p| p.heuristic(), |p| p.is_goal());
```

**Strengths:** Very generic. No opinionated map format. Excellent algorithm coverage beyond just
grid pathfinding. Active maintenance, 4.6M downloads all-time. **Weaknesses:** Not grid-optimized.
No Dijkstra maps (the roguelike kind), no FOV.

[Source: crates.io](https://crates.io/crates/pathfinding)

### 6.3 noise (noise-rs)

The primary Rust noise library. Provides `NoiseFn` trait and composable noise generators:

- Perlin, simplex, OpenSimplex, value noise, worley
- fBm, ridged multi, hybrid multi, billow (fractal combiners)
- Combinators: add, multiply, blend, select, turbulence
- Optional image output (`"images"` feature)

Downloads: ~2M all-time. v0.9.

[Source: crates.io](https://crates.io/crates/noise)

### 6.4 fastnoise-lite

Port of Auburn's FastNoise Lite. Single-file, zero-dependency (unless using `no_std`):

- OpenSimplex2, OpenSimplex2S, Cellular (Voronoi), Perlin, Value, Value Cubic
- Domain warp support
- Multiple fractal types
- `f32` or `f64` via feature flag
- `no_std` support via `libm` feature

Downloads: ~92K all-time. Lighter weight than noise-rs.

[Source: crates.io](https://crates.io/crates/fastnoise-lite)

### 6.5 Other Notable Crates

| Crate          | Purpose                         | Notes                                                                 |
| -------------- | ------------------------------- | --------------------------------------------------------------------- |
| `line_drawing` | Bresenham, Xiaolin Wu, midpoint | Small, focused. Iterator-based API.                                   |
| `bresenham`    | Bresenham's line only           | Minimal, iterator-based.                                              |
| `mapgen`       | Roguelike map generation        | BSP, cellular automata, drunkard walk, prefabs. Built on bracket-lib. |
| `wfc`          | Wave function collapse          | General WFC implementation for grids.                                 |
| `doryen-fov`   | FOV algorithms                  | Port of libtcod's FOV. Standalone.                                    |

---

## 7. Bundled vs Separate: The Design Spectrum

### 7.1 The libtcod Model (Fully Bundled)

libtcod bundles terminal rendering, FOV, pathfinding, noise, BSP, heightmaps, and GUI tools into a
single C library.

**Advantages:**One dependency gets you everything. Consistent API. Good for beginners/game jams.**Disadvantages:** The maintainers themselves concluded it was unsustainable. From libtcod issue

## 147

> "Libtcod's size makes it difficult to port, maintain, and document. It has too many things at
> once."

The library is now being split into `libtcod-fov`, `libtcod-terminal`, `libtcod-pathfinding`,
`libtcod-noise`.

[Source: libtcod/libtcod#147](https://github.com/libtcod/libtcod/issues/147)

### 7.2 The BearLibTerminal Model (Pure Rendering)

BearLibTerminal provides only a terminal-like window with grid rendering and input. No algorithms at
all. Users bring their own FOV, pathfinding, etc.

**Advantages:**Minimal, focused, easy to maintain. Users choose their own algorithms.**Disadvantages:** Higher barrier to entry. Every project reinvents the same FOV/pathfinding code.

### 7.3 The bracket-lib Model (Workspace Crates)

bracket-lib started as a monolith (RLTK) and split into workspace crates. Users can depend on just
`bracket-pathfinding` without pulling in the terminal renderer.

**Advantages:** Pick and choose. Shared traits (`bracket-algorithm-traits`) provide integration
without coupling. **Disadvantages:** The traits still carry some opinions (SmallVec, usize indices).
The "meta-crate" re-export means some users pull everything anyway.

### 7.4 The Ratatui Model (Core + Extensions)

Ratatui (v0.30+) split into `ratatui-core`, `ratatui-widgets`, and backend crates. Core is tiny,
widgets are optional.

**Key insight:** The core crate defines the abstraction (Widget trait, Frame, Buffer), and
everything else is optional. Third-party widget crates can depend on just `ratatui-core`.

[Source: ratatui#1388](https://github.com/ratatui/ratatui/issues/1388)

---

## 8. API Design Patterns

### 8.1 bracket-lib's Trait Approach

```rust
trait BaseMap {
    fn is_opaque(&self, idx: usize) -> bool;
    fn get_available_exits(&self, idx: usize) -> SmallVec<[(usize, f32); 10]>;
    fn get_pathing_distance(&self, idx1: usize, idx2: usize) -> f32;
}

trait Algorithm2D: BaseMap {
    fn dimensions(&self) -> Point;
    // Provided: point2d_to_index, index_to_point2d, in_bounds
}
```

### Critique

- `BaseMap` mixes FOV concerns (`is_opaque`) with pathfinding concerns (`get_available_exits`,

  `get_pathing_distance`). These should be separate traits.

- `SmallVec<[(usize, f32); 10]>` in a public trait is opinionated. An iterator or generic collection

  would be better.

- `usize` indices lose type safety. A `Point` or `GridPos` type is more ergonomic.
- The trait requires implementing all methods even if you only want FOV.

### 8.2 pathfinding Crate's Closure Approach

```rust
let path = astar(
    &start,
    |pos| pos.successors().into_iter(),  // neighbors
    |pos| pos.manhattan_distance(&goal),  // heuristic
    |pos| *pos == goal,                   // goal test
);
```

**Advantages:**Zero trait implementations required. Works with any type. Very flexible.**Disadvantages:** No shared abstraction means no code reuse between algorithms. Each call
re-specifies the map interface.

### 8.3 Recommended Approach for rg

Split concerns into focused traits with minimal requirements:

```rust
/// Core grid trait - just dimensions and indexing
trait Grid {
    fn width(&self) -> u32;
    fn height(&self) -> u32;
    fn contains(&self, pos: Pos) -> bool;
}

/// FOV needs only opacity information
trait FovMap: Grid {
    fn is_opaque(&self, pos: Pos) -> bool;
}

/// Pathfinding needs neighbor/cost information
trait PathMap: Grid {
    type Exits: IntoIterator<Item = (Pos, f32)>;
    fn exits(&self, pos: Pos) -> Self::Exits;
}
```

### Principles

- Use `Pos` (a typed `(i32, i32)` or similar) instead of `usize` indices
- Separate FOV from pathfinding traits
- Use associated types for flexibility (no SmallVec in signatures)
- Provide free functions that take `&impl FovMap` rather than methods on the trait
- Support both trait-based and closure-based APIs

---

## 9. Performance Considerations

### 9.1 FOV Performance

- Shadowcasting is O(n) where n is the number of visible tiles. It naturally skips shadowed areas.
- For typical roguelike visible areas (20x20 to 40x40), all algorithms except Digital FOV complete

  in under 100 microseconds.

- The iterative variant of symmetric shadowcasting avoids recursion overhead (important if the stack

  is constrained, e.g., WASM).

- Use exact integer/fraction arithmetic for slopes to avoid floating-point drift artifacts.

### 9.2 Pathfinding Performance

- A\* with binary heap is O(n log n) but visits far fewer nodes than Dijkstra in practice.
- For typical roguelike maps (<100x100), A\* completes in microseconds.
- Dijkstra maps are O(n) to compute and reusable across all entities per turn.
- JPS provides 10-30x speedup over A\* on large uniform grids but adds implementation complexity.
- bracket-pathfinding uses `SmallVec` for exit lists to avoid heap allocation for the common case

  (<=10 exits).

### 9.3 Map Generation Performance

- BSP and cellular automata are fast enough to generate during level transitions (milliseconds).
- WFC can be slow for large maps due to backtracking. Consider running it in a background thread.
- Drunkard's walk can be slow if the target fill percentage is high; use a step limit.

### 9.4 General Rust Performance Tips

- Represent grids as flat `Vec<T>` with row-major indexing (`y * width + x`). This is

  cache-friendly.

- Avoid `HashMap` for visited sets in pathfinding; use a flat `Vec<bool>` or bitset indexed by

  position.

- Use `#[inline]` on small hot functions (is_opaque, index conversion).
- Consider SIMD for noise generation (fastnoise-lite is already optimized for this).
- The `threaded` feature in bracket-pathfinding uses Rayon for parallel Dijkstra map computation.

---

## 10. Recommendation for rg

### Architecture: Layered Workspace with Optional Algorithm Crates

```text
rg/                          # workspace root
  rg-core/                   # Grid, Pos, Rect, Color, Cell - zero deps
  rg-terminal/               # Terminal rendering (crossterm backend)
  rg-algorithms/             # Optional: FOV, pathfinding, line drawing
    (or split further:)
    rg-fov/                  # FOV only (symmetric shadowcasting)
    rg-pathfinding/          # A*, Dijkstra maps
  rg-mapgen/                 # Optional: BSP, cellular automata, drunkard walk
```rust

### What to include in core

- **Pos, Rect, Direction** types (foundational, used everywhere)
- **Bresenham line drawing** (tiny, essential for LOS checks and projectiles, ~30 lines of code)
- **Grid trait** (width, height, contains) and basic indexing

### What to offer as optional crates

- **FOV:** Symmetric shadowcasting (Albert Ford's algorithm). Small, self-contained, high value.

  Depends only on `rg-core` for `Pos`/`Grid`.

- **Pathfinding:** A*and Dijkstra maps. Moderate value-add since the `pathfinding` crate exists and

  is excellent for A*, but roguelike-style Dijkstra maps are not available elsewhere as a ready-made
  solution.

- **Map generation:** BSP, cellular automata, drunkard walk. These are simple enough that many users

  will want to customize heavily. Provide reference implementations or an `rg-mapgen` examples crate
  rather than a hard dependency.

### What NOT to include

- **Noise:** Defer to `noise-rs` or `fastnoise-lite`. These are mature, well-maintained crates.

  Including noise would be scope creep.

- **WFC:** Too complex, too specialized. Defer to the `wfc` crate.
- **JPS/Flow fields:** Niche optimizations. The `pathfinding` crate covers exotic algorithms.
- **A full terminal emulator:** rg should focus on the grid abstraction. Rendering is a backend

  concern.

### API Design

- Define `FovMap` and `PathMap` as separate traits in `rg-core` (trait definitions only, no

  algorithms)

- Algorithm crates provide free functions:

  `fov::compute(map: &impl FovMap, origin: Pos, radius: u32) -> HashSet<Pos>`

- Also provide closure-based variants for users who don't want to implement traits
- Use `Pos` everywhere (not usize indices)
- Return iterators or allocated collections (let the user choose via `.collect()`)

### Rationale

This follows the "ratatui model" of a small core with optional extensions, avoiding both the libtcod
trap (too much bundled, hard to maintain) and the BearLibTerminal trap (too little, high barrier to
entry). The key algorithms that benefit most from tight grid integration (FOV, Dijkstra maps, line
drawing) are provided, while noise and exotic algorithms are left to specialized crates.

---

## Sources

### Kept

- **Albert Ford, Symmetric Shadowcasting** (<https://www.albertford.com/shadowcasting/>) -

  Definitive reference for symmetric FOV with full pseudocode, CC0 licensed

- **RogueBasin, FOV Comparative Study**

  (<http://www.roguebasin.com/index.php/Comparative_study_of_field_of_view_algorithms_for_2D_grid_based_worlds>) -
  Only rigorous benchmark comparison of FOV algorithms

- **RogueBasin, FOV using Recursive Shadowcasting**

  (<http://www.roguebasin.com/index.php/FOV_using_recursive_shadowcasting>) - Original recursive
  shadowcasting reference by Bjorn Bergstrom

- **RogueBasin, The Incredible Power of Dijkstra Maps**

  (<http://www.roguebasin.com/index.php/The_Incredible_Power_of_Dijkstra_Maps>) - Brian Walker's
  seminal article on Dijkstra map applications in Brogue

- **RogueBasin, Cellular Automata Cave Generation**

  (<http://www.roguebasin.com/index.php/Cellular_Automata_Method_for_Generating_Random_Cave-Like_Levels>) -
  Comprehensive reference with rule tweaking and isolated cave solutions

- **RogueBasin, Basic BSP Dungeon Generation**

  (<http://www.roguebasin.com/index.php/Basic_BSP_Dungeon_generation>) - Standard BSP reference with
  clear step-by-step illustrations

- **bracket-lib GitHub** (<https://github.com/amethyst/bracket-lib>) - Primary Rust roguelike

  toolkit, workspace architecture reference

- **bracket-algorithm-traits docs**

  (<https://docs.rs/bracket-algorithm-traits/latest/bracket_algorithm_traits/>) -
  BaseMap/Algorithm2D trait design reference

- **bracket-pathfinding docs** (<https://crates.io/crates/bracket-pathfinding>) - A\*, Dijkstra, FOV

  implementation details and API patterns

- **pathfinding crate** (<https://crates.io/crates/pathfinding>) - General-purpose pathfinding with

  closure-based API, 4.6M downloads

- **noise-rs** (<https://crates.io/crates/noise>) - Primary Rust noise library, 2M downloads
- **fastnoise-lite** (<https://crates.io/crates/fastnoise-lite>) - Lightweight noise with no_std

  support

- **libtcod issue #147** (<https://github.com/libtcod/libtcod/issues/147>) - Maintainer's rationale

  for splitting the monolith

- **Adam Milazzo, Roguelike Vision Algorithms**

  (<http://www.adammil.net/blog/v125_Roguelike_Vision_Algorithms.html>) - Comprehensive analysis of
  FOV algorithm properties and desirable characteristics

- **Ratatui modularization issue** (<https://github.com/ratatui/ratatui/issues/1388>) - Precedent

  for splitting a Rust TUI library into workspace crates

### Dropped

- **RogueBasin output_libraries.md** (roguebasin GitHub mirror) - General comparison, no algorithm

  depth

- **BearLibTerminal design docs** (foo.wyrd.name) - Describes BearLib's rendering design, not

  algorithmic

- **Generic Rust module/workspace docs** (doc.rust-lang.org, stackoverflow) - General language

  reference, not project-specific

- **libtcod xterm renderer issue #78** - Rendering concern, not algorithm-related

## Gaps

1. **Benchmarks in Rust specifically:** The FOV benchmarks are from 2009 C++ implementations

   (libtcod). Rust-specific benchmarks with modern CPUs would be useful, particularly for comparing
   recursive vs iterative symmetric shadowcasting.

1. **JPS implementation quality in Rust:** No well-maintained standalone JPS crate was found. The

   `pathfinding` crate does not include JPS.

1. **WFC for roguelike dungeons:** Most WFC discussion focuses on image synthesis. Practical

   roguelike dungeon WFC examples with adjacency rule design are sparse.

1. **Drunkard's walk variants:** No single canonical reference. The algorithm is simple enough that

   it's usually described inline in tutorials.

1. **Real-world usage data:** How many Rust roguelikes use bracket-lib vs rolling their own vs

   combining smaller crates is not well documented.
