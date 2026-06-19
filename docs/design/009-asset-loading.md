# ADR 009: Tilesets, Sprite Sheets, and Asset Loading

**Status:** Draft **Date:** 2026-06-19 **Parent:** [ADR 001: Architecture](001-architecture.md)

## Context

With `SoftwareBackend` (ADR 007) handling TTF fonts and ADR 008 handling the composition of
elements, we now need a mechanism to load retro `.png` sprite sheets, map them to specific Unicode
points or CP437, and configure how many cells a sprite occupies.

## Decisions & Rust API Guidelines

1. **Typed Configuration (C-BUILDER):** Unlike BearLibTerminal's string-based
   `terminal_set("0xE000: tileset.png, size=16x16")`, we will use a strictly typed builder API for
   defining tilesets.
2. **Texture Atlas:** The backend will manage an internal Texture Atlas. Tilesets loaded by the user
   will be packed or referenced as slices of this atlas.
3. **Codepage Mapping:** We will provide built-in support for mapping standard CP437 layouts to
   Unicode sequences.
4. **Multi-cell Spacing:** Tilesets can define `spacing_x` and `spacing_y` to indicate that a sprite
   spans multiple grid cells (e.g., a 32x32 sprite on a 16x16 grid uses `spacing=2x2`).

---

## Detailed Implementation Milestones

### M1: Tileset Configuration API

**Goal:** Define the structures for configuring a tileset in `src/backend/software/config.rs`.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codepage {
    Ascii,
    Cp437,
    Custom(&'static [char]),
}

#[derive(Debug, Clone)]
pub struct TilesetOptions {
    pub bytes: Vec<u8>,
    pub start_codepoint: char,
    pub tile_width: u16,
    pub tile_height: u16,
    pub columns: u16,
    pub codepage: Codepage,
    pub spacing_cells_x: u16,
    pub spacing_cells_y: u16,
}

impl TilesetOptions {
    // Builder pattern methods...
}
```

### M2: Texture Atlas & Loading

**Goal:** Update the backend to decode `.png` images (using the `image` crate) and store them.

- Depend on `image` with `png` feature.
- Extract individual tiles based on `tile_width` and `tile_height`.
- Map them to the internal `GlyphCache` or a new `SpriteCache` using the specified `start_codepoint`
  and `Codepage` mapping.

### M3: Drawing Multi-cell Sprites

**Goal:** During `blit_grid`, when encountering a tile that spans multiple cells, draw the entire
sprite.

- If `spacing_cells > 1`, the anchor cell holds the tile.
- The `SoftwareBackend` will draw the pixel data expanding beyond the cell boundary.
- Combined with ADR 008 Layers, this ensures large sprites do not get overwritten by neighboring
  cells on the same layer.
