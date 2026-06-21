# ADR 008: Layers and Sub-cell Offsets

**Status:**Draft**Date:**2026-06-20**Parent:** [ADR 001: Architecture](001-architecture.md)

## Context

The grid model must evolve beyond a single flat plane of characters. We need:

1. **Layers (0–255):** Strict Z-ordering for distinct scene elements (background terrain on layer 0,

   entities on layer 1, UI on layer 2).

1. **Sub-cell offsets:** Pixel-level translation (`dx`, `dy`) for smooth animations without

   affecting grid logic.

This ADR does **not** add per-cell tile stacking (putting multiple characters in the same cell
without overwriting). Every library surveyed — bracket-lib, doryen-rs, libtcod, ratatui — uses a
flat one-tile-per-cell model and achieves composition through multiple independent layer buffers
blitted in order. BearLibTerminal's `composition` flag was a workaround for its single-layer
architecture; having true layers removes the need for the workaround.

This ADR also does not touch `CrosstermBackend`. Sub-cell offsets have no ANSI terminal
representation; the default `draw_layers` impl provides the correct behaviour (extract layer 0,
ignore higher layers). `SoftwareBackend` (ADR 007) is the primary target.

---

## `alpha-blend` dependency

All compositing uses `alpha-blend` 0.2.1. Bitmap font glyphs are always opaque, so the `source_over`
alpha bug (`a * a` instead of `a * 255`) is not hit until ADR 009 adds RGBA8 sprites. See
`.matan/alpha-blend.md` for the full audit.

```toml
alpha-blend = { version = "0.2", default-features = false, features = ["std"] }
```

---

## Decisions & Rust API Guidelines

1. **The current `Cell` type becomes `Tile`** with two new fields (`dx`, `dy`). The old `cell.rs`

   is renamed to `tile.rs`. `Tile` is _the_ atomic drawable unit — one per cell per layer. No
   wrapper enum, no stack.

1. **No per-cell tile stacking.** Each `LayerBuf` is a flat `Vec<Tile>` where `tiles[i]` corresponds

   to cell `i`. Composition happens at the layer level: write the floor to layer 0, the item to
   layer 1, and the compositor blits layer 1 over layer 0's pixels. Entity tiles are cleared from
   their layer when they move.

1. **Sparse layer allocation.** `Grid` holds `Vec<Option<LayerBuf>>` of length 256. Layer 0 is

   always allocated. Layers 1–255 are allocated on first write. Single-layer games pay zero overhead
   (one `Option<LayerBuf>` is `Some`, the rest are `None`). Identical to the previous design.

1. **`Terminal` gains `active_layer: u8`** (default 0). No `composition` field. Existing `put`,

   `print`, and `put_styled` write to `self.active_layer`. The new `put_offset(x, y, dx, dy, ch)`
   writes to the active layer with pixel offsets.

1. **`Backend::draw`** is renamed to receive `&Tile` instead of `&Cell`. A new `draw_layers` method

   is added with a default impl that extracts layer 0 items, ignores higher layers, and calls
   `draw`. `CrosstermBackend` and `HeadlessBackend` need no modifications.

1. **Diff algorithm:** `Grid::diff` iterates allocated layers 0 → 255, then cells in

   row-major order. A cell is yielded if: (a) the layer is new (no previous entry) — all cells
   yielded, or (b) both frames have the layer and the `Tile` differs. Semantics identical to the
   previous design, but yields `&Tile` directly instead of `&Tile (no stacking)`.

1. **Compositing:** `SoftwareRenderer` blits layers 0 → 255. Layer 0 fills the cell background from

   `style.bg` (opaque). Layers > 0 are composited over whatever is already in the pixel buffer;
   their _background_ is transparent (no bg fill), only the glyph pixels overwrite the layer below.

---

## Detailed Implementation Milestones

### M1: Rename `Cell` to `Tile` and add offsets

**Goal:** Rename `src/cell.rs` → `src/tile.rs`, replace `Cell` with `Tile`, add `dx`/`dy`. No grid
or terminal changes yet — just a rename that compiles.

**File:** `src/tile.rs`

```diff

- // src/cell.rs (deleted)
+ // src/tile.rs

- pub struct Cell {
+ pub struct Tile {

      pub(crate) glyph: char,
      pub(crate) style: Style,

+ pub(crate) dx: i16,    // new
+ pub(crate) dy: i16,    // new

      #[cfg(feature = "egc")]
      pub(crate) flags: CellFlags,
      #[cfg(feature = "egc")]
      pub(crate) extra: Option<Arc<String>>,
  }
```

The public API changes from `Cell` to `Tile`. Since retroglyph is not published, no backward compat is
needed. Update all references in the codebase:

```diff

- pub use cell::Cell;
+ pub use tile::Tile;

```

### Acceptance criteria

- Every reference to `Cell` becomes `Tile` across all files (`src/cell.rs`, `src/grid.rs`,

  `src/terminal.rs`, `src/backend/*.rs`).

- All existing tests pass with zero semantic changes (the new fields are always 0 by default).
- `size_of::<Tile>()` is `size_of::<Cell>() + 4` (two `i16` fields).

**Tests:** The existing `Cell` tests in `src/cell.rs` are moved to `src/tile.rs` and pass unchanged
(since `new()` only takes `glyph` and `style`; `dx` and `dy` default to 0).

---

### M2: `LayerBuf` and multi-layer `Grid`

**Goal:**Replace `Grid`'s single flat `GridBuf<Tile>` with `Vec<Option<LayerBuf>>`.**File:**
`src/grid.rs` (modified)

```rust
/// Buffer for a single layer: a flat grid of one tile per cell.
pub(crate) struct LayerBuf {
    buf: GridBuf<Tile, Vec<Tile>, RowMajor>,
}

impl LayerBuf {
    fn new(width: u16, height: u16) -> Self {
        let n = usize::from(width) * usize::from(height);
        Self {
            buf: GridBuf::from_buffer(alloc::vec![Tile::default(); n], usize::from(width)),
        }
    }
}

pub struct Grid {
    width: u16,
    height: u16,
    /// Indexed by layer ID (0–255). Index 0 is always `Some`.
    /// Unwritten layers are `None` — no allocation until first write.
    layers: Vec<Option<LayerBuf>>,
}

impl Grid {
    pub fn new(width: u16, height: u16) -> Self {
        let mut layers = alloc::vec![None; 256];
        layers[0] = Some(LayerBuf::new(width, height));
        Self { width, height, layers }
    }
}
```

### Layer access

```rust
impl Grid {
    /// Borrow a layer, or None if unallocated.
    pub(crate) fn layer(&self, id: u8) -> Option<&LayerBuf> {
        self.layers[usize::from(id)].as_ref()
    }

    /// Borrow mutably, allocating the layer if needed.
    pub(crate) fn layer_or_alloc(&mut self, id: u8) -> &mut LayerBuf {
        let idx = usize::from(id);
        if self.layers[idx].is_none() {
            self.layers[idx] = Some(LayerBuf::new(self.width, self.height));
        }
        self.layers[idx].as_mut().unwrap()
    }
}
```

### Existing `put` and `get` — unchanged signatures, now write to layer 0

```rust
impl Grid {
    pub fn put(&mut self, x: u16, y: u16, tile: Tile) {
        let pos = to_grixy_pos(Pos::new(x, y));
        assert!(self.layer(0).unwrap().buf.contains(pos),
            "coordinates out of bounds: ({x}, {y})");
        self.layers[0].as_mut().unwrap().buf[pos] = tile;
    }

    pub fn get(&self, x: u16, y: u16) -> &Tile {
        &self.layers[0].as_ref().unwrap().buf[to_grixy_pos(Pos::new(x, y))]
    }
}
```

(`checked_put`, `checked_get`, `checked_get_mut` follow the same pattern — all forward to layer 0.)

### New multi-layer methods

```rust
impl Grid {
    /// Write a tile to `layer` at `(x, y)`. Allocates the layer if necessary.
    /// Returns None if `(x, y)` is out of bounds.
    pub fn put_tile(&mut self, layer: u8, x: u16, y: u16, tile: Tile) -> Option<()> {
        let pos = to_grixy_pos(Pos::new(x, y));
        let lb = self.layer_or_alloc(layer);
        if !lb.buf.contains(pos) {
            return None;
        }
        lb.buf[pos] = tile;
        Some(())
    }

    /// Read a tile on `layer` at `(x, y)`, or None if bounds/unallocated.
    pub fn get_tile(&self, layer: u8, x: u16, y: u16) -> Option<&Tile> {
        let pos = to_grixy_pos(Pos::new(x, y));
        self.layer(layer)?.buf.get(pos)
    }

    /// Clear a specific layer (resets all tiles to default).
    pub fn clear(&mut self, layer: u8) {
        if let Some(lb) = self.layers[usize::from(layer)].as_mut() {
            lb.buf.clear();
        }
    }

    /// Clear all allocated layers.
    pub fn clear_all(&mut self) {
        for lb in self.layers.iter_mut().flatten() {
            lb.buf.clear();
        }
    }

    /// Iterate cells on a specific layer.
    pub fn cells(&self, layer: u8) -> Option<Cells<'_>> {
        let lb = self.layer(layer)?;
        Some(Cells {
            iter: lb.buf.as_ref().iter().enumerate(),
            width: usize::from(self.width),
        })
    }
}
```

**`clear()` clears only layer 0** (matching BearLibTerminal semantics):

```diff
  pub fn clear(&mut self) {

- self.buf.clear();
+ self.clear(0);

  }
```

### Multi-layer diff

```rust
impl Grid {
    /// Yield (layer_id, Pos, &Tile) for every changed cell across all layers.
    ///
    /// Order: layer-major (0 → 255), then row-major within each layer.
    pub fn diff<'a>(&'a self, other: &'a Self)
        -> impl Iterator<Item = (u8, Pos, &'a Tile)> + 'a
    {
        (0u8..=255).flat_map(move |id| {
            let cur = self.layer(id);
            let prev = other.layer(id);
            match (cur, prev) {
                (None, _) => Either::Left(core::iter::empty()),
                (Some(c), None) => Either::Right(Either::Left(
                    c.buf.as_ref().iter().enumerate().map(move |(i, tile)| {
                        let x = (i % usize::from(self.width)) as u16;
                        let y = (i / usize::from(self.width)) as u16;
                        (id, Pos::new(x, y), tile)
                    }),
                )),
                (Some(c), Some(p)) => Either::Right(Either::Right(
                    c.buf.diff(&p.buf).map(move |(pos, tile)| {
                        (id, from_grixy_pos(pos), tile)
                    }),
                )),
            }
        })
    }
}
```

**`resize`** iterates all allocated layers:

```diff
  pub fn resize(&mut self, width: u16, height: u16) {
      self.width = width;
      self.height = height;

+ for lb in self.layers.iter_mut().flatten() {
+ lb.buf.resize(usize::from(width), usize::from(height));
+ }

  }
```

**Acceptance criteria** (identical to previous design minus stacking):

- Freshly created `Grid`: layer 0 is `Some`, layers 1–255 are `None`.
- `put_tile(5, 0, 0, tile)` allocates layer 5; layer 6 remains `None`.
- `diff` against an identical grid yields zero items.
- Changing one tile on layer 0 yields exactly 1 tuple with `layer_id == 0`.
- A newly allocated layer 3 against a previous grid without it yields `width * height` tuples.
- Tuples are ordered layer-major: all layer-0 changes before all layer-1 changes.
- `resize` preserves cell content on all allocated layers.

### Tests (replacing prior composition/stack tests with multi-layer tests)

```rust
#[test]
fn grid_layer_zero_always_allocated() {
    let g = Grid::new(5, 5);
    assert!(g.layer(0).is_some());
    assert!(g.layer(1).is_none());
}

#[test]
fn grid_put_tile_allocates_layer() {
    let mut g = Grid::new(5, 5);
    g.put_tile(3, 0, 0, Tile::new('@', Style::default()));
    assert!(g.layer(3).is_some());
    assert!(g.layer(4).is_none());
}

#[test]
fn grid_diff_empty_when_identical() {
    let g = Grid::new(5, 5);
    let prev = Grid::new(5, 5);
    assert_eq!(g.diff(&prev).count(), 0);
}

#[test]
fn grid_diff_reports_changed_cell() {
    let mut cur = Grid::new(5, 5);
    let prev = Grid::new(5, 5);
    cur.put_tile(0, 2, 3, Tile::new('X', Style::default()));
    let diffs: Vec<_> = cur.diff(&prev).collect();
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].0, 0);
    assert_eq!(diffs[0].1, Pos::new(2, 3));
    assert_eq!(diffs[0].2.glyph, 'X');
}

#[test]
fn grid_diff_new_layer_yields_all_cells() {
    let mut cur = Grid::new(3, 4);
    let prev = Grid::new(3, 4);
    cur.put_tile(1, 0, 0, Tile::new('A', Style::default()));
    let diffs: Vec<_> = cur.diff(&prev).collect();
    assert_eq!(diffs.len(), 12);
    assert!(diffs.iter().all(|(l, _, _)| *l == 1));
}

#[test]
fn grid_diff_layer_major_order() {
    let mut cur = Grid::new(3, 3);
    let prev = Grid::new(3, 3);
    cur.put_tile(2, 0, 0, Tile::new('B', Style::default()));
    cur.put_tile(0, 1, 0, Tile::new('A', Style::default()));
    let layers: Vec<u8> = cur.diff(&prev).map(|(l, _, _)| l).collect();
    assert_eq!(layers[0], 0);
    assert!(layers[1..].iter().all(|&l| l == 2));
}

#[test]
fn grid_put_and_get_on_layer_2() {
    let mut g = Grid::new(5, 5);
    g.put_tile(2, 1, 1, Tile::new('Z', Style::default()));
    assert_eq!(g.get_tile(2, 1, 1).unwrap().glyph, 'Z');
    assert!(g.get_tile(2, 2, 2).unwrap().glyph != 'Z'); // default
    assert!(g.get_tile(3, 0, 0).is_none()); // unallocated
}
```

**The existing `Grid` tests** (`test_grid_new`, `test_grid_put_get`, etc.) continue to pass
unchanged, exercising the layer-0 path.

---

### M3: Terminal stateful API

### Goal:**Add `layer()`, `put_offset()` to `Terminal`. No `composition` field.**New field

```diff
  pub struct Terminal<B: Backend> {
      current: Grid,
      previous: Grid,
      backend: B,
      drawing_style: Style,
      queued_event: Option<Event>,

+ active_layer: u8,   // default 0

  }
```

### New methods

```rust
impl<B: Backend> Terminal<B> {
    /// Sets the active drawing layer (0–255). Returns `&mut Self` for chaining.
    pub fn layer(&mut self, layer: u8) -> &mut Self {
        self.active_layer = layer;
        self
    }

    /// Places `ch` at `(x, y)` with pixel offset `(dx, dy)`, using the current
    /// style and active layer.
    pub fn put_offset(&mut self, x: u16, y: u16, dx: i16, dy: i16, ch: char) {
        let tile = Tile {
            glyph: ch,
            style: self.drawing_style,
            dx,
            dy,
            ..Tile::default()
        };
        self.current.put_tile(self.active_layer, x, y, tile);
    }
}
```

**`put` and `print`**call `put_offset(x, y, 0, 0, ch)` internally.**`clear()`** clears layer 0 only:

```diff
  pub fn clear(&mut self) {

- self.current.clear();
+ self.current.clear(0);

  }
```

### New layer-aware `clear` methods

```rust
impl<B: Backend> Terminal<B> {
    pub fn clear(&mut self, layer: u8) {
        self.current.clear(layer);
    }

    pub fn clear_all(&mut self) {
        self.current.clear_all();
    }
}
```

### Updated `present()`

```diff
  pub fn present(&mut self) {

- let diff = self.current.diff(&self.previous);
- self.backend.draw(diff);
+ let diff = self.current.diff(&self.previous);
+ self.backend.draw_layers(diff);

      self.backend.flush();
      core::mem::swap(&mut self.current, &mut self.previous);
  }
```

### Acceptance criteria (2)

- `term.layer(2).put(0, 0, '@')` writes to layer 2; layer 0 at `(0,0)` is unchanged.
- `term.put_offset(1, 1, -4, 2, 'X')` stores a tile with `dx = -4, dy = 2`.
- `clear()` resets layer 0 only; a tile on layer 1 survives.
- After `present()`, `active_layer` is unchanged.

**Tests:**

```rust
#[test]
fn terminal_layer_routes_to_correct_layer() {
    let mut term = Terminal::new(Headless::new(5, 5));
    term.layer(1).put(0, 0, 'A');
    assert_eq!(term.grid().get_tile(0, 0, 0).unwrap().glyph, ' ');
    assert_eq!(term.grid().get_tile(1, 0, 0).unwrap().glyph, 'A');
}

#[test]
fn terminal_put_offset_stores_offset() {
    let mut term = Terminal::new(Headless::new(5, 5));
    term.put_offset(1, 1, -4, 2, 'X');
    let tile = term.grid().get_tile(0, 1, 1).unwrap();
    assert_eq!(tile.glyph, 'X');
    assert_eq!(tile.dx, -4);
    assert_eq!(tile.dy, 2);
}

#[test]
fn terminal_clear_only_affects_layer_zero() {
    let mut term = Terminal::new(Headless::new(5, 5));
    term.layer(1).put(0, 0, 'Z');
    term.layer(0).put(0, 0, 'A');
    term.clear();
    assert_eq!(term.grid().get_tile(0, 0, 0).unwrap().glyph, ' ');
    assert_eq!(term.grid().get_tile(1, 0, 0).unwrap().glyph, 'Z');
}

#[test]
fn terminal_active_layer_unchanged_after_present() {
    let mut term = Terminal::new(Headless::new(5, 5));
    term.layer(3);
    term.present();
    assert_eq!(term.active_layer, 3);
}
```

---

### M4: Backend trait evolution and SoftwareBackend compositing

### Goal:**Update `Backend` trait and `SoftwareBackend` to handle multiple layers.**Updated `Backend` trait (`src/backend/mod.rs`)

```rust
pub trait Backend {
    /// Draw changed cells on layer 0 (legacy path).
    fn draw<'a, I>(&mut self, content: I)
    where
        I: Iterator<Item = (Pos, &'a Tile)>;

    /// Draw changed cells across all layers.
    ///
    /// Default implementation: route layer 0 tiles to `draw`,
    /// ignore higher layers. Override to support layering, sub-cell
    /// offsets, or multi-layer compositing.
    fn draw_layers<'a, I>(&mut self, content: I)
    where
        I: Iterator<Item = (u8, Pos, &'a Tile)>,
    {
        self.draw(content.filter_map(|(layer, pos, tile)| {
            if layer == 0 { Some((pos, tile)) } else { None }
        }));
    }

    // flush, size, resize, clear, poll_event, set_cursor_visible,
    // set_cursor_position — all unchanged.
}
```

Key difference from the previous design: no `Tile (no stacking)` to unwrap. The default impl
directly passes `&Tile` to `draw`, which is exactly what existing backends consume.

**`HeadlessBackend`** stores tiles directly — no change in logic, just the type from `Cell` to
`Tile`. Its `format_view` now reads `.glyph` from `Tile` instead of `Cell`:

```diff
  impl Backend for Headless {
      fn draw<'a, I>(&mut self, content: I)
      where

- I: Iterator<Item = (Pos, &'a Cell)>,
+ I: Iterator<Item = (Pos, &'a Tile)>,

      {
          for (pos, cell) in content {
              self.grid.checked_put(pos.x, pos.y, cell.clone());
          }
      }
  }
```

**`CrosstermBackend`** — same change:

```diff
      fn draw<'a, I>(&mut self, content: I)
      where

- I: Iterator<Item = (Pos, &'a Cell)>,
+ I: Iterator<Item = (Pos, &'a Tile)>,

      {
          for (pos, cell) in content {
              // ... cell.style.fg, cell.style.bg, cell.glyph, cell.flags ...
          }
      }
```

No code changes inside the method body — the field names are identical. The default `draw_layers`
impl routes layer 0 through `draw`, so `CrosstermBackend` never sees layers > 0. This is correct
behaviour for an ANSI terminal.

**`SoftwareRenderer::draw_layers`** — the only backend that overrides:

```rust
#[cfg(feature = "software")]
fn draw_layers<'a, I>(&mut self, content: I)
where
    I: Iterator<Item = (u8, Pos, &'a Tile)>,
{
    let font = self.options.font.as_ref().expect("font missing");
    let scale = usize::from(self.options.scale);
    let cell_w = usize::from(font.glyph_width) * scale;
    let cell_h = usize::from(font.glyph_height) * scale;
    let buf_w = usize::from(self.options.cols) * cell_w;
    let inner = self.inner.as_mut().expect("draw called outside game thread");

    for (layer_id, pos, tile) in content {
        let px_x = usize::from(pos.x) * cell_w;
        let px_y = usize::from(pos.y) * cell_h;

        if layer_id == 0 {
            // Fill solid background from style.bg (opaque).
            let bg = resolve_bg_color(tile.style.bg);
            for y in 0..cell_h {
                let row_start = (px_y + y) * buf_w + px_x;
                let row_end = row_start + cell_w;
                inner.pixel_buf[row_start..row_end].fill(bg);
            }
        }

        // Blit the glyph (1-bit bitmap font). Higher layers don't fill background.
        blit_tile(&mut inner.pixel_buf, buf_w, px_x, px_y, tile, font, cell_w, cell_h, scale);
    }
}
```

The `blit_tile` function is unchanged from the current code except it uses `tile.dx` and `tile.dy`
for sub-cell offset (previously ignored). Portable across all layers — glyph pixels overwrite
whatever is in the buffer, whether background (layer 0) or a lower layer's glyph (layers > 0).

### Acceptance criteria (3)

- A tile on layer 0 with no other layers: pixel output identical to current `blit_cell`.
- A tile on layer 1 placed over a layer-0 tile: layer-1 glyph pixels overwrite layer-0 pixels

  in the buffer.

- A tile with `dx = -2` at cell (1,0): glyph pixels shifted 2px left, no panic.
- `CrosstermBackend` and `HeadlessBackend` compile without modification.

**Tests:**

```rust
#[test]
fn blit_tile_opaque_writes_fg_color() {
    // Existing test, now exercises Tile instead of Cell.
}

#[test]
fn blit_tile_sub_cell_offset_shifts_pixels() {
    // Place a tile with dx=+2, scale=1. Verify glyph pixels start 2px
    // to the right of the cell's left boundary.
}

#[test]
fn layer_zero_fills_background_higher_layers_do_not() {
    // Two layers. Layer 0 paints bg; layer 1 paints only glyph pixels
    // over the existing buffer content.
}
```

---

## Migration notes

- `Cell` is removed; all uses become `Tile`. Since retroglyph is not published and has no downstream

  dependents, no backward compat is needed.

- `Backend::draw` signature changes from `&Cell` to `&Tile`. Existing backend implementations need

  a one-character type change; the field accessors inside are identical (`glyph`, `style`, `flags`,
  `extra`).

- `Backend::draw_layers` is additive with a default impl; existing backends compile without changes.
- `Terminal::put` / `print` / `put_styled` are behaviourally identical for callers that never call

  `layer()` or `put_offset()`.

- `Grid::cells()` iterates layer 0; `Grid::cells(layer)` for explicit layer access.
