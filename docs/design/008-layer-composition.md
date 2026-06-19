# ADR 008: Layers, Tile Composition, and Sub-cell Offsets

**Status:** Draft **Date:** 2026-06-19 **Parent:** [ADR 001: Architecture](001-architecture.md)

## Context

To achieve full graphical parity with BearLibTerminal, our grid model must evolve beyond a single
flat plane of characters. We need to support:

1. **Layers (0-255):** Strict Z-ordering for distinct scene elements (e.g., background terrain on
   Layer 0, entities on Layer 1, UI on Layer 2).
2. **Tile Composition (Stacking):** The ability to place multiple sprites/characters into the same
   grid cell without overwriting the previous contents (e.g., placing an item on top of a floor
   tile).
3. **Sub-cell Offsets:** Pixel-level translation (`dx`, `dy`) of individual tiles for smooth
   animations and precise visual positioning without affecting grid logic.

## Decisions & Rust API Guidelines

1. **Layer Management:** The `Terminal` will maintain the active layer state. The `Grid` will be
   updated to hold multiple `Layer` structs. To minimize memory overhead for simple games, layers
   should be allocated sparsely (e.g., a `HashMap<u8, Layer>` or a sparse `Vec<Layer>`).
2. **The `Tile` Abstraction:** We will introduce a `Tile` struct that encapsulates a single drawable
   element: its glyph, style, EGC data, and pixel offset.
3. **Composition Mode:** `Terminal` will introduce a `composition(bool)` state. When `true`, `put`
   and `print` will append to the cell's `Tile` stack rather than replacing the base tile.
4. **Cell Memory Footprint (C-PERF):** A naive `Vec<Tile>` per cell would cause unacceptable memory
   bloat and heap fragmentation. We will use a small-vec approach (e.g., `tinyvec` or custom inline
   storage for 1-2 tiles, spilling to heap only for deep stacks).
5. **Alpha Blending:** The rendering backend will use pre-multiplied alpha blending for compositing
   tiles within a cell and layers on top of each other.

---

## Detailed Implementation Milestones

### M1: The `Tile` and `Layer` Types

**Goal:** Extract the visual data from `Cell` into `Tile`, and wrap `Grid`'s buffer in a `Layer`.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tile {
    pub glyph: char,
    pub style: Style,
    pub offset_x: i16,
    pub offset_y: i16,
    #[cfg(feature = "egc")]
    pub flags: CellFlags,
    #[cfg(feature = "egc")]
    pub extra: Option<Arc<String>>,
}

// A cell is now conceptually a stack of Tiles.
// For performance, we optimize for the 1-tile case.
pub struct Cell {
    base: Tile,
    stack: Option<Box<Vec<Tile>>>, // Allocated only when composition is used
}

pub struct Layer {
    pub width: u16,
    pub height: u16,
    pub buffer: Vec<Cell>,
}
```

### M2: Grid Layering

**Goal:** Update `Grid` to manage multiple layers sparsely.

```rust
pub struct Grid {
    width: u16,
    height: u16,
    layers: Vec<Option<Layer>>, // Index is layer ID. Max 256.
}
```

- Update `Grid::put`, `Grid::get` to take a `layer: u8` parameter.
- Layer 0 is always allocated. Other layers are allocated on demand.
- `Grid::diff` must now diff all layers and yield `(layer, x, y, cell)`.

### M3: Terminal Stateful API

**Goal:** Add `layer`, `composition`, and `put_ext` to `Terminal`.

```rust
impl<B: Backend> Terminal<B> {
    pub fn layer(&mut self, layer: u8) -> &mut Self;
    pub fn composition(&mut self, enabled: bool) -> &mut Self;
    pub fn put_ext(&mut self, x: u16, y: u16, dx: i16, dy: i16, ch: char);
}
```

### M4: Backend Blitting Updates

**Goal:** The `SoftwareBackend` (ADR 007) must be updated to iterate over layers 0..=255, and for
each cell, iterate over its `Tile` stack, applying pre-multiplied alpha blending.
