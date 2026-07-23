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
//! (see [`crate::Output::composites_layers`]):
//!
//! - **Cell backends** (`Headless`, `retroglyph-crossterm`) do not composite
//!   layers themselves. [`crate::Terminal::present`] calls
//!   `flatten_into` (crate-private) to collapse every allocated layer
//!   into a single-layer frame *before* handing it to the backend, so
//!   layers 1+ behave identically on every cell backend.
//! - **Pixel backends** (`retroglyph-software`) composite per pixel: they
//!   receive the raw layered stream from
//!   [`crate::Output::draw_layers`] (layer-major, ascending id) and paint
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
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
#[cfg(feature = "gem")]
use alpha_blend::blend_modes::SeparableBlendMode;
use core::fmt;
use core::ops::{Index, IndexMut};
use grixy::buf::GridBuf;
use grixy::ops::layout::RowMajor;
use grixy::ops::{ExactSizeGrid, GridRead, GridWrite};

/// Blend mode for [`Grid::blit_alpha`], selecting how source and destination colors combine
/// before the `fg_alpha`/`bg_alpha` factor is applied.
///
/// [`Linear`](Self::Linear) is a straight per-channel color lerp -- `blit_alpha`'s original
/// behavior. The remaining variants are the [W3C separable blend modes] libtcod also offers:
/// each computes a fully blended color per channel via
/// [`alpha_blend::blend_modes::SeparableBlendMode`], and *that* result is what gets lerped
/// against the destination by the alpha factor, in place of the source color `Linear` would use.
///
/// Requires the `gem` feature (default on): see [`Grid::blit_alpha`]'s doc comment for which
/// crate backs each mode.
///
/// [W3C separable blend modes]: https://www.w3.org/TR/compositing-1/#blending
#[cfg(feature = "gem")]
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BlendMode {
    /// Straight per-channel RGB lerp between destination and source.
    #[default]
    Linear,
    /// Lightens: `dst + src - dst * src`. Always at least as light as either input.
    Screen,
    /// Brightens the destination to reflect the source (aka "color dodge").
    Dodge,
    /// Darkens the destination to reflect the source (aka "color burn").
    Burn,
    /// Multiplies or screens the colors, depending on the destination.
    Overlay,
}

#[cfg(feature = "gem")]
impl BlendMode {
    /// The equivalent [`SeparableBlendMode`], or `None` for [`Linear`](Self::Linear) (which uses
    /// [`gem::rgb::Lerp`] instead -- see [`blend_color`]).
    const fn separable(self) -> Option<SeparableBlendMode> {
        match self {
            Self::Linear => None,
            Self::Screen => Some(SeparableBlendMode::Screen),
            Self::Dodge => Some(SeparableBlendMode::ColorDodge),
            Self::Burn => Some(SeparableBlendMode::ColorBurn),
            Self::Overlay => Some(SeparableBlendMode::Overlay),
        }
    }
}

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
#[derive(Clone)]
pub(crate) struct LayerBuf {
    pub(crate) buf: GridBuf<Tile, Vec<Tile>, RowMajor>,
    /// Sparse EGC side-table: flat row-major index -> full grapheme text, for
    /// tiles with [`TileFlags::HAS_EXTRA`] set. Empty unless the `egc`
    /// feature is used to write a multi-codepoint grapheme, which is what
    /// keeps [`Tile`] itself small (see [`Grid::grapheme`]).
    ///
    /// The `HAS_EXTRA` flag is authoritative: readers must check it before
    /// consulting this map, since some write paths (`put`, `put_tile`,
    /// `IndexMut`, `cells_mut`) can leave a stale entry behind when they
    /// overwrite a tile that used to carry extra text without an explicit
    /// cleanup call. Since those paths only ever hand out or store tiles
    /// with `HAS_EXTRA` clear, a stale entry is harmless: it is simply
    /// never looked up until the slot is reused by `write_grapheme`, which
    /// always overwrites it.
    extras: BTreeMap<usize, Arc<str>>,
}

impl LayerBuf {
    fn new(width: u16, height: u16) -> Self {
        let n = usize::from(width) * usize::from(height);
        Self {
            buf: GridBuf::from_buffer(alloc::vec![Tile::default(); n], usize::from(width)),
            extras: BTreeMap::new(),
        }
    }

    /// Returns the grapheme text for the tile at flat index `idx`, or `None`
    /// if `tile` doesn't have [`TileFlags::HAS_EXTRA`] set.
    fn extra_for(&self, idx: usize, tile: &Tile) -> Option<&str> {
        if tile.flags.contains(TileFlags::HAS_EXTRA) {
            self.extras.get(&idx).map(|s| &**s)
        } else {
            None
        }
    }

    /// Returns a cloned `Arc` handle to the grapheme text at flat index
    /// `idx`, or `None` if `tile` doesn't have [`TileFlags::HAS_EXTRA`] set.
    /// Used to copy extras between grids (e.g. [`Grid::blit`]) without
    /// re-allocating the string.
    fn extra_arc_for(&self, idx: usize, tile: &Tile) -> Option<Arc<str>> {
        if tile.flags.contains(TileFlags::HAS_EXTRA) {
            self.extras.get(&idx).cloned()
        } else {
            None
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
#[derive(Clone)]
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
    /// Since [`Tile`] cannot carry a multi-codepoint grapheme itself (see
    /// [`grapheme`](Self::grapheme)), overwriting a cell this way always
    /// clears any extra text previously stored for it -- use
    /// [`write_grapheme`](Self::write_grapheme) to write EGCs.
    ///
    /// # Panics
    ///
    /// Panics if the coordinates are out of bounds.
    pub fn put(&mut self, x: u16, y: u16, tile: Tile) {
        let pos = to_grixy_pos(Pos::new(x, y));
        let idx = usize::from(y) * usize::from(self.width) + usize::from(x);
        let lb = self.layer0_mut();
        assert!(
            lb.buf.contains(pos),
            "coordinates out of bounds: ({x}, {y})"
        );
        lb.extras.remove(&idx);
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

    /// Returns the full grapheme cluster stored for the tile at `(x, y)` on
    /// `layer`, if any.
    ///
    /// `Some` only when the tile has [`TileFlags::HAS_EXTRA`] set, i.e. it
    /// was written via [`write_grapheme`](Self::write_grapheme) with a
    /// multi-codepoint EGC (combining marks, ZWJ sequences, etc.). For the
    /// common single-codepoint case, or without the `egc` feature, this is
    /// always `None`; use [`get_tile`](Self::get_tile)'s
    /// [`Tile::glyph`](crate::tile::Tile::glyph) and
    /// [`encode_utf8`](char::encode_utf8) to reconstruct the string instead.
    ///
    /// Returns `None` if the layer is unallocated or the coordinates are out
    /// of bounds.
    #[must_use]
    pub fn grapheme(&self, layer: u8, x: u16, y: u16) -> Option<&str> {
        let lb = self.layer(layer)?;
        let pos = to_grixy_pos(Pos::new(x, y));
        let tile = lb.buf.get(pos)?;
        let idx = usize::from(y) * usize::from(self.width) + usize::from(x);
        lb.extra_for(idx, tile)
    }

    /// Tries to set the tile at the given coordinates on layer 0.
    ///
    /// Returns `None` if the coordinates are out of bounds. See
    /// [`put`](Self::put) for the EGC-clearing caveat.
    pub fn checked_put(&mut self, x: u16, y: u16, tile: Tile) -> Option<()> {
        let pos = to_grixy_pos(Pos::new(x, y));
        let idx = usize::from(y) * usize::from(self.width) + usize::from(x);
        let lb = self.layer0_mut();
        if lb.buf.contains(pos) {
            lb.extras.remove(&idx);
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
            lb.extras.clear();
        }
    }

    /// Resize the grid to `width` × `height` tiles.
    ///
    /// Content within the overlapping region is preserved on all allocated
    /// layers. New cells are initialised to the default tile. Shrinking
    /// discards tiles outside the new bounds.
    pub fn resize(&mut self, width: u16, height: u16) {
        let old_width = usize::from(self.width);
        let new_width = usize::from(width);
        let new_height = usize::from(height);
        self.width = width;
        self.height = height;
        for layer in self.layers.iter_mut().flatten() {
            // The extras side-table is keyed by flat row-major index, which
            // shifts whenever the width changes -- remap it in lockstep with
            // `buf.resize` (below) rather than leaving it pointing at stale
            // (or now out-of-bounds) cells.
            if !layer.extras.is_empty() {
                layer.extras = layer
                    .extras
                    .iter()
                    .filter_map(|(&old_idx, s)| {
                        let x = old_idx % old_width;
                        let y = old_idx / old_width;
                        (x < new_width && y < new_height).then(|| (y * new_width + x, s.clone()))
                    })
                    .collect();
            }
            layer.buf.resize(new_width, new_height);
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
    /// - Stores multi-codepoint EGCs (combining marks, ZWJ sequences) in the
    ///   layer's EGC side-table (see [`grapheme`](Self::grapheme)), capped at
    ///   8 codepoints total.
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
        let flags = if width == 2 {
            TileFlags::WIDE_CHAR
        } else {
            TileFlags::empty()
        };
        let flags = if has_extra {
            flags | TileFlags::HAS_EXTRA
        } else {
            flags
        };

        lb.buf.as_mut()[idx].glyph = first;
        lb.buf.as_mut()[idx].style = style;
        lb.buf.as_mut()[idx].flags = flags;
        // `width` here is the full grapheme's display width (1 or 2), not just `first`'s -- more
        // accurate than recomputing from the primary codepoint alone, and exactly what the
        // terminal renderer needs to advance the cursor after printing this cell.
        #[allow(clippy::cast_possible_truncation)]
        {
            lb.buf.as_mut()[idx].width = width as u8;
        }
        if has_extra {
            lb.extras.insert(idx, Arc::from(cap_grapheme(grapheme)));
        } else {
            lb.extras.remove(&idx);
        }

        // Place spacer for wide characters.
        if width == 2 {
            let spacer_idx = usize::from(y) * grid_w + usize::from(x + 1);
            if spacer_idx < cap {
                let spacer = &mut lb.buf.as_mut()[spacer_idx];
                spacer.glyph = ' ';
                spacer.style = style;
                spacer.width = 0;
                spacer.flags = TileFlags::WIDE_CHAR_SPACER;
                lb.extras.remove(&spacer_idx);
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
                    lb.extras.remove(&pidx);
                }
            }

            if flags.contains(TileFlags::WIDE_CHAR) {
                let sidx = usize::from(y) * w + usize::from(cx + 1);
                if sidx < cap {
                    lb.buf.as_mut()[sidx].reset();
                    lb.extras.remove(&sidx);
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
    ///
    /// Like [`put`](Self::put), any tile written this way has its extra
    /// grapheme text cleared, since a caller-constructed [`Tile`] can never
    /// legitimately carry [`TileFlags::HAS_EXTRA`] (the flag is
    /// crate-private). Internal callers that need to preserve EGC text
    /// across a copy (e.g. [`blit`](Self::blit)) follow up with a direct
    /// extras-table write.
    pub fn put_tile(&mut self, layer: u8, x: u16, y: u16, mut tile: Tile) -> Option<()> {
        let pos = to_grixy_pos(Pos::new(x, y));
        let idx = usize::from(y) * usize::from(self.width) + usize::from(x);
        let lb = self.layer_or_alloc(layer);
        if !lb.buf.contains(pos) {
            return None;
        }
        lb.extras.remove(&idx);
        tile.flags.remove(TileFlags::HAS_EXTRA);
        lb.buf[pos] = tile;
        Some(())
    }

    /// Sets the extra grapheme text for an already-written tile at `(x, y)`
    /// on `layer`, setting [`TileFlags::HAS_EXTRA`] to match. Does nothing if
    /// out of bounds. Crate-private: the only external way to write EGC text
    /// is [`write_grapheme`](Self::write_grapheme).
    pub(crate) fn set_extra(&mut self, layer: u8, x: u16, y: u16, extra: Arc<str>) {
        let pos = to_grixy_pos(Pos::new(x, y));
        let idx = usize::from(y) * usize::from(self.width) + usize::from(x);
        let lb = self.layer_or_alloc(layer);
        if lb.buf.contains(pos) {
            lb.buf[pos].flags.insert(TileFlags::HAS_EXTRA);
            lb.extras.insert(idx, extra);
        }
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
                    let src_idx = usize::from(sy) * usize::from(src.width) + usize::from(sx);
                    let extra = src
                        .layer(layer)
                        .and_then(|lb| lb.extra_arc_for(src_idx, tile));
                    self.put_tile(layer, dx, dy, *tile);
                    if let Some(extra) = extra {
                        self.set_extra(layer, dx, dy, extra);
                    }
                }
            }
        }
    }

    /// Same as [`blit`](Self::blit) but blends foreground and background
    /// colors with the given alpha factors, using `mode` to compute the
    /// blended color. `fg_alpha` and `bg_alpha` are in 0.0-1.0 range where
    /// 0.0 = keep destination, 1.0 = replace with src; for a non-
    /// [`Linear`](BlendMode::Linear) `mode`, "replace with src" instead means
    /// "replace with `mode`'s fully blended color" (see [`BlendMode`]).
    ///
    /// Blending operates on packed RGB values; [`Color::Default`] preserves
    /// the destination. Non-RGB color variants (Ansi/Indexed) are passed
    /// through unblended, regardless of `mode`.
    ///
    /// Requires the `gem` feature (default on): [`BlendMode::Linear`]'s
    /// per-channel color lerp is delegated to [`gem::rgb::Lerp`]; the other
    /// modes delegate to [`alpha_blend::blend_modes::SeparableBlendMode`].
    #[cfg(feature = "gem")]
    #[allow(clippy::too_many_arguments, clippy::float_cmp)]
    pub fn blit_alpha(
        &mut self,
        layer: u8,
        src: &Self,
        src_rect: Rect,
        dst_x: u16,
        dst_y: u16,
        mode: BlendMode,
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
                    let mut blended = *tile;
                    if let Some(dst) = self.get_tile(layer, dx, dy) {
                        // `fg_alpha == 1.0` only lets `Linear` skip the call: `Linear` at `t ==
                        // 1.0` is `src` by definition, but a `Screen`/`Dodge`/`Burn`/`Overlay`
                        // mix at full alpha still needs to run the mode's formula -- it isn't
                        // equivalent to the raw source color (see `blend_color`'s matching guard).
                        if mode != BlendMode::Linear || fg_alpha != 1.0 {
                            blended.style.fg =
                                blend_fg(mode, tile.style.fg, dst.style.fg, fg_alpha);
                        }
                        if mode != BlendMode::Linear || bg_alpha != 1.0 {
                            blended.style.bg =
                                blend_bg(mode, tile.style.bg, dst.style.bg, bg_alpha);
                        }
                    }
                    let src_idx = usize::from(sy) * usize::from(src.width) + usize::from(sx);
                    let extra = src
                        .layer(layer)
                        .and_then(|lb| lb.extra_arc_for(src_idx, tile));
                    self.put_tile(layer, dx, dy, blended);
                    if let Some(extra) = extra {
                        self.set_extra(layer, dx, dy, extra);
                    }
                }
            }
        }
    }

    /// Yield `(layer_id, Pos, &Tile, Option<&str>)` for every allocated cell
    /// across all layers, in layer-major (0 → `max_layer`) then row-major
    /// order. The last element is the tile's grapheme text (see
    /// [`grapheme`](Self::grapheme)), `Some` only when
    /// [`TileFlags::HAS_EXTRA`] is set.
    ///
    /// Unallocated layers are skipped. This is used by backends that need
    /// the full frame on every draw (see [`crate::Output::needs_full_frame`]).
    ///
    /// This iterator is zero-allocation: it walks the layer buffers inline.
    pub fn layers(&self) -> impl Iterator<Item = (u8, Pos, &Tile, Option<&str>)> + '_ {
        let width = usize::from(self.width);
        (0..=self.max_layer)
            .filter_map(move |id| self.layer(id).map(|lb| (id, lb)))
            .flat_map(move |(id, lb)| {
                lb.buf.as_ref().iter().enumerate().map(move |(i, tile)| {
                    #[allow(clippy::cast_possible_truncation)]
                    let x = (i % width) as u16;
                    #[allow(clippy::cast_possible_truncation)]
                    let y = (i / width) as u16;
                    (id, Pos::new(x, y), tile, lb.extra_for(i, tile))
                })
            })
    }

    /// Clear every allocated layer.
    pub fn clear_all(&mut self) {
        for layer in self.layers.iter_mut().flatten() {
            layer.buf.clear();
            layer.extras.clear();
        }
    }

    /// Composite every allocated layer into `dst`'s layer 0, one tile per cell.
    ///
    /// Used by [`crate::Terminal::present`] for backends that do not composite
    /// layers themselves (see [`crate::Output::composites_layers`]). The rule
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
        let width = usize::from(self.width);
        for y in 0..self.height {
            for x in 0..self.width {
                let mut out = *self.get(x, y);
                let idx = usize::from(y) * width + usize::from(x);
                let mut out_extra = self.layer0().extra_arc_for(idx, &out);
                for id in 1..=self.max_layer {
                    let Some(tile) = self.get_tile(id, x, y) else {
                        continue;
                    };
                    let contributes_glyph = !tile.flags.contains(TileFlags::EMPTY);
                    if contributes_glyph {
                        out.glyph = tile.glyph;
                        out.width = tile.width;
                        out.style.fg = tile.style.fg;
                        out.dx = tile.dx;
                        out.dy = tile.dy;
                        out.flags = tile.flags;
                        out_extra = self.layer(id).and_then(|lb| lb.extra_arc_for(idx, tile));
                    }
                    if tile.style.bg != Color::Default {
                        out.style.bg = tile.style.bg;
                    }
                }
                dst.put(x, y, out);
                if let Some(extra) = out_extra {
                    dst.set_extra(0, x, y, extra);
                }
            }
        }
    }

    /// Yield `(layer_id, Pos, &Tile, Option<&str>)` for every changed
    /// position across all layers, in layer-major (0 → `max_layer`) then
    /// row-major order. The last element is the changed tile's grapheme text
    /// (see [`grapheme`](Self::grapheme)).
    ///
    /// Three cases per layer:
    /// - Layer absent in `self`: nothing yielded.
    /// - Layer in `self`, absent in `other` (newly allocated): all
    ///   `width × height` tiles yielded.
    /// - Layer in both: only positions where the `Tile` or its grapheme text
    ///   differs are yielded. `self` and `other` must have matching
    ///   dimensions for this case; the crate never calls `diff` otherwise.
    ///
    /// This iterator is zero-allocation: it walks the layer buffers inline.
    pub fn diff<'a>(
        &'a self,
        other: &'a Self,
    ) -> impl Iterator<Item = (u8, Pos, &'a Tile, Option<&'a str>)> + 'a {
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
                            (id, Pos::new(x, y), tile, cur_lb.extra_for(i, tile))
                        }),
                ),
                // Layer in both: only the differing cells. Compared by hand
                // (rather than delegating to grixy's `GridDiff`) because a
                // `Tile`-only comparison can't see grapheme-text changes: two
                // multi-codepoint EGCs sharing a primary codepoint but
                // different combining marks (e.g. `e\u{0301}` vs `e\u{0300}`)
                // compare equal on every `Tile` field.
                (Some(cur_lb), Some(prev_lb)) => {
                    LayerDiff::Diff(cur_lb.buf.as_ref().iter().enumerate().filter_map(
                        move |(i, tile)| {
                            let prev_tile = &prev_lb.buf.as_ref()[i];
                            let cur_extra = cur_lb.extra_for(i, tile);
                            let prev_extra = prev_lb.extra_for(i, prev_tile);
                            if tile == prev_tile && cur_extra == prev_extra {
                                return None;
                            }
                            #[allow(clippy::cast_possible_truncation)]
                            let x = (i % width) as u16;
                            #[allow(clippy::cast_possible_truncation)]
                            let y = (i / width) as u16;
                            Some((id, Pos::new(x, y), tile, cur_extra))
                        },
                    ))
                }
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
    F: Iterator<Item = (u8, Pos, &'a Tile, Option<&'a str>)>,
    D: Iterator<Item = (u8, Pos, &'a Tile, Option<&'a str>)>,
{
    type Item = (u8, Pos, &'a Tile, Option<&'a str>);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Empty => None,
            Self::Full(iter) => iter.next(),
            Self::Diff(iter) => iter.next(),
        }
    }
}

/// Blend two [`Color`] values using `mode`. [`Color::Default`] preserves the
/// destination. Non-RGB source colors are returned as-is (no resolution).
///
/// [`BlendMode::Linear`] is a per-channel sRGB-domain lerp (dst -> src by
/// `t`) delegated to [`gem::rgb::Lerp`], which is `no_std`-safe (round-half-
/// away via `floor(x + 0.5)`, no `std`/`libm` float intrinsics). The other
/// modes evaluate [`SeparableBlendMode::mix`] per channel in `0.0..=1.0`
/// (converting u8 <-> f32 at the boundary; see [`blend_separable_channel`]),
/// then lerp that fully mixed color against the destination by `t`, same as
/// `Linear`.
#[cfg(feature = "gem")]
#[allow(clippy::float_cmp)]
fn blend_color(mode: BlendMode, src: Color, dst: Color, t: f32) -> Color {
    use gem::rgb::{HasBlue as _, HasGreen as _, HasRed as _, Lerp as _, Rgb888};
    match (src, dst) {
        (Color::Default, _) => Color::Default,
        (
            Color::Rgb {
                r: sr,
                g: sg,
                b: sb,
            },
            Color::Rgb {
                r: dr,
                g: dg,
                b: db,
            },
        ) if mode != BlendMode::Linear || t != 1.0 => {
            // `Linear` at `t == 1.0` is `src` by definition (skip to the catch-all arm below);
            // the other modes must still run their mix formula at `t == 1.0` -- see `blit_alpha`.
            let (r, g, b) = mode.separable().map_or_else(
                || {
                    // `dst.lerp(src, t)`, not `src.lerp(dst, t)`: at `t == 0.0` this must return
                    // `dst` ("keep destination", per `blit_alpha`'s doc comment) and only reach
                    // `src` at `t == 1.0` -- the same `0.0 == dst, 1.0 == fully blended` contract
                    // every other `BlendMode` follows (see `blend_separable_channel`).
                    let out = Rgb888::from_rgb(dr, dg, db).lerp(Rgb888::from_rgb(sr, sg, sb), t);
                    (out.red(), out.green(), out.blue())
                },
                |sep| {
                    (
                        blend_separable_channel(sep, sr, dr, t),
                        blend_separable_channel(sep, sg, dg, t),
                        blend_separable_channel(sep, sb, db, t),
                    )
                },
            );
            Color::Rgb { r, g, b }
        }
        (src, _) => src,
    }
}

/// Evaluates `sep`'s per-channel mixing function for one RGB channel (`src`/`dst` are u8, `sep`
/// operates in `0.0..=1.0` f32), then lerps that mixed value against `dst` by `t` -- `0.0` keeps
/// `dst`, `1.0` uses the fully mixed color. Rounds with `libm::roundf` rather than `f32::round`
/// (a `std`-only method not available in `core`, same reasoning as `libm::fmaf` in
/// `animate::easing`) and clamps before converting back to u8, since `ColorDodge`/`ColorBurn`'s
/// `min(1.0, ...)` branches can round a hair outside `0.0..=1.0` at the float boundary.
#[cfg(feature = "gem")]
fn blend_separable_channel(sep: SeparableBlendMode, src: u8, dst: u8, t: f32) -> u8 {
    let cs = f32::from(src) / 255.0;
    let cb = f32::from(dst) / 255.0;
    let mixed = sep.mix(cb, cs);
    // Not `f32::mul_add`: it's a std-only inherent method, not in `core`. `libm::fmaf` is the
    // no_std-safe equivalent (see `animate::easing` for the same reasoning).
    let blended = libm::fmaf(mixed - cb, t, cb);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let out = libm::roundf(blended.clamp(0.0, 1.0) * 255.0) as u8;
    out
}

#[cfg(feature = "gem")]
fn blend_fg(mode: BlendMode, src: Color, dst: Color, t: f32) -> Color {
    blend_color(mode, src, dst, t)
}

#[cfg(feature = "gem")]
fn blend_bg(mode: BlendMode, src: Color, dst: Color, t: f32) -> Color {
    blend_color(mode, src, dst, t)
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
        assert_eq!(diffs[0], (0, Pos::new(0, 0), g1.get(0, 0), None));
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
        assert!(diffs.iter().all(|(l, _, _, _)| *l == 1));
    }

    #[test]
    fn test_grid_diff_layer_major_order() {
        let mut cur = Grid::new(3, 3);
        let prev = Grid::new(3, 3);
        cur.put_tile(2, 0, 0, Tile::new('B', Style::default()));
        cur.put_tile(0, 1, 0, Tile::new('A', Style::default()));
        let layers: Vec<u8> = cur.diff(&prev).map(|(l, _, _, _)| l).collect();
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

    #[test]
    fn test_grid_clone_is_independent() {
        let mut g = Grid::new(3, 3);
        g.put_tile(0, 0, 0, Tile::new('A', Style::default()));
        g.put_tile(2, 1, 1, Tile::new('B', Style::default()));

        let mut cloned = g.clone();
        assert_eq!(cloned.get(0, 0).glyph, 'A');
        assert_eq!(cloned.get_tile(2, 1, 1).unwrap().glyph, 'B');
        assert_eq!(cloned.max_layer(), g.max_layer());

        // Mutating the clone must not affect the original (deep copy).
        cloned.put_tile(0, 0, 0, Tile::new('Z', Style::default()));
        assert_eq!(cloned.get(0, 0).glyph, 'Z');
        assert_eq!(g.get(0, 0).glyph, 'A');
    }

    // --- Extra grapheme text (EGC side-table) ---

    #[cfg(feature = "egc")]
    #[test]
    fn test_grid_write_grapheme_stores_and_reads_extra() {
        let mut g = Grid::new(5, 5);
        g.write_grapheme(0, 1, 1, "e\u{0301}", Style::default());
        assert_eq!(g.get(1, 1).glyph, 'e');
        assert_eq!(g.grapheme(0, 1, 1), Some("e\u{0301}"));

        // Single-codepoint writes never populate the side-table.
        g.write_grapheme(0, 2, 2, "a", Style::default());
        assert_eq!(g.grapheme(0, 2, 2), None);
    }

    #[cfg(feature = "egc")]
    #[test]
    fn test_grid_overwrite_clears_extra() {
        let mut g = Grid::new(5, 5);
        g.write_grapheme(0, 0, 0, "e\u{0301}", Style::default());
        assert_eq!(g.grapheme(0, 0, 0), Some("e\u{0301}"));

        // A plain `put` (or a later single-codepoint `write_grapheme`) must
        // drop the stale side-table entry, not just leave it unreachable.
        g.put(0, 0, Tile::new('X', Style::default()));
        assert_eq!(g.grapheme(0, 0, 0), None);
        assert!(!g.get(0, 0).flags().contains(TileFlags::HAS_EXTRA));
    }

    #[cfg(feature = "egc")]
    #[test]
    fn test_grid_resize_remaps_extras_to_new_stride() {
        let mut g = Grid::new(4, 4);
        g.write_grapheme(0, 3, 1, "e\u{0301}", Style::default());
        assert_eq!(g.grapheme(0, 3, 1), Some("e\u{0301}"));

        // Widening changes the row stride, so the flat index for (3, 1)
        // changes even though the cell itself is preserved.
        g.resize(8, 4);
        assert_eq!(g.get(3, 1).glyph, 'e');
        assert_eq!(g.grapheme(0, 3, 1), Some("e\u{0301}"));
        // No ghost entry landed on some other cell at the old flat index.
        assert_eq!(g.grapheme(0, 7, 0), None);

        // Shrinking past the cell drops its extras entry along with the tile.
        g.resize(2, 4);
        assert_eq!(g.grapheme(0, 3, 1), None);
    }

    #[cfg(feature = "egc")]
    #[test]
    fn test_grid_diff_detects_grapheme_only_change() {
        // Same glyph, style, and flags on both sides -- only the combining
        // mark differs. A `Tile`-only diff would miss this.
        let mut cur = Grid::new(2, 2);
        let mut prev = Grid::new(2, 2);
        cur.write_grapheme(0, 0, 0, "e\u{0301}", Style::default());
        prev.write_grapheme(0, 0, 0, "e\u{0300}", Style::default());

        let diffs: Vec<_> = cur.diff(&prev).collect();
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].1, Pos::new(0, 0));
        assert_eq!(diffs[0].3, Some("e\u{0301}"));

        // Identical grapheme text on both sides: no diff.
        let mut prev2 = Grid::new(2, 2);
        prev2.write_grapheme(0, 0, 0, "e\u{0301}", Style::default());
        assert_eq!(cur.diff(&prev2).count(), 0);
    }

    #[cfg(feature = "egc")]
    #[test]
    fn test_grid_blit_preserves_extra() {
        let mut src = Grid::new(2, 2);
        src.write_grapheme(0, 0, 0, "e\u{0301}", Style::default());

        let mut dst = Grid::new(2, 2);
        dst.blit(0, &src, Rect::new(0, 0, 2, 2), 0, 0);
        assert_eq!(dst.get(0, 0).glyph, 'e');
        assert_eq!(dst.grapheme(0, 0, 0), Some("e\u{0301}"));
    }

    // --- `BlendMode` / `blit_alpha` ---

    #[cfg(feature = "gem")]
    #[test]
    fn test_blend_separable_channel_screen() {
        // cb = 102 (0.4), cs = 204 (0.8): screen = cb + cs - cb*cs = 0.88.
        assert_eq!(
            blend_separable_channel(SeparableBlendMode::Screen, 204, 102, 1.0),
            224
        );
        // t = 0.5 lerps the destination halfway to that fully mixed color.
        assert_eq!(
            blend_separable_channel(SeparableBlendMode::Screen, 204, 102, 0.5),
            163
        );
    }

    #[cfg(feature = "gem")]
    #[test]
    fn test_blend_separable_channel_dodge() {
        // cb = 51 (0.2), cs = 204 (0.8): min(1, 0.2 / 0.2) saturates to 1.0.
        assert_eq!(
            blend_separable_channel(SeparableBlendMode::ColorDodge, 204, 51, 1.0),
            255
        );
        assert_eq!(
            blend_separable_channel(SeparableBlendMode::ColorDodge, 204, 51, 0.5),
            153
        );
    }

    #[cfg(feature = "gem")]
    #[test]
    fn test_blend_separable_channel_burn() {
        // cb = 204 (0.8), cs = 51 (0.2): 1 - min(1, 0.2 / 0.2) bottoms out at 0.0.
        assert_eq!(
            blend_separable_channel(SeparableBlendMode::ColorBurn, 51, 204, 1.0),
            0
        );
        assert_eq!(
            blend_separable_channel(SeparableBlendMode::ColorBurn, 51, 204, 0.5),
            102
        );
    }

    #[cfg(feature = "gem")]
    #[test]
    fn test_blend_separable_channel_overlay() {
        // cb = 51 (0.2, the <= 0.5 branch): 2 * cb * cs.
        assert_eq!(
            blend_separable_channel(SeparableBlendMode::Overlay, 204, 51, 1.0),
            82
        );
        assert_eq!(
            blend_separable_channel(SeparableBlendMode::Overlay, 204, 51, 0.5),
            66
        );
        // cb = 204 (0.8, the > 0.5 branch): 1 - 2 * (1 - cb) * (1 - cs).
        assert_eq!(
            blend_separable_channel(SeparableBlendMode::Overlay, 51, 204, 1.0),
            173
        );
    }

    /// End-to-end through `blit_alpha`, not just the per-channel helper: proves `BlendMode`
    /// actually reaches `blend_fg`/`blend_bg` and lands on the destination tile's style.
    #[cfg(feature = "gem")]
    #[test]
    fn test_grid_blit_alpha_screen_blends_fg() {
        let mut src = Grid::new(1, 1);
        src.put(
            0,
            0,
            Tile::default()
                .with_glyph('X')
                .with_style(Style::new().fg(Color::Rgb {
                    r: 204,
                    g: 204,
                    b: 204,
                })),
        );

        let mut dst = Grid::new(1, 1);
        dst.put(
            0,
            0,
            Tile::default()
                .with_glyph('_')
                .with_style(Style::new().fg(Color::Rgb {
                    r: 102,
                    g: 102,
                    b: 102,
                })),
        );

        dst.blit_alpha(
            0,
            &src,
            Rect::new(0, 0, 1, 1),
            0,
            0,
            BlendMode::Screen,
            1.0,
            1.0,
        );
        assert_eq!(
            dst.get(0, 0).style.fg,
            Color::Rgb {
                r: 224,
                g: 224,
                b: 224
            }
        );
    }

    /// `BlendMode::Linear` at `t == 0.0` keeps the destination and at `t == 1.0` uses the source
    /// -- matching `blit_alpha`'s doc comment (this direction was actually inverted before this
    /// change: the underlying `gem::rgb::Lerp` call had `src`/`dst` swapped, so `t == 0.0` used
    /// to return `src` and `t == 1.0` returned `dst`. No prior tests covered `blit_alpha`, so
    /// this had shipped unnoticed).
    #[cfg(feature = "gem")]
    #[test]
    fn test_grid_blit_alpha_linear_direction() {
        let mut src = Grid::new(1, 1);
        src.put(
            0,
            0,
            Tile::default()
                .with_glyph('X')
                .with_style(Style::new().fg(Color::Rgb {
                    r: 255,
                    g: 255,
                    b: 255,
                })),
        );

        let dst_color = Color::Rgb { r: 0, g: 0, b: 0 };
        let at = |t: f32| {
            let mut dst = Grid::new(1, 1);
            dst.put(
                0,
                0,
                Tile::default()
                    .with_glyph('_')
                    .with_style(Style::new().fg(dst_color)),
            );
            dst.blit_alpha(
                0,
                &src,
                Rect::new(0, 0, 1, 1),
                0,
                0,
                BlendMode::Linear,
                t,
                1.0,
            );
            dst.get(0, 0).style.fg
        };

        assert_eq!(at(0.0), dst_color);
        assert_eq!(
            at(1.0),
            Color::Rgb {
                r: 255,
                g: 255,
                b: 255
            }
        );
        let Color::Rgb { r, g, b } = at(0.5) else {
            panic!("expected Color::Rgb");
        };
        assert!(r > 0 && r < 255, "expected a mid-gray, got {r}");
        assert_eq!(r, g);
        assert_eq!(g, b);
    }

    /// Every `BlendMode` preserves `Color::Default` and passes non-RGB colors through unblended,
    /// same as the pre-existing `Linear` behavior.
    #[cfg(feature = "gem")]
    #[test]
    fn test_blend_color_non_rgb_passthrough_all_modes() {
        for mode in [
            BlendMode::Linear,
            BlendMode::Screen,
            BlendMode::Dodge,
            BlendMode::Burn,
            BlendMode::Overlay,
        ] {
            assert_eq!(
                blend_color(mode, Color::Default, Color::Rgb { r: 1, g: 2, b: 3 }, 0.5),
                Color::Default
            );
            assert_eq!(
                blend_color(mode, Color::BLACK, Color::WHITE, 0.5),
                Color::BLACK
            );
        }
    }

    #[cfg(feature = "egc")]
    #[test]
    fn test_grid_clone_preserves_extra() {
        let mut g = Grid::new(2, 2);
        g.write_grapheme(0, 0, 0, "e\u{0301}", Style::default());
        let cloned = g.clone();
        assert_eq!(cloned.grapheme(0, 0, 0), Some("e\u{0301}"));
    }

    #[cfg(feature = "egc")]
    #[test]
    fn test_grid_flatten_into_carries_extra_from_higher_layer() {
        let mut g = Grid::new(2, 2);
        g.write_grapheme(1, 0, 0, "e\u{0301}", Style::default());
        let mut flattened = Grid::new(2, 2);
        g.flatten_into(&mut flattened);
        assert_eq!(flattened.get(0, 0).glyph, 'e');
        assert_eq!(flattened.grapheme(0, 0, 0), Some("e\u{0301}"));
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
