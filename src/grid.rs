//! The grid container.

#[cfg(feature = "egc")]
use crate::style::Style;
use crate::tile::Tile;
#[cfg(feature = "egc")]
use crate::tile::TileFlags;
#[cfg(feature = "egc")]
use crate::tile::cap_grapheme;
use alloc::vec::Vec;
use core::fmt;
use core::ops::{Index, IndexMut};
use grixy::buf::GridBuf;
use grixy::ops::layout::RowMajor;
use grixy::ops::{ExactSizeGrid, GridDiff, GridRead, GridWrite};

/// Size of the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct Size {
    /// Width.
    pub width: u16,
    /// Height.
    pub height: u16,
}

/// Pos in the grid, in (x = column, y = row) order.
///
/// Implements [`Ord`] in row-major order (y primary, then x), which is the
/// natural ordering for terminal rendering: top-to-bottom, left-to-right within
/// each row.
pub type Pos = ixy::Pos<u16>;

/// Rectangle in the grid.
pub type Rect = ixy::Rect<u16>;

impl From<(u16, u16)> for Size {
    fn from((width, height): (u16, u16)) -> Self {
        Self { width, height }
    }
}

impl From<Size> for (u16, u16) {
    fn from(s: Size) -> Self {
        (s.width, s.height)
    }
}

// ---------------------------------------------------------------------------
// Helpers: coordinate conversion between u16 and usize
// ---------------------------------------------------------------------------

fn to_grixy_pos(pos: Pos) -> grixy::core::Pos {
    grixy::core::Pos::new(usize::from(pos.x), usize::from(pos.y))
}

#[allow(clippy::missing_const_for_fn)]
fn from_grixy_pos(pos: grixy::core::Pos) -> Pos {
    #[allow(clippy::cast_possible_truncation)]
    Pos::new(pos.x as u16, pos.y as u16)
}

// ---------------------------------------------------------------------------
// Grid iterators
// ---------------------------------------------------------------------------

/// Iterator over all cells with their `(x, y)` coordinates.
pub struct Cells<'a> {
    iter: core::iter::Enumerate<core::slice::Iter<'a, Tile>>,
    width: usize,
}

impl<'a> Iterator for Cells<'a> {
    type Item = (u16, u16, &'a Tile);

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|(i, tile)| {
            #[allow(clippy::cast_possible_truncation)]
            let x = (i % self.width) as u16;
            #[allow(clippy::cast_possible_truncation)]
            let y = (i / self.width) as u16;
            (x, y, tile)
        })
    }
}

/// Mutable iterator over all cells with their `(x, y)` coordinates.
pub struct CellsMut<'a> {
    iter: core::iter::Enumerate<core::slice::IterMut<'a, Tile>>,
    width: usize,
}

impl<'a> Iterator for CellsMut<'a> {
    type Item = (u16, u16, &'a mut Tile);

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|(i, tile)| {
            #[allow(clippy::cast_possible_truncation)]
            let x = (i % self.width) as u16;
            #[allow(clippy::cast_possible_truncation)]
            let y = (i / self.width) as u16;
            (x, y, tile)
        })
    }
}

// ---------------------------------------------------------------------------
// LayerBuf — a single layer's flat buffer
// ---------------------------------------------------------------------------

/// A single layer in the grid: a flat 2D buffer of one tile per cell.
///
/// Layer 0 is always allocated. Layers 1–255 are allocated on first write
/// (see [`Grid::put_tile`]).
pub(crate) struct LayerBuf {
    pub(crate) buf: GridBuf<Tile, Vec<Tile>, RowMajor>,
}

impl LayerBuf {
    fn new(width: u16, height: u16) -> Self {
        let n = usize::from(width) * usize::from(height);
        Self {
            buf: GridBuf::from_buffer(alloc::vec![Tile::default(); n], usize::from(width)),
        }
    }
}

// ---------------------------------------------------------------------------
// Grid
// ---------------------------------------------------------------------------

/// The main grid container for the terminal.
///
/// Holds up to 256 layers (0–255). Layer 0 is always allocated; higher layers
/// are allocated on first write. Single-layer games pay no overhead — layers
/// 1+ remain `None` until used.
///
/// Note: This uses `alloc::vec::Vec`, requiring an allocator in `no_std` environments.
/// For strictly static, no-alloc environments, a static-sized grid type may be added in the future.
pub struct Grid {
    width: u16,
    height: u16,
    /// Indexed by layer ID (0–255). Index 0 is always `Some`.
    /// Unwritten layers are `None` — no allocation until first write.
    layers: Vec<Option<LayerBuf>>,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

impl Grid {
    /// Borrow a specific layer, or `None` if unallocated.
    fn layer(&self, id: u8) -> Option<&LayerBuf> {
        self.layers[usize::from(id)].as_ref()
    }

    /// Borrow a specific layer mutably, allocating it if necessary.
    fn layer_or_alloc(&mut self, id: u8) -> &mut LayerBuf {
        let idx = usize::from(id);
        if self.layers[idx].is_none() {
            self.layers[idx] = Some(LayerBuf::new(self.width, self.height));
        }
        self.layers[idx].as_mut().unwrap()
    }

    /// Borrow layer 0 (always allocated).
    fn layer0(&self) -> &LayerBuf {
        // SAFETY: layer 0 is always `Some` (set in `new`).
        self.layers[0].as_ref().unwrap()
    }

    /// Borrow layer 0 mutably (always allocated).
    fn layer0_mut(&mut self) -> &mut LayerBuf {
        self.layers[0].as_mut().unwrap()
    }
}

// ---------------------------------------------------------------------------
// Grid — public API (all forward to layer 0)
// ---------------------------------------------------------------------------

impl Grid {
    /// Creates a new grid of the given dimensions.
    ///
    /// Layer 0 is allocated immediately. Layers 1–255 are `None` until first
    /// write via [`put_tile`](Self::put_tile).
    #[must_use]
    pub fn new(width: u16, height: u16) -> Self {
        let mut layers = alloc::vec![];
        layers.resize_with(256, || None);
        layers[0] = Some(LayerBuf::new(width, height));
        Self {
            width,
            height,
            layers,
        }
    }

    /// Returns the width of the grid.
    #[must_use]
    pub const fn width(&self) -> u16 {
        self.width
    }

    /// Returns the height of the grid.
    #[must_use]
    pub const fn height(&self) -> u16 {
        self.height
    }

    /// Sets the tile at the given coordinates on layer 0.
    ///
    /// # Panics
    ///
    /// Panics if the coordinates are out of bounds.
    pub fn put(&mut self, x: u16, y: u16, tile: Tile) {
        let pos = to_grixy_pos(Pos::new(x, y));
        let lb = self.layer0_mut();
        assert!(
            lb.buf.contains(pos),
            "coordinates out of bounds: ({x}, {y})"
        );
        lb.buf[pos] = tile;
    }

    /// Gets the tile at the given coordinates on layer 0.
    ///
    /// # Panics
    ///
    /// Panics if the coordinates are out of bounds.
    #[must_use]
    pub fn get(&self, x: u16, y: u16) -> &Tile {
        &self.layer0().buf[to_grixy_pos(Pos::new(x, y))]
    }

    /// Tries to set the tile at the given coordinates on layer 0.
    ///
    /// Returns `None` if the coordinates are out of bounds.
    pub fn checked_put(&mut self, x: u16, y: u16, tile: Tile) -> Option<()> {
        let pos = to_grixy_pos(Pos::new(x, y));
        let lb = self.layer0_mut();
        if lb.buf.contains(pos) {
            lb.buf[pos] = tile;
            Some(())
        } else {
            None
        }
    }

    /// Tries to get the tile at the given coordinates on layer 0.
    ///
    /// Returns `None` if the coordinates are out of bounds.
    #[must_use]
    pub fn checked_get(&self, x: u16, y: u16) -> Option<&Tile> {
        let pos = to_grixy_pos(Pos::new(x, y));
        self.layer0().buf.get(pos)
    }

    /// Tries to get a mutable reference to the tile at the given coordinates
    /// on layer 0.
    ///
    /// Returns `None` if the coordinates are out of bounds.
    pub fn checked_get_mut(&mut self, x: u16, y: u16) -> Option<&mut Tile> {
        let pos = to_grixy_pos(Pos::new(x, y));
        self.layer0_mut().buf.get_mut(pos)
    }

    /// Iterates all tiles on `layer` with their `(x, y)` coordinates.
    ///
    /// Returns `None` if the layer is unallocated.
    #[must_use]
    pub fn cells(&self, layer: u8) -> Option<Cells<'_>> {
        let lb = self.layer(layer)?;
        Some(Cells {
            iter: lb.buf.as_ref().iter().enumerate(),
            width: usize::from(self.width),
        })
    }

    /// Iterates all tiles on `layer` mutably with their `(x, y)` coordinates.
    ///
    /// If the layer has not been written to yet, it is allocated first.
    pub fn cells_mut(&mut self, layer: u8) -> CellsMut<'_> {
        let width = usize::from(self.width);
        let lb = self.layer_or_alloc(layer);
        CellsMut {
            iter: lb.buf.as_mut().iter_mut().enumerate(),
            width,
        }
    }

    /// Clears a specific layer, resetting all tiles to the default.
    ///
    /// Does nothing if the layer is unallocated.
    pub fn clear(&mut self, layer: u8) {
        if let Some(lb) = self.layers[usize::from(layer)].as_mut() {
            lb.buf.clear();
        }
    }

    /// Resize the grid to `width` × `height` tiles.
    ///
    /// Content within the overlapping region is preserved on all allocated
    /// layers. New cells are initialised to the default tile. Shrinking
    /// discards tiles outside the new bounds.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        for layer in self.layers.iter_mut().flatten() {
            layer.buf.resize(usize::from(width), usize::from(height));
        }
    }

    // ------------------------------------------------------------------
    // Write grapheme — layer 0 only
    // ------------------------------------------------------------------

    /// Write a grapheme cluster at `(x, y)` on layer 0, enforcing wide-
    /// character invariants.
    ///
    /// This is the canonical way to place content into the grid when the `egc`
    /// feature is enabled. It:
    ///
    /// - Clears any wide character whose primary or spacer cell would be
    ///   overwritten.
    /// - Sets [`TileFlags::WIDE_CHAR`] on the primary cell and places a
    ///   [`TileFlags::WIDE_CHAR_SPACER`] in the adjacent cell for 2-column
    ///   characters.
    /// - Stores multi-codepoint EGCs (combining marks, ZWJ sequences) in
    ///   `Tile::extra`, capped at 8 codepoints total.
    ///
    /// Does nothing if `(x, y)` is out of bounds, if the grapheme has zero
    /// display width, or if a 2-column wide character would overflow the grid
    /// (the last column needs both its own cell and a spacer).
    ///
    /// # Panics
    ///
    /// Panics if the grapheme's display width exceeds [`u16::MAX`]. In
    /// practice this cannot happen: the maximum Unicode grapheme width is 2.
    ///
    /// Only present when the `egc` feature is enabled.
    #[cfg(feature = "egc")]
    pub fn write_grapheme(&mut self, x: u16, y: u16, grapheme: &str, style: Style) {
        use unicode_width::UnicodeWidthStr;

        let width = u16::try_from(grapheme.width()).expect("grapheme width exceeds u16");
        if width == 0 {
            return;
        }

        // Capture dimensions as plain values to avoid borrow conflicts.
        let w = usize::from(self.width);
        let cap = w * usize::from(self.height);
        let idx = usize::from(y) * w + usize::from(x);
        if idx >= cap {
            return;
        }

        // A 2-column char needs a spacer at x+1. If that's out of bounds,
        // silently refuse rather than leaving an orphaned primary cell.
        if width == 2 && x.saturating_add(1) as usize >= w {
            return;
        }

        // Clear any wide-char cell that would be partially overwritten.
        // clear_overlap only needs dimensions as values (captured above).
        self.clear_overlap(x, y, width);

        // Capture width before borrowing self mutably.
        let grid_w = usize::from(self.width);
        let idx = usize::from(y) * grid_w + usize::from(x);

        let lb = self.layer0_mut();
        // Build cell content.
        let mut chars = grapheme.chars();
        let first = chars.next().unwrap_or(' ');
        let has_extra = chars.next().is_some();
        let extra = if has_extra {
            Some(alloc::sync::Arc::new(cap_grapheme(grapheme)))
        } else {
            None
        };
        let flags = if width == 2 {
            TileFlags::WIDE_CHAR
        } else {
            TileFlags::empty()
        };

        lb.buf.as_mut()[idx].glyph = first;
        lb.buf.as_mut()[idx].style = style;
        lb.buf.as_mut()[idx].extra = extra;
        lb.buf.as_mut()[idx].flags = flags;

        // Place spacer for wide characters.
        if width == 2 {
            let spacer_idx = usize::from(y) * grid_w + usize::from(x + 1);
            if spacer_idx < cap {
                let spacer = &mut lb.buf.as_mut()[spacer_idx];
                spacer.glyph = ' ';
                spacer.style = style;
                spacer.extra = None;
                spacer.flags = TileFlags::WIDE_CHAR_SPACER;
            }
        }
    }

    /// Clears wide-character cells that would be partially overwritten by a
    /// write starting at `(x, y)` spanning `width` columns.
    ///
    /// Operates on layer 0.
    #[cfg(feature = "egc")]
    fn clear_overlap(&mut self, x: u16, y: u16, width: u16) {
        let w = usize::from(self.width);
        let cap = w * usize::from(self.height);
        let lb = self.layer0_mut();
        for cx in x..x.saturating_add(width) {
            let idx = usize::from(y) * w + usize::from(cx);
            if idx >= cap {
                continue;
            }
            // flags is Copy, so reading through the shared ref is fine.
            let flags = lb.buf.as_ref()[idx].flags;

            if flags.contains(TileFlags::WIDE_CHAR_SPACER) && cx > 0 {
                let pidx = usize::from(y) * w + usize::from(cx - 1);
                if pidx < cap {
                    lb.buf.as_mut()[pidx].reset();
                }
            }

            if flags.contains(TileFlags::WIDE_CHAR) {
                let sidx = usize::from(y) * w + usize::from(cx + 1);
                if sidx < cap {
                    lb.buf.as_mut()[sidx].reset();
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Grid — multi-layer API
// ---------------------------------------------------------------------------

impl Grid {
    /// Write a tile to `layer` at `(x, y)`.
    ///
    /// Allocates the layer if it has not been written to yet. Returns `None`
    /// if `(x, y)` is out of bounds.
    ///
    /// To read back, use [`get_tile`](Self::get_tile).
    pub fn put_tile(&mut self, layer: u8, x: u16, y: u16, tile: Tile) -> Option<()> {
        let pos = to_grixy_pos(Pos::new(x, y));
        let lb = self.layer_or_alloc(layer);
        if !lb.buf.contains(pos) {
            return None;
        }
        lb.buf[pos] = tile;
        Some(())
    }

    /// Read a tile on `layer` at `(x, y)`, or `None` if the layer is
    /// unallocated or the coordinates are out of bounds.
    #[must_use]
    pub fn get_tile(&self, layer: u8, x: u16, y: u16) -> Option<&Tile> {
        let pos = to_grixy_pos(Pos::new(x, y));
        self.layer(layer)?.buf.get(pos)
    }

    /// Yield `(layer_id, Pos, &Tile)` for every allocated cell across
    /// all layers, in layer-major (0 → 255) then row-major order.
    ///
    /// Unallocated layers are skipped. This is used by backends that need
    /// the full frame on every draw (see [`Backend::needs_full_frame`]).
    pub fn layers(&self) -> impl Iterator<Item = (u8, Pos, &Tile)> + '_ {
        let mut results = Vec::new();
        for id in 0u8..=255 {
            if let Some(lb) = self.layer(id) {
                #[allow(clippy::cast_possible_truncation)]
                for (i, tile) in lb.buf.as_ref().iter().enumerate() {
                    let x = (i % usize::from(self.width)) as u16;
                    let y = (i / usize::from(self.width)) as u16;
                    results.push((id, Pos::new(x, y), tile));
                }
            }
        }
        results.into_iter()
    }

    /// Clear every allocated layer.
    pub fn clear_all(&mut self) {
        for layer in self.layers.iter_mut().flatten() {
            layer.buf.clear();
        }
    }

    /// Yield `(layer_id, Pos, &Tile)` for every changed position across all
    /// layers, in layer-major (0 → 255) then row-major order.
    ///
    /// Three cases per layer:
    /// - Layer absent in `self`: nothing yielded.
    /// - Layer in `self`, absent in `other` (newly allocated): all
    ///   `width × height` tiles yielded.
    /// - Layer in both: only positions where the `Tile` differs are yielded.
    pub fn diff<'a>(&'a self, other: &'a Self) -> impl Iterator<Item = (u8, Pos, &'a Tile)> + 'a {
        let mut results = Vec::new();
        for id in 0u8..=255 {
            match (self.layer(id), other.layer(id)) {
                (None, _) => {}
                (Some(cur), None) => {
                    // Newly allocated layer: all cells are "changed".
                    // TODO(M3): avoid Vec allocation, use Either-style iterator.
                    #[allow(clippy::cast_possible_truncation)]
                    for (i, tile) in cur.buf.as_ref().iter().enumerate() {
                        let x = (i % usize::from(self.width)) as u16;
                        let y = (i / usize::from(self.width)) as u16;
                        results.push((id, Pos::new(x, y), tile));
                    }
                }
                (Some(cur), Some(prev)) => {
                    for (pos, tile) in cur.buf.diff(&prev.buf) {
                        results.push((id, from_grixy_pos(pos), tile));
                    }
                }
            }
        }
        results.into_iter()
    }
}

// ---------------------------------------------------------------------------
// Index / IndexMut — layer 0
// ---------------------------------------------------------------------------

impl Index<Pos> for Grid {
    type Output = Tile;

    fn index(&self, pos: Pos) -> &Tile {
        &self.layer0().buf[to_grixy_pos(pos)]
    }
}

impl IndexMut<Pos> for Grid {
    fn index_mut(&mut self, pos: Pos) -> &mut Tile {
        let pos = to_grixy_pos(pos);
        &mut self.layer0_mut().buf[pos]
    }
}

// ---------------------------------------------------------------------------
// Display / Debug — layer 0
// ---------------------------------------------------------------------------

impl fmt::Display for Grid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for y in 0..self.height() {
            for x in 0..self.width() {
                let tile = self.get(x, y);
                #[cfg(feature = "egc")]
                let is_spacer = tile.flags.contains(TileFlags::WIDE_CHAR_SPACER);
                #[cfg(not(feature = "egc"))]
                let is_spacer = tile.glyph == '\0';
                let c = if is_spacer {
                    ' ' // right half of a wide char — don't print twice
                } else if tile.glyph == ' ' {
                    '·' // empty cell marker
                } else {
                    tile.glyph
                };
                write!(f, "{c}")?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

impl fmt::Debug for Grid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Grid")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Existing tests (must pass unchanged) ---

    #[test]
    fn test_grid_new() {
        let grid = Grid::new(80, 25);
        assert_eq!(grid.width(), 80);
        assert_eq!(grid.height(), 25);
    }

    #[test]
    fn test_grid_put_get() {
        let mut grid = Grid::new(10, 10);
        let tile = Tile::default().with_glyph('X');

        grid.put(5, 5, tile);
        assert_eq!(grid.get(5, 5).glyph(), 'X');
    }

    #[test]
    fn test_grid_checked_put_get() {
        let mut grid = Grid::new(10, 10);
        let tile = Tile::default().with_glyph('Y');

        assert!(grid.checked_put(5, 5, tile).is_some());
        assert_eq!(grid.checked_get(5, 5).unwrap().glyph(), 'Y');

        assert!(grid.checked_get(10, 0).is_none());
        assert!(grid.checked_put(0, 10, Tile::default()).is_none());
    }

    #[test]
    #[should_panic(expected = "coordinates out of bounds")]
    fn test_grid_panic_put() {
        let mut grid = Grid::new(10, 10);
        grid.put(10, 0, Tile::default());
    }

    #[test]
    fn test_grid_diff() {
        let mut g1 = Grid::new(2, 2);
        let g2 = Grid::new(2, 2);

        g1.put(0, 0, Tile::default().with_glyph('A'));

        let diffs: Vec<_> = g1.diff(&g2).collect();
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0], (0, Pos::new(0, 0), g1.get(0, 0)));
    }

    #[test]
    fn test_grid_resize_expand() {
        let mut grid = Grid::new(3, 3);
        grid.put(1, 1, Tile::default().with_glyph('X'));
        grid.resize(6, 6);
        assert_eq!(grid.width(), 6);
        assert_eq!(grid.height(), 6);
        assert_eq!(grid.get(1, 1).glyph(), 'X'); // preserved
        assert_eq!(grid.get(5, 5).glyph(), ' '); // new cells default
    }

    #[test]
    fn test_grid_resize_shrink() {
        let mut grid = Grid::new(10, 10);
        grid.put(1, 1, Tile::default().with_glyph('A'));
        grid.resize(5, 5);
        assert_eq!(grid.width(), 5);
        assert_eq!(grid.height(), 5);
        assert_eq!(grid.get(1, 1).glyph(), 'A'); // still in bounds, preserved
    }

    #[test]
    fn test_grid_resize_preserves_overlap() {
        let mut grid = Grid::new(4, 4);
        grid.put(0, 0, Tile::default().with_glyph('@'));
        grid.put(3, 3, Tile::default().with_glyph('X'));
        grid.resize(3, 3); // shrink: (3,3) falls outside
        assert_eq!(grid.get(0, 0).glyph(), '@');
        assert_eq!(grid.get(2, 2).glyph(), ' '); // was default, still default
    }

    #[test]
    fn test_grid_display() {
        let mut grid = Grid::new(3, 2);
        grid.put(0, 0, Tile::default().with_glyph('A'));

        let s = alloc::format!("{grid}");
        assert_eq!(s, "A··\n···\n");
    }

    #[test]
    fn test_grid_cells_count() {
        let grid = Grid::new(4, 3);
        assert_eq!(grid.cells(0).unwrap().count(), 12);
    }

    #[test]
    fn test_grid_cells_coordinates() {
        let grid = Grid::new(3, 2);
        let coords: Vec<(u16, u16)> = grid.cells(0).unwrap().map(|(x, y, _)| (x, y)).collect();
        assert_eq!(
            coords,
            vec![(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1),]
        );
    }

    #[test]
    fn test_grid_cells_mut() {
        use crate::style::Style;
        let mut grid = Grid::new(2, 2);
        for (x, y, tile) in grid.cells_mut(0) {
            #[allow(clippy::cast_possible_truncation)]
            let idx = (y * 2 + x) as u8;
            *tile = Tile::new(char::from(b'A' + idx), Style::default());
        }
        assert_eq!(grid.get(0, 0).glyph(), 'A');
        assert_eq!(grid.get(1, 0).glyph(), 'B');
        assert_eq!(grid.get(0, 1).glyph(), 'C');
        assert_eq!(grid.get(1, 1).glyph(), 'D');
    }

    #[test]
    fn test_rect_contains() {
        let r = Rect::new(2, 3, 4, 5);
        assert!(r.contains_pos(Pos::new(2, 3)));
        assert!(r.contains_pos(Pos::new(5, 7)));
        assert!(!r.contains_pos(Pos::new(6, 3))); // x == x+width, exclusive
        assert!(!r.contains_pos(Pos::new(2, 8))); // y == y+height, exclusive
        assert!(!r.contains_pos(Pos::new(1, 3)));
    }

    #[test]
    fn test_rect_area() {
        assert_eq!(Rect::new(0, 0, 5, 3).area(), 15);
        assert_eq!(Rect::default().area(), 0);
    }

    #[test]
    fn test_rect_top_left_bottom_right() {
        let r = Rect::new(1, 2, 3, 4);
        assert_eq!(r.top_left(), Pos::new(1, 2));
        assert_eq!(r.bottom_right(), Pos::new(4, 6));
    }

    #[test]
    fn test_rect_intersects() {
        let a = Rect::new(0, 0, 4, 4);
        let b = Rect::new(2, 2, 4, 4);
        let c = Rect::new(4, 0, 4, 4); // touches edge, no overlap
        assert!(!a.intersect(b).is_empty());
        assert!(a.intersect(c).is_empty());
    }

    #[test]
    fn test_rect_positions() {
        let r = Rect::new(1, 2, 2, 2);
        let pts: Vec<Pos> = r.pos_iter().collect();
        assert_eq!(
            pts,
            vec![
                Pos::new(1, 2),
                Pos::new(2, 2),
                Pos::new(1, 3),
                Pos::new(2, 3),
            ]
        );
    }

    #[test]
    fn test_index_position() {
        let mut grid = Grid::new(5, 5);
        let pos = Pos::new(2, 3);
        grid[pos] = Tile::default().with_glyph('Z');
        assert_eq!(grid[pos].glyph(), 'Z');
    }

    #[test]
    fn test_position_from_tuple() {
        let p: Pos = (3u16, 7u16).into();
        assert_eq!(p, Pos::new(3, 7));
        let t: (u16, u16) = p.into();
        assert_eq!(t, (3, 7));
    }

    #[test]
    fn test_size_from_tuple() {
        let s: Size = (80u16, 25u16).into();
        assert_eq!(
            s,
            Size {
                width: 80,
                height: 25
            }
        );
        let t: (u16, u16) = s.into();
        assert_eq!(t, (80, 25));
    }

    #[test]
    fn test_position_ord_row_major() {
        let mut positions = vec![Pos::new(5, 0), Pos::new(0, 1), Pos::new(3, 0)];
        positions.sort();
        assert_eq!(
            positions,
            vec![Pos::new(3, 0), Pos::new(5, 0), Pos::new(0, 1),]
        );
    }

    #[test]
    fn test_size_ord() {
        assert!(
            Size {
                width: 1,
                height: 2
            } < Size {
                width: 2,
                height: 1
            }
        );
    }

    // --- New tests for multi-layer API ---

    #[test]
    fn test_grid_layer_zero_always_allocated() {
        let g = Grid::new(5, 5);
        assert!(g.layer(0).is_some());
        for id in 1u8..=5 {
            assert!(g.layer(id).is_none(), "layer {id} should be None");
        }
    }

    #[test]
    fn test_grid_put_tile_allocates_layer() {
        let mut g = Grid::new(5, 5);
        g.put_tile(3, 0, 0, Tile::new('@', Style::default()));
        assert!(g.layer(3).is_some());
        assert!(g.layer(4).is_none());
    }

    #[test]
    fn test_grid_diff_empty_when_identical() {
        let g = Grid::new(5, 5);
        let prev = Grid::new(5, 5);
        assert_eq!(g.diff(&prev).count(), 0);
    }

    #[test]
    fn test_grid_diff_reports_changed_cell() {
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
    fn test_grid_diff_new_layer_yields_all_cells() {
        let mut cur = Grid::new(3, 4);
        let prev = Grid::new(3, 4);
        cur.put_tile(1, 0, 0, Tile::new('A', Style::default()));
        let diffs: Vec<_> = cur.diff(&prev).collect();
        // All 12 cells of the newly allocated layer 1 are yielded.
        assert_eq!(diffs.len(), 12);
        assert!(diffs.iter().all(|(l, _, _)| *l == 1));
    }

    #[test]
    fn test_grid_diff_layer_major_order() {
        let mut cur = Grid::new(3, 3);
        let prev = Grid::new(3, 3);
        cur.put_tile(2, 0, 0, Tile::new('B', Style::default()));
        cur.put_tile(0, 1, 0, Tile::new('A', Style::default()));
        let layers: Vec<u8> = cur.diff(&prev).map(|(l, _, _)| l).collect();
        // Layer 0's change appears first, then all of layer 2.
        assert_eq!(layers[0], 0);
        assert!(layers[1..].iter().all(|&l| l == 2));
    }

    #[test]
    fn test_grid_put_and_get_on_layer_2() {
        use crate::style::Style;
        let mut g = Grid::new(5, 5);
        g.put_tile(2, 1, 1, Tile::new('Z', Style::default()));
        assert_eq!(g.get_tile(2, 1, 1).unwrap().glyph, 'Z');
        // Layer 0 at same position should still be default.
        assert_eq!(g.get(1, 1).glyph, ' ');
        // Unallocated layer returns None.
        assert!(g.get_tile(3, 0, 0).is_none());
    }

    #[test]
    fn test_grid_clear_layer() {
        let mut g = Grid::new(5, 5);
        g.put_tile(1, 0, 0, Tile::new('Z', Style::default()));
        g.put_tile(0, 0, 0, Tile::new('A', Style::default()));
        g.clear(1);
        assert_eq!(g.get_tile(0, 0, 0).unwrap().glyph, 'A');
        assert!(g.get_tile(1, 0, 0).is_some());
        assert_eq!(g.get_tile(1, 0, 0).unwrap().glyph, ' '); // cleared
    }

    #[test]
    fn test_grid_clear_all() {
        let mut g = Grid::new(5, 5);
        g.put_tile(1, 0, 0, Tile::new('Z', Style::default()));
        g.put_tile(0, 0, 0, Tile::new('A', Style::default()));
        g.clear_all();
        // Both layers reset to default (space).
        assert_eq!(g.get(0, 0).glyph, ' ');
        assert_eq!(g.get_tile(1, 0, 0).unwrap().glyph, ' ');
    }
}
