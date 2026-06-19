# ADR 010: Migrate `rg` to `ixy` and `grixy`

**Status:** Draft **Date:** 2026-06-19

## Context

`ixy` v0.6.0-alpha.5 and `grixy` v0.6.0-alpha.5 have been published. Both are owned crates in the
same family. The overhaul work tracked in `rg/.matan/overhaul-ixy.md` (ixy) and
`grixy/.matan/plan.md` (grixy) is done.

`rg` currently hand-rolls four types that these crates own:

| `rg` type        | Lines | Replaces with            |
| ---------------- | ----- | ------------------------ |
| `grid::Position` | ~25   | `ixy::Pos<u16>` newtype  |
| `grid::Rect`     | ~60   | `ixy::Rect<u16>` alias   |
| `grid::Size`     | ~15   | **kept** (see §3)        |
| `grid::Grid`     | ~280  | `grixy::GridBuf` newtype |

These two migrations are independent enough to be split into two milestones and reviewed separately.
Migrating ixy first (M-A) validates the geometry surface before touching the heavier grid storage
(M-B).

---

## Non-goals

- Migrating `grid::Size` — `ixy::Size` stores `usize` dimensions; `rg` uses `u16` throughout the
  backend interface. The mismatch would ripple into every backend and is not worth the churn.
  Revisit when `ixy::Size` becomes generic.
- Exposing `grixy` traits (`GridRead`, `GridWrite`, etc.) as part of `rg`'s public API — the newtype
  wrapper deliberately hides them.
- Changing the `Backend::draw()` iterator item type — it stays `(Position, &Cell)`.

---

## Dependency versions

```toml
# rg/Cargo.toml
[dependencies]
ixy  = "0.6.0-alpha.5"
grixy = { version = "0.6.0-alpha.5", features = ["alloc", "buffer"] }
```

---

## M-A: Replace `Position` and `Rect` with `ixy` types

### A1. `grid::Position` → newtype over `ixy::Pos<u16>`

`ixy::Pos<u16>` is lexicographic by default (x-primary). `rg` requires row-major (y-primary) for
`Ord`, so a newtype is necessary — a plain re-export would silently break sorting.

```rust
/// A position in the grid, in (x = column, y = row) order.
///
/// Implements [`Ord`] in **row-major** order (y primary, then x), which is the
/// natural ordering for terminal rendering: top-to-bottom, left-to-right within
/// each row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Position(pub ixy::Pos<u16>);

impl Position {
    /// Creates a new position.
    pub const fn new(x: u16, y: u16) -> Self {
        Self(ixy::Pos::new(x, y))
    }

    /// The column (x) coordinate.
    pub const fn x(self) -> u16 { self.0.x }

    /// The row (y) coordinate.
    pub const fn y(self) -> u16 { self.0.y }
}

impl PartialOrd for Position {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Position {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.0.cmp_row_major(&other.0)
    }
}

impl From<(u16, u16)> for Position {
    fn from((x, y): (u16, u16)) -> Self { Self::new(x, y) }
}

impl From<Position> for (u16, u16) {
    fn from(p: Position) -> Self { (p.x(), p.y()) }
}
```

**Call-site changes in `rg`** — `Position` was a plain struct with public fields; the newtype wraps
the same data but fields become methods:

| Before                                | After                            |
| ------------------------------------- | -------------------------------- |
| `Position { x, y }`                   | `Position::new(x, y)`            |
| `pos.x`                               | `pos.x()`                        |
| `pos.y`                               | `pos.y()`                        |
| `Position::default()`                 | unchanged (derive still applies) |
| `let (x, y): (u16, u16) = pos.into()` | unchanged                        |
| `pos.cmp(other)`                      | unchanged (Ord impl preserved)   |

Files touched: `src/grid.rs`, `src/terminal.rs`, `src/backend/mod.rs`, `src/backend/headless.rs`,
`src/backend/crossterm.rs`, `src/backend/software/mod.rs`.

### A2. `grid::Rect` → type alias for `ixy::Rect<u16>`

`ixy::Rect<u16>` stores `(x, y, w, h)` as private fields. All of rg's `Rect` methods have direct
equivalents. Since there are no rg-specific invariants beyond what ixy already enforces, a type
alias is sufficient — no newtype overhead.

```rust
/// A rectangle in the grid.
pub type Rect = ixy::Rect<u16>;
```

**Call-site changes in `rg`**:

| Before                                    | After                                                      |
| ----------------------------------------- | ---------------------------------------------------------- |
| `Rect { x, y, width, height }`            | `Rect::new(x, y, usize::from(width), usize::from(height))` |
| `rect.x`                                  | `rect.left()`                                              |
| `rect.y`                                  | `rect.top()`                                               |
| `rect.width`                              | `rect.width()`                                             |
| `rect.height`                             | `rect.height()`                                            |
| `rect.contains(pos)`                      | `rect.contains_pos(pos.0)`                                 |
| `rect.area() -> u32`                      | `rect.area() as u32` (ixy returns `usize`)                 |
| `rect.top_left() -> Position`             | `Position(rect.top_left())`                                |
| `rect.bottom_right() -> Position`         | `Position(rect.bottom_right())`                            |
| `rect.intersects(other) -> bool`          | `!rect.intersect(other).is_empty()`                        |
| `rect.positions()`                        | `rect.pos_iter().map(Position)`                            |
| `for y in rect.y..(rect.y + rect.height)` | `for y in rect.top()..rect.bottom()`                       |
| `for x in rect.x..(rect.x + rect.width)`  | `for x in rect.left()..rect.right()`                       |

Files touched: `src/grid.rs`, `src/terminal.rs`.

**Free gains from `ixy::Rect<u16>`** after this migration:

- `Rect::contains_rect(other)` — sub-region containment check (no equivalent today)
- `Rect::row_rect(row)` — a 1-row-tall slice of the rectangle
- `Rect::col_rect(col)` — a 1-column-wide slice
- `Rect::intersect(other) -> Rect` — actual intersection rectangle, not just a bool

### A3. Delete dead code

After migration, remove from `src/grid.rs`:

- `struct Position` and all its `impl` blocks
- `struct Rect` and all its `impl` blocks
- Tests for `Position` and `Rect` internals — these now live in `ixy`'s own test suite

Keep: `struct Size`, `struct Grid`, `Cells`, `CellsMut`, `DiffIterator`, `Display for Grid`, all
Grid methods, all Grid tests.

### A4. Update public re-exports

```rust
// src/lib.rs
pub use ixy::{Pos, Rect};           // Rect is now the ixy alias
pub use grid::{Grid, Position, Size};  // Position is our newtype; Size unchanged
```

Downstream users who imported `rg::Rect` directly now get `ixy::Rect<u16>` — they gain methods, no
functionality is removed.

### Acceptance criteria — M-A

- [ ] `cargo check --all-targets --all-features` passes
- [ ] `just check` passes (fmt + clippy + doc)
- [ ] All existing `rg` tests pass
- [ ] `Position` sorts in row-major order (existing test `test_position_ord_row_major` still passes)
- [ ] `Rect` call-sites migrated — no `rect.x`, `rect.y`, `rect.width`, `rect.height` field access
      remains in `src/`
- [ ] No `struct Position` or `struct Rect` definitions remain in `src/grid.rs`
- [ ] `rg::Position`, `rg::Rect` still in public API (`src/lib.rs`)

---

## M-B: Replace `Grid` with a `grixy::GridBuf` newtype

### B1. Coordinate bridge: u16 ↔ usize

`grixy` uses `ixy::Pos<usize>` internally. `rg` uses `Position(ixy::Pos<u16>)`. The bridge is two
small helpers in `src/grid.rs`, not exported:

```rust
// Internal only — not pub
fn to_grixy(pos: Position) -> grixy::core::Pos {
    grixy::core::Pos::new(usize::from(pos.x()), usize::from(pos.y()))
}

fn from_grixy(pos: grixy::core::Pos) -> Position {
    // Terminal grids are bounded by u16; these casts are always safe.
    #[allow(clippy::cast_possible_truncation)]
    Position::new(pos.x as u16, pos.y as u16)
}
```

These only appear at the Grid newtype boundary. No other code in `rg` sees `usize` coordinates.

### B2. The `Grid` newtype

```rust
use grixy::buf::GridBuf;
use grixy::ops::layout::RowMajor;

/// A 2D grid of [`Cell`]s, stored row-major.
///
/// Coordinates are `(x, y)` where `x` is the column (0 = left) and `y` is the
/// row (0 = top). The grid is indexed by [`Position`].
pub struct Grid(GridBuf<Cell, alloc::vec::Vec<Cell>, RowMajor>);
```

The newtype hides `grixy` from rg's public API. All existing `Grid` methods are re-implemented as
thin forwarding wrappers. The `egc` feature methods (`write_grapheme`, `clear_overlap`) remain as
inherent methods on the newtype — they don't belong in `grixy`.

### B3. Method mapping

| `rg::Grid` method         | Delegates to                                                  |
| ------------------------- | ------------------------------------------------------------- | ------ | ------------------------------------------- |
| `Grid::new(w, h)`         | `GridBuf::new(usize::from(w), usize::from(h))`                |
| `width() -> u16`          | `self.0.width() as u16`                                       |
| `height() -> u16`         | `self.0.height() as u16`                                      |
| `get(x, y) -> &Cell`      | `&self.0[to_grixy(Position::new(x, y))]`                      |
| `put(x, y, cell)`         | `self.0[to_grixy(...)] = cell`                                |
| `checked_get(x, y)`       | `self.0.get(to_grixy(...))`                                   |
| `checked_put(x, y, cell)` | `self.0.get_mut(...).map(                                     | c      | { \*c = cell; })`                           |
| `checked_get_mut(x, y)`   | `self.0.get_mut(to_grixy(...))`                               |
| `cells()`                 | `self.0.cells().map(                                          | (p, c) | (from_grixy(p).x(), from_grixy(p).y(), c))` |
| `cells_mut()`             | manual impl — grixy has no `cells_mut()` yet (see §B4)        |
| `clear()`                 | `self.0.clear()` (via `GridWrite`)                            |
| `resize(w, h)`            | `self.0.resize(usize::from(w), usize::from(h))`               |
| `diff(&other)`            | `self.0.diff(&other.0).map(                                   | (p, c) | (from_grixy(p), c))`                        |
| `Index<Position>`         | `self.0[to_grixy(pos)]`                                       |
| `IndexMut<Position>`      | `&mut self.0[to_grixy(pos)]`                                  |
| `Display`                 | delegate to `grixy`'s `Display` via `write!(f, "{}", self.0)` |

### B4. `cells_mut()` — grixy gap

`grixy` currently has no `cells_mut()` equivalent (mutable position-paired iteration). The newtype
must provide it directly against the inner `Vec<Cell>`:

```rust
pub fn cells_mut(&mut self) -> CellsMut<'_> {
    let width = self.0.width();
    CellsMut {
        iter: self.0.as_mut().iter_mut().enumerate(),
        width,
    }
}
```

`GridBuf<T, Vec<T>, L>` already implements `AsMut<[T]>`, so `self.0.as_mut()` gives `&mut [Cell]`.
`CellsMut` struct stays in `src/grid.rs` unchanged.

When `grixy` adds `cells_mut()` or a `GridWriteIter` trait, replace this with the delegation.

### B5. `Debug` on `Grid`

`GridBuf` derives `Debug` but shows internal field names. Override with a custom impl that mirrors
the old output (width, height, and abbreviated buffer):

```rust
impl fmt::Debug for Grid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Grid")
            .field("width", &self.0.width())
            .field("height", &self.0.height())
            .finish_non_exhaustive()
    }
}
```

### B6. Delete dead code

After migration, remove from `src/grid.rs`:

- `struct Grid` and all inherent methods (replaced by the newtype)
- `Cells` struct — only if `grixy::GridIter::cells()` return type is usable; otherwise keep
- `CellsMut` struct — keep until grixy provides `cells_mut()`
- `DiffIterator` enum — replaced by `grixy::ops::GridDiff`'s opaque iterator
- All Grid unit tests that duplicate `grixy`'s own coverage (new/put/get/resize/diff basics)
- Keep: EGC tests (`write_grapheme`, wide-char spacer logic), `test_grid_display`

### B7. Update imports and re-exports

```rust
// src/grid.rs — remove these use statements:
use alloc::vec::Vec;
use core::ops::{Index, IndexMut};  // kept only if newtype still needs them

// src/lib.rs — no change to public surface:
pub use grid::{Grid, Position, Size};
```

### Acceptance criteria — M-B

- [ ] `cargo check --all-targets --all-features` passes
- [ ] `just check` passes
- [ ] All existing `rg` tests pass including EGC tests (`cfg(feature = "egc")`)
- [ ] `insta` snapshot tests still match
- [ ] No `struct Grid { width, height, buffer }` definition remains in `src/`
- [ ] No `DiffIterator` enum remains in `src/`
- [ ] `Grid::diff`, `Grid::resize`, `Grid::cells`, `Grid::checked_*` all covered by tests
- [ ] `cells_mut()` test (`test_grid_cells_mut`) still passes

---

## Breaking surface exposed to downstream

Both milestones are pre-v1.0 (`rg` is on v0.1.0), so breaking changes are permitted. Document in
`CHANGELOG.md`:

| Changed             | Before                                 | After                                        | Notes                                     |
| ------------------- | -------------------------------------- | -------------------------------------------- | ----------------------------------------- |
| `Position` fields   | `pos.x`, `pos.y`                       | `pos.x()`, `pos.y()`                         | Fields were public; now methods           |
| `Rect` constructors | `Rect { x, y, width, height }`         | `Rect::new(x, y, w, h)`                      | `w`/`h` are `usize`, not `u16`            |
| `Rect` field access | `rect.width`, `rect.height`            | `rect.width()`, `rect.height()`              | Public fields → methods                   |
| `Rect::contains`    | takes `Position`                       | takes `Position` via `.0`                    | needs `.0` unwrap if calling ixy directly |
| `Rect::intersects`  | returns `bool`                         | removed; use `!rect.intersect(o).is_empty()` | richer return type                        |
| `Rect::positions`   | returns `impl Iterator<Item=Position>` | `pos_iter().map(Position)`                   | renamed                                   |

---

## Risk register

| Risk                                                                                        | Likelihood | Mitigation                                                                                                   |
| ------------------------------------------------------------------------------------------- | ---------- | ------------------------------------------------------------------------------------------------------------ |
| `ixy::Rect<u16>` `width()`/`height()` return `T` — arithmetic with them needs `usize` casts | Medium     | `rect.width_usize()` / `rect.height_usize()` for indexing paths; `width()` for u16 math                      |
| `clippy::pedantic` rejects `as u16` truncation casts in `from_grixy`                        | Medium     | Use `#[allow(clippy::cast_possible_truncation)]` with a comment; terminal dims are always ≤ u16              |
| `grixy` `diff()` or `resize()` not yet in the alpha                                         | Low        | Both confirmed in grixy 0.6.0-alpha.5; verify with `cargo check` first                                       |
| EGC `write_grapheme` touches internal buffer fields (`self.buffer[idx]`)                    | High       | After M-B, access the buffer via `self.0.as_mut()` or `get_mut()`; plan the index arithmetic before starting |
| `cells_mut()` returning raw enumerate — fragile if grixy changes the buffer layout          | Low        | Documented in §B4 as temporary; add a `// TODO: replace when grixy has cells_mut()` comment                  |

---

## Order of operations

```
M-A: ixy geometry
 ├── A1: Position newtype
 ├── A2: Rect alias + call-site sweep
 ├── A3: Delete Position/Rect dead code
 └── A4: Update re-exports + just check

M-B: grixy Grid (start after M-A is green)
 ├── B1: Add grixy dep; write to_grixy/from_grixy helpers
 ├── B2: Write Grid newtype shell (compiles but methods panic/todo)
 ├── B3: Implement each method one group at a time:
 │    ├── new / width / height
 │    ├── get / put / Index / IndexMut
 │    ├── checked_get / checked_put / checked_get_mut
 │    ├── cells / cells_mut
 │    ├── clear / resize
 │    ├── diff
 │    └── write_grapheme / clear_overlap (egc)
 ├── B4: Delete dead code
 └── B5: just check + run all tests
```

Work on M-B method groups in the order listed — each group is independently testable and the
existing tests provide a safety net after every step.
