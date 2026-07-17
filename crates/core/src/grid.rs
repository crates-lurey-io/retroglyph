//! The layered tile grid: [`Grid`], plus the [`Size`], [`Pos`], and [`Rect`]
//! coordinate types used throughout the crate.
//!
//! # Layers, draw order, and compositing
//!
//! A [`Grid`] holds up to 256 independent layers (`u8` ids `0..=255`), one
//! [`Tile`] per cell on each. Layer 0 is always allocated; layers 1-255 are
//! allocated lazily, on first write to that layer (see
//! [`put_tile`](Grid::put_tile), [`cells_mut`](Grid::cells_mut)). This is the
//! crate's most distinctive feature and the one most worth understanding
//! before reaching for a second layer.
//!
//! ## Draw order
//!
//! Layers composite bottom-to-top, in ascending id order: 0 first, then every
//! allocated layer up to [`max_layer`](Grid::max_layer), each painted over
//! whatever the layers below it produced. Layer id *is* z-order -- there is
//! no separate depth or z-index to set. A common convention is layer 0 for
//! terrain, 1 for items, 2 for actors, 3+ for UI/effects, but the crate
//! enforces nothing; any id can hold any content.
//!
//! Compositing itself happens in one of two places, chosen by the backend
//! (see [`crate::Backend::composites_layers`]):
//!
//! - **Cell backends** (`Headless`, `retroglyph-crossterm`) do not composite
//!   layers themselves. [`crate::Terminal::present`] calls
//!   `flatten_into` (crate-private) to collapse every allocated layer
//!   into a single-layer frame *before* handing it to the backend, so
//!   layers 1+ behave identically on every cell backend.
//! - **Pixel backends** (`retroglyph-software`) composite per pixel: they
//!   receive the raw layered stream from
//!   [`crate::Backend::draw_layers`] (layer-major, ascending id) and paint
//!   each layer's cells directly onto the pixel buffer in that order.
//!
//! ## The `EMPTY` flag: transparency vs. opaque occlusion
//!
//! Every [`Tile`] carries [`TileFlags::EMPTY`], set on [`Tile::default`] and
//! cleared by every write (`put_tile`, `write_grapheme`, indexing, ...).
//! Compositing treats it as the transparency bit:
//!
//! - An **untouched cell** (`EMPTY` set) is fully transparent:
//!   [`blit`](Grid::blit) skips it, and `flatten_into` (crate-private)
//!   leaves whatever the layers below already drew.
//! - An **explicit space** (`Tile::new(' ', style)`, `EMPTY` clear) is
//!   opaque: it overwrites the glyph and foreground below it, same as any
//!   other character. This is the one sharp edge in the model -- `' '`
//!   painted on a higher layer *erases* content underneath, it does not
//!   reveal it.
//!
//! Background color follows its own rule, independent of `EMPTY`: a tile's
//! background only overwrites the composited background when it is not
//! [`Color::Default`]. A non-empty tile with a `Default` background still
//! lets a lower layer's background show through even though its glyph is
//! opaque. See `flatten_into` (crate-private) for the exact rule.
//!
//! ## No short-circuiting: every allocated layer is visited, for every cell
//!
//! Compositing does not stop early when it hits an opaque tile on a high
//! layer. Both `flatten_into` (crate-private) and the software
//! backend's per-pixel compositor walk layers `0..=max_layer` in order for
//! *every* cell, unconditionally -- even if a fully opaque tile on layer 5
//! makes layers 6-50 invisible at that position. Cost is `O(max_layer)` per
//! cell, not `O(topmost opaque layer)`. Painting one fully opaque layer 250
//! over the whole grid still walks (and `EMPTY`-checks) layers 1-249 on
//! every present.
//!
//! ## Allocation cost: layer 1 vs. layer 200
//!
//! Writing to a layer for the first time allocates one `width x height`
//! buffer of [`Tile`]s -- the same cost regardless of the layer's id. Layer
//! 200 is no more expensive to *allocate* than layer 1: `Grid` stores a
//! 256-slot `Vec<Option<LayerBuf>>`, and only the slots that have been
//! written to hold `Some`; the rest are a cheap `None`.
//!
//! What the layer id *does* affect is steady-state iteration cost, via
//! [`max_layer`](Grid::max_layer): every present, diff, and full-grid
//! iteration walks `0..=max_layer`, skipping unallocated slots with an O(1)
//! `None` check. `max_layer` only grows -- clearing a layer
//! ([`clear`](Grid::clear)) does not deallocate it or lower `max_layer`. So
//! writing once to layer 200 and never touching layers 1-199 means every
//! future frame's compositing pass walks past 199 unallocated slots to reach
//! it. That walk is cheap (a pointer-sized `None` check per skipped layer)
//! but not free; prefer low, contiguous layer ids for frequently-updated
//! content and reserve high ids for rarely-touched overlays (e.g. a debug
//! HUD pinned to layer 255).

use crate::color::Color;
#[cfg(feature = "egc")]
use crate::style::Style;
use crate::tile::Tile;
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
// TODO: derive Clone once ixy's RowMajor (a ZST layout marker) implements Clone/Copy.
// Blocked on upstream: https://github.com/crates-lurey-io/ixy
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

/// A 2D buffer of [`Tile`]s, addressable across up to 256 stacked layers.
///
/// Layer 0 is always allocated; higher layers are allocated on first write.
/// Single-layer use pays no overhead: layers 1+ stay unallocated until used.
///
/// Requires an allocator (backed by `alloc::vec::Vec`), so it is unavailable
/// in strictly static, no-alloc environments.
pub struct Grid {
    width: u16,
    height: u16,
    /// Indexed by layer ID (0–255). Index 0 is always `Some`.
    /// Unwritten layers are `None` — no allocation until first write.
    layers: Vec<Option<LayerBuf>>,
    /// Highest layer ID that has been allocated. Always at least 0.
    max_layer: u8,
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
        if id > self.max_layer {
            self.max_layer = id;
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
            max_layer: 0,
        }
    }

    /// Build a grid from a rectangular character map, one [`Tile`] per cell.
    ///
    /// `map` is split on `\n`; the grid width is the longest line's character
    /// count and the height is the number of lines. Lines shorter than the
    /// widest are padded with the default tile. `f` maps each character to its
    /// tile, called once per character in reading order.
    ///
    /// Characters are counted as Unicode scalar values (one column each), which
    /// matches ASCII / CP437 maps and level/prefab strings. Wide characters are
    /// not width-adjusted.
    ///
    /// # Example
    ///
    /// ```
    /// use retroglyph_core::{Grid, Style, Tile};
    ///
    /// let grid = Grid::from_charmap("##\n#.", |c| match c {
    ///     '#' => Tile::new('#', Style::default()),
    ///     _ => Tile::default(),
    /// });
    /// assert_eq!((grid.width(), grid.height()), (2, 2));
    /// assert_eq!(grid.get(0, 0).glyph(), '#');
    /// assert_eq!(grid.get(1, 1).glyph(), ' ');
    /// ```
    #[must_use]
    pub fn from_charmap<F>(map: &str, mut f: F) -> Self
    where
        F: FnMut(char) -> Tile,
    {
        let mut width: u16 = 0;
        let mut height: u16 = 0;
        for line in map.lines() {
            let len = u16::try_from(line.chars().count()).unwrap_or(u16::MAX);
            width = width.max(len);
            height = height.saturating_add(1);
        }
        let mut grid = Self::new(width, height);
        for (y, line) in map.lines().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let y = y as u16;
            for (x, ch) in line.chars().enumerate() {
                #[allow(clippy::cast_possible_truncation)]
                let x = x as u16;
                grid.put_tile(0, x, y, f(ch));
            }
        }
        grid
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

    /// Returns the highest layer id that has ever been allocated.
    ///
    /// Always at least 0 (layer 0 is always allocated). This only grows:
    /// clearing a layer does not deallocate it, so the value does not shrink
    /// once a higher layer has been written.
    #[must_use]
    pub const fn max_layer(&self) -> u8 {
        self.max_layer
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
    pub fn write_grapheme(&mut self, layer: u8, x: u16, y: u16, grapheme: &str, style: Style) {
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
        self.clear_overlap(layer, x, y, width);

        // Capture width before borrowing self mutably.
        let grid_w = usize::from(self.width);
        let idx = usize::from(y) * grid_w + usize::from(x);

        let lb = self.layer_or_alloc(layer);
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
    #[cfg(feature = "egc")]
    fn clear_overlap(&mut self, layer: u8, x: u16, y: u16, width: u16) {
        let w = usize::from(self.width);
        let cap = w * usize::from(self.height);
        let lb = self.layer_or_alloc(layer);
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

    /// Copy tiles from `src` within `src_rect` to `self` at `(dst_x, dst_y)`
    /// on `layer`. Empty tiles (nothing written; see [`Tile::is_empty`]) are
    /// treated as transparent and skipped. An explicit space is copied and
    /// overwrites the destination.
    pub fn blit(&mut self, layer: u8, src: &Self, src_rect: Rect, dst_x: u16, dst_y: u16) {
        for sy in src_rect.top()..src_rect.bottom() {
            for sx in src_rect.left()..src_rect.right() {
                let Some(tile) = src.get_tile(layer, sx, sy) else {
                    continue;
                };
                if !tile.flags.contains(TileFlags::EMPTY) {
                    let dx = dst_x + sx.saturating_sub(src_rect.left());
                    let dy = dst_y + sy.saturating_sub(src_rect.top());
                    self.put_tile(layer, dx, dy, tile.clone());
                }
            }
        }
    }

    /// Same as [`blit`](Self::blit) but blends foreground and background
    /// colors with the given alpha factors. `fg_alpha` and `bg_alpha` are in
    /// 0.0-1.0 range where 0.0 = keep destination, 1.0 = replace with src.
    ///
    /// Blending operates on packed RGB values; [`Color::Default`] preserves
    /// the destination. Non-RGB color variants (Ansi/Indexed) are passed
    /// through unblended.
    ///
    /// Requires the `gem` feature (default on): the per-channel color lerp is
    /// delegated to [`gem::rgb::Lerp`].
    #[cfg(feature = "gem")]
    #[allow(clippy::too_many_arguments, clippy::float_cmp)]
    pub fn blit_alpha(
        &mut self,
        layer: u8,
        src: &Self,
        src_rect: Rect,
        dst_x: u16,
        dst_y: u16,
        fg_alpha: f32,
        bg_alpha: f32,
    ) {
        for sy in src_rect.top()..src_rect.bottom() {
            for sx in src_rect.left()..src_rect.right() {
                let Some(tile) = src.get_tile(layer, sx, sy) else {
                    continue;
                };
                if !tile.flags.contains(TileFlags::EMPTY) {
                    let dx = dst_x + sx.saturating_sub(src_rect.left());
                    let dy = dst_y + sy.saturating_sub(src_rect.top());
                    let mut blended = tile.clone();
                    if let Some(dst) = self.get_tile(layer, dx, dy) {
                        if fg_alpha != 1.0 {
                            blended.style.fg = blend_fg(tile.style.fg, dst.style.fg, fg_alpha);
                        }
                        if bg_alpha != 1.0 {
                            blended.style.bg = blend_bg(tile.style.bg, dst.style.bg, bg_alpha);
                        }
                    }
                    self.put_tile(layer, dx, dy, blended);
                }
            }
        }
    }

    /// Yield `(layer_id, Pos, &Tile)` for every allocated cell across
    /// all layers, in layer-major (0 → `max_layer`) then row-major order.
    ///
    /// Unallocated layers are skipped. This is used by backends that need
    /// the full frame on every draw (see [`crate::Backend::needs_full_frame`]).
    ///
    /// This iterator is zero-allocation: it walks the layer buffers inline.
    pub fn layers(&self) -> impl Iterator<Item = (u8, Pos, &Tile)> + '_ {
        let width = usize::from(self.width);
        (0..=self.max_layer)
            .filter_map(move |id| self.layer(id).map(|lb| (id, lb)))
            .flat_map(move |(id, lb)| {
                lb.buf.as_ref().iter().enumerate().map(move |(i, tile)| {
                    #[allow(clippy::cast_possible_truncation)]
                    let x = (i % width) as u16;
                    #[allow(clippy::cast_possible_truncation)]
                    let y = (i / width) as u16;
                    (id, Pos::new(x, y), tile)
                })
            })
    }

    /// Clear every allocated layer.
    pub fn clear_all(&mut self) {
        for layer in self.layers.iter_mut().flatten() {
            layer.buf.clear();
        }
    }

    /// Composite every allocated layer into `dst`'s layer 0, one tile per cell.
    ///
    /// Used by [`crate::Terminal::present`] for backends that do not composite
    /// layers themselves (see [`crate::Backend::composites_layers`]). The rule
    /// matches the software renderer's pixel semantics and the [`blit`](Self::blit)
    /// transparency convention:
    ///
    /// - Start from layer 0's tile (its `bg` fills the cell).
    /// - For each higher allocated layer, in ascending order: if the tile is
    ///   not empty (see [`Tile::is_empty`]) replace the glyph, foreground,
    ///   offsets, flags, and extra; if its background is not
    ///   [`Color::Default`], replace the background.
    ///
    /// Because an explicit space is not empty, drawing one on a higher layer
    /// overwrites (erases) the glyph beneath it.
    ///
    /// `dst` must have the same dimensions as `self`.
    pub(crate) fn flatten_into(&self, dst: &mut Self) {
        for y in 0..self.height {
            for x in 0..self.width {
                let mut out = self.get(x, y).clone();
                for id in 1..=self.max_layer {
                    let Some(tile) = self.get_tile(id, x, y) else {
                        continue;
                    };
                    let contributes_glyph = !tile.flags.contains(TileFlags::EMPTY);
                    if contributes_glyph {
                        out.glyph = tile.glyph;
                        out.style.fg = tile.style.fg;
                        out.dx = tile.dx;
                        out.dy = tile.dy;
                        out.flags = tile.flags;
                        out.extra.clone_from(&tile.extra);
                    }
                    if tile.style.bg != Color::Default {
                        out.style.bg = tile.style.bg;
                    }
                }
                dst.put(x, y, out);
            }
        }
    }

    /// Yield `(layer_id, Pos, &Tile)` for every changed position across all
    /// layers, in layer-major (0 → `max_layer`) then row-major order.
    ///
    /// Three cases per layer:
    /// - Layer absent in `self`: nothing yielded.
    /// - Layer in `self`, absent in `other` (newly allocated): all
    ///   `width × height` tiles yielded.
    /// - Layer in both: only positions where the `Tile` differs are yielded.
    ///
    /// This iterator is zero-allocation: it walks the layer buffers inline.
    pub fn diff<'a>(&'a self, other: &'a Self) -> impl Iterator<Item = (u8, Pos, &'a Tile)> + 'a {
        let width = usize::from(self.width);
        let max = self.max_layer;
        (0..=max).flat_map(move |id| {
            match (self.layer(id), other.layer(id)) {
                // Layer absent in `self`: nothing changed.
                (None, _) => LayerDiff::Empty,
                // Newly allocated layer: all cells are "changed".
                (Some(cur_lb), None) => LayerDiff::Full(
                    cur_lb
                        .buf
                        .as_ref()
                        .iter()
                        .enumerate()
                        .map(move |(i, tile)| {
                            #[allow(clippy::cast_possible_truncation)]
                            let x = (i % width) as u16;
                            #[allow(clippy::cast_possible_truncation)]
                            let y = (i / width) as u16;
                            (id, Pos::new(x, y), tile)
                        }),
                ),
                // Layer in both: only the differing cells.
                (Some(cur_lb), Some(prev_lb)) => LayerDiff::Diff(
                    cur_lb
                        .buf
                        .diff(&prev_lb.buf)
                        .map(move |(pos, tile)| (id, from_grixy_pos(pos), tile)),
                ),
            }
        })
    }
}

/// Per-layer diff iterator, replacing a boxed trait object so `diff` performs
/// no per-layer heap allocation.
enum LayerDiff<F, D> {
    Empty,
    Full(F),
    Diff(D),
}

impl<'a, F, D> Iterator for LayerDiff<F, D>
where
    F: Iterator<Item = (u8, Pos, &'a Tile)>,
    D: Iterator<Item = (u8, Pos, &'a Tile)>,
{
    type Item = (u8, Pos, &'a Tile);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Empty => None,
            Self::Full(iter) => iter.next(),
            Self::Diff(iter) => iter.next(),
        }
    }
}

/// Blend two [`Color`] values by interpolating RGB components.
/// [`Color::Default`] preserves the destination. Non-RGB source colors are
/// returned as-is (no resolution).
///
/// Per-channel sRGB-domain lerp (src -> dst by `t`) delegated to
/// [`gem::rgb::Lerp`], which is `no_std`-safe (round-half-away via
/// `floor(x + 0.5)`, no `std`/`libm` float intrinsics).
#[cfg(feature = "gem")]
#[allow(clippy::float_cmp)]
fn blend_color(src: Color, dst: Color, t: f32) -> Color {
    use gem::rgb::{HasBlue as _, HasGreen as _, HasRed as _, Lerp as _, Rgb888};
    match (src, dst) {
        (Color::Default, _) => Color::Default,
        (
            Color::Rgb {
                r: sr,
                g: sg,
                b: sb,
            },
            Color::Rgb { r, g, b },
        ) if t != 1.0 => {
            let out = Rgb888::from_rgb(sr, sg, sb).lerp(Rgb888::from_rgb(r, g, b), t);
            Color::Rgb {
                r: out.red(),
                g: out.green(),
                b: out.blue(),
            }
        }
        (src, _) => src,
    }
}

#[cfg(feature = "gem")]
fn blend_fg(src: Color, dst: Color, t: f32) -> Color {
    blend_color(src, dst, t)
}

#[cfg(feature = "gem")]
fn blend_bg(src: Color, dst: Color, t: f32) -> Color {
    blend_color(src, dst, t)
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

/// Property tests for the wide-character (EGC) grid invariants.
///
/// These exercise the trickiest code in the crate — `write_grapheme` and its
/// `clear_overlap` helper — by hammering a small grid with random sequences of
/// narrow, wide, combining, and emoji graphemes and checking that the
/// wide-character bookkeeping never desyncs.
#[cfg(all(test, feature = "egc"))]
mod egc_proptests {
    use super::*;
    use crate::style::Style;
    use proptest::prelude::*;

    const W: u16 = 8;
    const H: u16 = 4;

    /// Narrow, wide (CJK), combining-mark, and wide-emoji graphemes.
    const GRAPHEMES: &[&str] = &["a", "\u{4e2d}", "e\u{0301}", "\u{1f600}"];

    /// Every `WIDE_CHAR` has its spacer to the right, every `WIDE_CHAR_SPACER`
    /// has its lead to the left, and no cell is both.
    fn assert_wide_invariants(grid: &Grid) {
        for y in 0..grid.height() {
            for x in 0..grid.width() {
                let flags = grid.get(x, y).flags();
                let lead = flags.contains(TileFlags::WIDE_CHAR);
                let spacer = flags.contains(TileFlags::WIDE_CHAR_SPACER);

                assert!(
                    !(lead && spacer),
                    "cell ({x}, {y}) is both wide lead and spacer"
                );

                if lead {
                    assert!(x + 1 < grid.width(), "wide lead at ({x}, {y}) has no room");
                    assert!(
                        grid.get(x + 1, y)
                            .flags()
                            .contains(TileFlags::WIDE_CHAR_SPACER),
                        "wide lead at ({x}, {y}) is missing its spacer"
                    );
                }

                if spacer {
                    assert!(x > 0, "orphan spacer at ({x}, {y}) (no cell to the left)");
                    assert!(
                        grid.get(x - 1, y).flags().contains(TileFlags::WIDE_CHAR),
                        "orphan spacer at ({x}, {y}) (left cell is not a wide lead)"
                    );
                }
            }
        }
    }

    proptest! {
        #[test]
        fn wide_char_bookkeeping_never_desyncs(
            ops in prop::collection::vec(
                (0u16..W, 0u16..H, 0usize..GRAPHEMES.len()),
                0..64,
            ),
        ) {
            let mut grid = Grid::new(W, H);
            for (x, y, gi) in ops {
                grid.write_grapheme(0, x, y, GRAPHEMES[gi], Style::default());
                // The invariant must hold after every single write, not just
                // at the end — an intermediate orphan would be a real bug.
                assert_wide_invariants(&grid);
            }
        }
    }
}
