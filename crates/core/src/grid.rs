//! The layered tile grid: [`Grid`], plus the [`Size`], [`Pos`], and [`Rect`]
//! coordinate types used throughout the crate.
//!
//! # Layers, draw order, and compositing
//!
//! A [`Grid`] holds up to 256 independent layers (`u8` ids `0..=255`), one
//! [`Tile`] per cell on each. Layer 0 is always allocated; layers 1-255 are
//! allocated lazily, on first write to that layer (see
//! [`put_tile`](Grid::put_tile), [`cells_mut_or_alloc`](Grid::cells_mut_or_alloc)): a
//! single-layer game pays zero overhead for layers it never writes to. This is the
//! crate's most distinctive feature and the one most worth understanding
//! before reaching for a second layer.
//!
//! Each cell carries a glyph, foreground/background [`Color`], and sub-cell pixel
//! offsets. [`Color`] covers the full spectrum: the terminal's default
//! foreground/background, the 16 standard ANSI colors, the 256-color palette, and 24-bit RGB.
//! [`Style`] has no text modifiers (bold, italic, underline, ...);
//! see its own doc comment for the full rationale.
//!
//! ## Draw order
//!
//! Layers composite bottom-to-top, in ascending id order: 0 first, then every
//! allocated layer up to [`max_layer`](Grid::max_layer), each painted over
//! whatever the layers below it produced. Layer id *is* z-order: there is
//! no separate depth or z-index to set. A common convention is layer 0 for
//! terrain, 1 for items, 2 for actors, 3+ for UI/effects, but the crate
//! enforces nothing; any id can hold any content.
//!
//! For overlapping *UI* specifically (chrome, popups, debug overlays, as opposed to a tile map's
//! own terrain/items/actors split), see [`crate::surface::Layer`] and
//! [`Surface::on_tier`](crate::surface::Surface::on_tier) for a small named convention and why
//! it beats ordering draw calls.
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
//!   other character. This is the one sharp edge in the model: `' '`
//!   painted on a higher layer *erases* content underneath, it does not
//!   reveal it.
//!
//! Background color follows its own rule, independent of `EMPTY`: a tile's
//! background only overwrites the composited background when it is not
//! [`Color::Default`]. A non-empty tile with a `Default` background still
//! lets a lower layer's background show through even though its glyph is
//! opaque. See `flatten_into` (crate-private) for the exact rule.
//!
//! ## Multi-cell spans
//!
//! [`write_span`](Grid::write_span) writes one piece of artwork across a `w x h` block of cells:
//! the top-left cell is the **anchor** ([`TileFlags::SPAN_ANCHOR`], carrying the footprint), and
//! every other cell is **covered** ([`TileFlags::SPAN_COVERED`], carrying its offset back to the
//! anchor). [`span_owner`](Grid::span_owner) resolves any cell of a span to its anchor in O(1),
//! so hit-testing a multi-cell sprite is one lookup rather than a rectangle scan.
//!
//! Covered cells keep **real glyphs**, and that is the point: they are the span's text fallback.
//! One `write_span` call renders correctly on every backend without a capability check.
//!
//! - A **cell backend** ignores `SPAN_COVERED` and prints all `w * h` glyphs, so `["C=", "[]"]`
//!   reads as a little piece of ASCII art.
//! - A **pixel backend** looks the anchor glyph up in its sprite cache, draws that one sprite
//!   across the whole footprint, and skips every covered cell's glyph.
//!
//! This is the deliberate difference from [`TileFlags::WIDE_CHAR_SPACER`], which every backend
//! skips: a wide character's spacer has no content of its own, whereas a covered cell does.
//!
//! A span is written and cleared whole. Any ordinary write into one of its cells
//! ([`put_tile`](Grid::put_tile), [`write_grapheme`](Grid::write_grapheme))
//! clears the entire span first, so an anchor can never be left claiming cells it no longer owns.
//! The exceptions are the escape hatches that hand out a `&mut Tile` directly
//! ([`tile_mut`](Grid::tile_mut), [`cells_mut`](Grid::cells_mut),
//! [`cells_mut_or_alloc`](Grid::cells_mut_or_alloc), `IndexMut`), which cannot intercept the
//! write; use [`clear_span`](Grid::clear_span) first if you reach for one of those on a grid
//! that uses spans.
//!
//! ## No short-circuiting: every allocated layer is visited, for every cell
//!
//! Compositing does not stop early when it hits an opaque tile on a high
//! layer. Both `flatten_into` (crate-private) and the software
//! backend's per-pixel compositor walk layers `0..=max_layer` in order for
//! *every* cell, unconditionally, even if a fully opaque tile on layer 5
//! makes layers 6-50 invisible at that position. An opaque high layer hides
//! the layers below it visually but never occludes them from the pass, so
//! prefer low, contiguous layer ids for frequently-updated content and
//! reserve high ids for rarely-touched overlays (e.g. a debug HUD pinned to
//! layer 255). See [`max_layer`](Grid::max_layer) for the iteration cost this
//! implies and [`Grid::new`] for the allocation cost of a first write.

use crate::backend::DrawCell;
use crate::color::Color;
use crate::style::Style;
use crate::tile::Tile;
use crate::tile::TileFlags;
#[cfg(feature = "egc")]
use crate::tile::cap_grapheme;
use crate::tint::Tint;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
// Aliased rather than imported as `BlendMode`: this module already defines its own `BlendMode`
// (below), and alpha-blend 0.3 renamed `blend_modes::SeparableBlendMode` to a top-level
// `BlendMode` of its own, which would otherwise collide.
#[cfg(feature = "blend-modes")]
use alpha_blend::BlendMode as SeparableBlendMode;
use core::fmt;
use core::ops::{Index, IndexMut};
use grixy::buf::GridBuf;
use grixy::ops::layout::RowMajor;
use grixy::ops::{ExactSizeGrid, GridRead, GridWrite};

/// Blend mode for [`Grid::blit_alpha`], selecting how source and destination colors combine
/// before the `fg_alpha`/`bg_alpha` factor is applied.
///
/// [`Linear`](Self::Linear) is a straight per-channel color lerp: `blit_alpha`'s original
/// behavior, always available (it needs only [`gem::Mix`], not the `blend-modes` feature). With
/// the `blend-modes` feature (default on), the remaining variants are also available: the
/// [W3C separable blend modes] libtcod also offers. Each computes a fully blended color per
/// channel via [`alpha_blend::BlendMode`] (imported here under its old name,
/// [`SeparableBlendMode`], to avoid colliding with this module's own [`BlendMode`]), and *that*
/// result is what gets lerped against the destination by the alpha factor, in place of the
/// source color `Linear` would use.
///
/// [W3C separable blend modes]: https://www.w3.org/TR/compositing-1/#blending
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BlendMode {
    /// Straight per-channel RGB lerp between destination and source.
    #[default]
    Linear,
    /// Lightens: `dst + src - dst * src`. Always at least as light as either input.
    ///
    /// Requires the `blend-modes` feature (default on).
    #[cfg(feature = "blend-modes")]
    Screen,
    /// Brightens the destination to reflect the source (aka "color dodge").
    ///
    /// Requires the `blend-modes` feature (default on).
    #[cfg(feature = "blend-modes")]
    Dodge,
    /// Darkens the destination to reflect the source (aka "color burn").
    ///
    /// Requires the `blend-modes` feature (default on).
    #[cfg(feature = "blend-modes")]
    Burn,
    /// Multiplies or screens the colors, depending on the destination.
    ///
    /// Requires the `blend-modes` feature (default on).
    #[cfg(feature = "blend-modes")]
    Overlay,
    /// Darkens: `dst * src`. Always at least as dark as either input; the complement of Screen.
    ///
    /// Requires the `blend-modes` feature (default on).
    #[cfg(feature = "blend-modes")]
    Multiply,
}

#[cfg(feature = "blend-modes")]
impl BlendMode {
    /// The equivalent [`SeparableBlendMode`], or `None` for [`Linear`](Self::Linear) (which uses
    /// [`gem::Mix`] instead: see [`blend_color`]).
    const fn separable(self) -> Option<SeparableBlendMode> {
        match self {
            Self::Linear => None,
            Self::Screen => Some(SeparableBlendMode::Screen),
            Self::Dodge => Some(SeparableBlendMode::ColorDodge),
            Self::Burn => Some(SeparableBlendMode::ColorBurn),
            Self::Overlay => Some(SeparableBlendMode::Overlay),
            Self::Multiply => Some(SeparableBlendMode::Multiply),
        }
    }
}

/// Size of the grid.
///
/// # Examples
///
/// ```
/// use retroglyph_core::Size;
///
/// let size = Size::new(80, 24);
/// assert_eq!(size.width(), 80);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Size {
    width: u16,
    height: u16,
}

impl Size {
    /// A size of `width` columns by `height` rows.
    #[must_use]
    pub const fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }

    /// The width, in columns.
    #[must_use]
    pub const fn width(self) -> u16 {
        self.width
    }

    /// The height, in rows.
    #[must_use]
    pub const fn height(self) -> u16 {
        self.height
    }
}

/// Pos in the grid, in (x = column, y = row) order.
///
/// Implements [`Ord`] in row-major order (y primary, then x), which is the
/// natural ordering for terminal rendering: top-to-bottom, left-to-right within
/// each row.
///
/// # Examples
///
/// ```
/// use retroglyph_core::Pos;
///
/// let pos = Pos::new(2, 1);
/// assert_eq!(pos.x, 2);
/// assert_eq!(pos.y, 1);
/// ```
///
/// This crate's `serde` feature forwards to [`ixy`]'s own `serde` feature, so `Pos` gains
/// `Serialize`/`Deserialize` from its upstream definition rather than one defined here.
pub type Pos = ixy::Pos<u16>;

/// Rectangle in the grid.
///
/// # Examples
///
/// ```
/// use retroglyph_core::Rect;
///
/// let rect = Rect::new(0, 0, 10, 4);
/// assert_eq!(rect.width(), 10);
/// assert_eq!(rect.height(), 4);
/// ```
///
/// This crate's `serde` feature forwards to [`ixy`]'s own `serde` feature, so `Rect` gains
/// `Serialize`/`Deserialize` from its upstream definition rather than one defined here.
pub type Rect = ixy::Rect<u16>;

/// A sub-cell pixel offset `(dx, dy)`, distinct from [`Pos`] so a caller can't transpose a
/// position and an offset in a call like [`Surface::put_offset`](crate::surface::Surface::put_offset).
///
/// Visual only: an offset shifts where a glyph is painted within its cell on backends that
/// support sub-cell placement (e.g. `retroglyph-software`); it never changes which cell a glyph
/// occupies, and cell-mode backends (e.g. `retroglyph-crossterm`) ignore it entirely.
///
/// # Examples
///
/// ```
/// use retroglyph_core::Offset;
///
/// let offset = Offset::new(3, -2);
/// assert_eq!(offset.dx, 3);
/// assert_eq!(offset.dy, -2);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Offset {
    /// Horizontal pixel offset.
    pub dx: i16,
    /// Vertical pixel offset.
    pub dy: i16,
}

impl Offset {
    /// Creates a new offset from `(dx, dy)`.
    #[must_use]
    pub const fn new(dx: i16, dy: i16) -> Self {
        Self { dx, dy }
    }
}

impl From<(i16, i16)> for Offset {
    fn from((dx, dy): (i16, i16)) -> Self {
        Self { dx, dy }
    }
}

impl From<Offset> for (i16, i16) {
    fn from(offset: Offset) -> Self {
        (offset.dx, offset.dy)
    }
}

impl From<(u16, u16)> for Size {
    fn from((width, height): (u16, u16)) -> Self {
        Self::new(width, height)
    }
}

impl From<Size> for (u16, u16) {
    fn from(s: Size) -> Self {
        (s.width(), s.height())
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
// LayerBuf: a single layer's flat buffer
// ---------------------------------------------------------------------------

/// A single layer in the grid: a flat 2D buffer of one tile per cell.
///
/// Layer 0 is always allocated. Layers 1–255 are allocated on first write
/// (see [`Grid::put_tile`]).
#[derive(Clone)]
pub(crate) struct LayerBuf {
    pub(crate) buf: GridBuf<Tile, Vec<Tile>, RowMajor>,
    /// Sparse side-table: flat row-major index -> the cell's out-of-line data, for tiles with
    /// [`TileFlags::HAS_EXTRA`] set. Empty until something writes a multi-codepoint grapheme or
    /// a tint, which is what keeps [`Tile`] itself small (see [`Grid::grapheme`] and
    /// [`Grid::tint`]).
    ///
    /// The `HAS_EXTRA` flag is authoritative: readers must check it before
    /// consulting this map, since some write paths (`put_tile`,
    /// `IndexMut`, `cells_mut`, `cells_mut_or_alloc`) can leave a stale entry behind when they
    /// overwrite a tile that used to carry extra data without an explicit
    /// cleanup call. Since those paths only ever hand out or store tiles
    /// with `HAS_EXTRA` clear, a stale entry is harmless: it is simply
    /// never looked up until the slot is reused by `write_grapheme` or `set_tint`, which
    /// always overwrite it.
    extras: BTreeMap<usize, TileExtra>,
}

/// One cell's out-of-line data: everything that belongs to a tile but does not fit in one.
///
/// [`Tile`] is exactly 20 bytes with no padding to spare, and both members here are rare enough
/// per cell that inlining either would grow every tile of every layer to pay for a minority of
/// them. They share one table, one flag, and one set of rekeying paths rather than each bringing
/// their own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TileExtra {
    /// The full grapheme cluster, when [`Tile::glyph`] holds only its first codepoint.
    pub(crate) grapheme: Option<Arc<str>>,
    /// How a pixel backend recolours this cell's sprite.
    pub(crate) tint: Tint,
}

impl TileExtra {
    /// Whether this entry carries nothing, and so should be dropped rather than stored.
    ///
    /// Keeping the table free of empty entries is what lets `HAS_EXTRA` be set exactly when an
    /// entry exists, instead of the flag and the table disagreeing about an all-default value.
    fn is_empty(&self) -> bool {
        self.grapheme.is_none() && self.tint == Tint::None
    }
}

impl LayerBuf {
    fn new(width: u16, height: u16) -> Self {
        let n = usize::from(width) * usize::from(height);
        Self {
            buf: GridBuf::from_buffer(alloc::vec![Tile::default(); n], usize::from(width)),
            extras: BTreeMap::new(),
        }
    }

    /// Returns the side-table entry for the tile at flat index `idx`, or `None` if `tile`
    /// doesn't have [`TileFlags::HAS_EXTRA`] set.
    fn entry_for(&self, idx: usize, tile: &Tile) -> Option<&TileExtra> {
        if tile.flags.contains(TileFlags::HAS_EXTRA) {
            self.extras.get(&idx)
        } else {
            None
        }
    }

    /// Returns the grapheme text for the tile at flat index `idx`, or `None`
    /// if `tile` doesn't have [`TileFlags::HAS_EXTRA`] set.
    fn extra_for(&self, idx: usize, tile: &Tile) -> Option<&str> {
        self.entry_for(idx, tile)?.grapheme.as_deref()
    }

    /// Returns the tint for the tile at flat index `idx`, or [`Tint::None`] if `tile` doesn't
    /// have [`TileFlags::HAS_EXTRA`] set.
    fn tint_for(&self, idx: usize, tile: &Tile) -> Tint {
        self.entry_for(idx, tile).map_or(Tint::None, |e| e.tint)
    }

    /// Returns a clone of the side-table entry at flat index `idx`, or `None` if `tile` doesn't
    /// have [`TileFlags::HAS_EXTRA`] set. Used to copy a cell's out-of-line data between grids
    /// (e.g. [`Grid::blit`]); the grapheme rides along as an `Arc` clone rather than a fresh
    /// allocation.
    fn extra_entry_for(&self, idx: usize, tile: &Tile) -> Option<TileExtra> {
        self.entry_for(idx, tile).cloned()
    }
}

// ---------------------------------------------------------------------------
// Grid
// ---------------------------------------------------------------------------

/// A 2D buffer of [`Tile`]s, addressable across up to 256 stacked layers.
///
/// Layer 0 is always allocated; higher layers are allocated on first write, growing the
/// layer-table `Vec` up to that layer's id as needed (see [`Grid::new`]). Single-layer use pays
/// no overhead: layers 1+ stay unallocated until used, and the layer table itself never grows
/// past a single slot.
///
/// # Out-of-bounds drawing
///
/// Drawing off the grid is a no-op, the same convention as drawing off-screen: every write method
/// that names a position or region (e.g. [`put_tile`](Self::put_tile), [`write_grapheme`](Self::write_grapheme),
/// [`write_span`](Self::write_span), [`blit`](Self::blit)) silently discards any part of the
/// write that falls outside `0..width` / `0..height`, rather than panicking. The one deliberate
/// exception is indexing (`Index<Pos>`/`IndexMut<Pos>`, and by extension anything built on it),
/// which panics on an out-of-bounds `Pos` the same way indexing a slice does. Read accessors
/// that take a position (e.g. [`tile`](Self::tile)) report an out-of-bounds position as `None`,
/// indistinguishable from an unallocated layer.
///
/// Requires an allocator (backed by `alloc::vec::Vec`), so it is unavailable
/// in strictly static, no-alloc environments.
///
/// # Examples
///
/// ```
/// use retroglyph_core::{Color, Grid, Pos, Style};
///
/// let mut grid = Grid::new(10, 5);
/// grid.put_tile(0, Pos::new(2, 1), retroglyph_core::Tile::new('@', Style::new().fg(Color::GREEN)));
/// assert_eq!(grid[Pos::new(2, 1)].glyph(), '@');
/// ```
#[derive(Clone)]
pub struct Grid {
    width: u16,
    height: u16,
    /// Indexed by layer ID (0–255), but only as long as the highest layer id ever written to
    /// (see [`layer_or_alloc`](Self::layer_or_alloc)), not always all 256 slots. Index 0 is
    /// always `Some`. Unwritten layers within the current length are `None`; ids past the end
    /// are treated identically to a `None` slot (see [`layer`](Self::layer)).
    layers: Vec<Option<LayerBuf>>,
    /// Highest layer ID that has been allocated. Always at least 0.
    max_layer: u8,
    /// Whether any multi-cell span has ever been written to this grid (see
    /// [`write_span`](Self::write_span)).
    ///
    /// Conservative and one-way: set on the first `write_span`, never cleared. Every ordinary
    /// write has to clear a span it would partially overwrite
    /// (`clear_span_overlap`), and this flag is what keeps that check from
    /// costing a buffer read per `put` in the overwhelmingly common grid that never uses a span
    /// at all: it degrades to one `bool` test. Clearing it again on the last span's removal
    /// would need span refcounting for no observable gain.
    has_spans: bool,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

impl Grid {
    /// Borrow a specific layer, or `None` if unallocated.
    ///
    /// `id` may be beyond the current layer-table `Vec`'s length: the table only grows as far
    /// as the highest layer id ever written (see [`layer_or_alloc`](Self::layer_or_alloc)), so an
    /// id past the end simply means "never written", same as an in-bounds `None` slot.
    fn layer(&self, id: u8) -> Option<&LayerBuf> {
        self.layers.get(usize::from(id))?.as_ref()
    }

    /// Borrow a specific layer mutably, allocating it if necessary.
    ///
    /// Grows the layer-table `Vec` up to `id + 1` slots on demand, rather than the table always
    /// holding all 256 possible slots (see retroglyph#264): a `Grid` that only ever writes to
    /// layer 0, or a handful of low ids, never pays for the 250+ slots it never touches.
    ///
    /// A first write allocates one `width x height` buffer of [`Tile`]s, the same cost regardless
    /// of the layer's id, plus that one-time table growth up to `id + 1`. Writing to layer 200
    /// first grows the table to 201 slots and allocates layer 200's buffer; the untouched slots
    /// 1-199 in between are a cheap `None`. What the id costs afterward is steady-state iteration,
    /// not allocation: see [`max_layer`](Self::max_layer).
    fn layer_or_alloc(&mut self, id: u8) -> &mut LayerBuf {
        let idx = usize::from(id);
        if idx >= self.layers.len() {
            self.layers.resize_with(idx + 1, || None);
        }
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
// Grid: public API (all forward to layer 0)
// ---------------------------------------------------------------------------

impl Grid {
    /// Creates a new grid of the given dimensions.
    ///
    /// Layer 0 is allocated immediately. Layers 1–255 are `None` until first
    /// write via [`put_tile`](Self::put_tile); the layer table itself only
    /// grows as far as the highest layer id ever written, not all 256 slots
    /// up front.
    #[must_use]
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            layers: alloc::vec![Some(LayerBuf::new(width, height))],
            max_layer: 0,
            has_spans: false,
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
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{Grid, Pos, Style, Tile};
    ///
    /// // A ragged map: the second line is shorter than the first.
    /// let grid = Grid::from_charmap("###\n#.", |c| match c {
    ///     '#' => Tile::new('#', Style::default()),
    ///     _ => Tile::default(),
    /// });
    ///
    /// // Width comes from the longest line; the shorter line is padded with the default
    /// // tile rather than truncating the grid to the shortest line.
    /// assert_eq!((grid.width(), grid.height()), (3, 2));
    /// assert_eq!(grid[Pos::new(0, 0)].glyph(), '#');
    /// assert_eq!(grid[Pos::new(1, 1)].glyph(), ' '); // '.' maps to the default tile
    /// assert_eq!(grid[Pos::new(2, 1)].glyph(), ' '); // padding past the short line's end
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
                grid.put_tile(0, Pos::new(x, y), f(ch));
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
    /// clearing a layer ([`clear`](Self::clear)) does not deallocate it, so
    /// the value does not shrink once a higher layer has been written.
    ///
    /// This is the layer id's steady-state cost: every present, diff, and
    /// full-grid iteration walks `0..=max_layer`, skipping unallocated slots
    /// with an O(1) `None` check, so compositing is `O(max_layer)` per cell
    /// rather than `O(topmost opaque layer)`. Writing once to layer 200 and
    /// never touching layers 1-199 means every future frame walks past 199
    /// `None` slots to reach it: cheap per skipped layer, but not free, which
    /// is why low, contiguous ids are preferred for frequently-updated
    /// content.
    #[must_use]
    pub const fn max_layer(&self) -> u8 {
        self.max_layer
    }

    /// Returns the full grapheme cluster stored for the tile at `(x, y)` on
    /// `layer`, if any.
    ///
    /// `Some` only when the tile has [`TileFlags::HAS_EXTRA`] set, i.e. it
    /// was written via [`write_grapheme`](Self::write_grapheme) with a
    /// multi-codepoint EGC (combining marks, ZWJ sequences, etc.). For the
    /// common single-codepoint case, or without the `egc` feature, this is
    /// always `None`; use [`tile`](Self::tile)'s
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
    /// Returns `None` if the layer is unallocated, mirroring [`cells`](Self::cells)'s
    /// fallibility. Use [`cells_mut_or_alloc`](Self::cells_mut_or_alloc) to allocate the layer
    /// first instead of failing.
    pub fn cells_mut(&mut self, layer: u8) -> Option<CellsMut<'_>> {
        let width = usize::from(self.width);
        let lb = self.layers.get_mut(usize::from(layer))?.as_mut()?;
        Some(CellsMut {
            iter: lb.buf.as_mut().iter_mut().enumerate(),
            width,
        })
    }

    /// Iterates all tiles on `layer` mutably with their `(x, y)` coordinates, allocating the
    /// layer first if it has not been written to yet.
    ///
    /// Prefer [`cells_mut`](Self::cells_mut) unless an empty layer legitimately needs to exist
    /// after this call returns; unlike that method, this one never fails, at the cost of always
    /// allocating.
    pub fn cells_mut_or_alloc(&mut self, layer: u8) -> CellsMut<'_> {
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
        if let Some(lb) = self
            .layers
            .get_mut(usize::from(layer))
            .and_then(Option::as_mut)
        {
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
            // shifts whenever the width changes: remap it in lockstep with
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
    // Write grapheme: layer 0 only
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
    /// Also does nothing if the grapheme has zero display width, or if a 2-column wide character
    /// would overflow the grid (the last column needs both its own cell and a spacer).
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

        // Clear any wide-char cell, or any multi-cell span, that would be partially overwritten.
        self.clear_span_overlap(layer, x, y, width);
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
        // `width` here is the full grapheme's display width (1 or 2), not just `first`'s: more
        // accurate than recomputing from the primary codepoint alone, and exactly what the
        // terminal renderer needs to advance the cursor after printing this cell.
        #[allow(clippy::cast_possible_truncation)]
        {
            lb.buf.as_mut()[idx].width = width as u8;
        }
        // A fresh glyph write replaces the cell's out-of-line data outright rather than merging
        // with it: a tint belongs to the artwork that was drawn here, not to the cell, so
        // overwriting the glyph drops it. `Grid::set_tint` is the follow-up that puts one back.
        if has_extra {
            lb.extras.insert(
                idx,
                TileExtra {
                    grapheme: Some(Arc::from(cap_grapheme(grapheme))),
                    tint: Tint::None,
                },
            );
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
    ///
    /// `clear_span_overlap` is the multi-cell-span analogue. It is not `egc`-gated, because a
    /// span is not a Unicode concept and exists on every feature combination, so a write that
    /// can land inside either kind of multi-cell structure calls both.
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
// Grid: multi-cell spans
// ---------------------------------------------------------------------------

impl Grid {
    /// Writes a multi-cell span at `(x, y)` on `layer`: one piece of artwork occupying a block of
    /// cells rather than one.
    ///
    /// `rows` holds one string per row of the footprint, so the span is `rows.len()` cells tall
    /// and `rows[0]`'s character count wide, and every row must be that same width. Any
    /// `AsRef<str>` row works, so a literal footprint (`&["[==]", "|__|"]`) and a computed one
    /// (`&Vec<String>`) both pass without a borrowing pass over the rows. The first
    /// character goes to the **anchor** cell at `(x, y)` with [`TileFlags::SPAN_ANCHOR`]; each
    /// remaining character goes to its own cell with [`TileFlags::SPAN_COVERED`]. `style` applies
    /// to every cell.
    ///
    /// # Text fallback
    ///
    /// The covered cells keep real glyphs, which is what lets one call render correctly on every
    /// backend with no capability check:
    ///
    /// - A **cell backend** (`Headless`, `retroglyph-crossterm`, `retroglyph-terminal`) ignores
    ///   [`TileFlags::SPAN_COVERED`] and prints all of them, so `["[==]", "|__|"]` reads as a
    ///   small piece of ASCII art.
    /// - A **pixel backend** (`retroglyph-software`, `retroglyph-gl`) looks the anchor glyph up in
    ///   its sprite cache, draws that one sprite across the whole footprint, and skips every
    ///   covered cell's glyph.
    ///
    /// This is the deliberate difference from [`TileFlags::WIDE_CHAR_SPACER`], which every
    /// backend skips.
    ///
    /// Any existing span or wide character the footprint would partially overwrite is cleared
    /// first, in full, as [`write_grapheme`](Self::write_grapheme) does for its own 1- or 2-cell
    /// write.
    ///
    /// For the common sprite case (one runtime-chosen anchor glyph, blanks in every covered
    /// cell), [`write_span_uniform`](Self::write_span_uniform) says the same thing without
    /// building the rows.
    ///
    /// # Returns
    ///
    /// `Some(())` once the whole span is written, or `None` having written nothing at all when
    /// `rows` is empty, its first row is empty, its rows differ in width, either axis exceeds 255
    /// cells, or the footprint would not fit in the grid at `(x, y)`.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() {
    /// # fn run() -> Option<()> {
    /// use retroglyph_core::{Grid, Pos, Style};
    ///
    /// let mut grid = Grid::new(8, 4);
    /// grid.write_span(0, 1, 1, &["[==]", "|__|"], Style::default())?;
    ///
    /// assert_eq!(grid.tile(0, Pos::new(1, 1))?.span(), (4, 2));
    /// // Covered cells keep their fallback glyphs, and name their anchor.
    /// assert_eq!(grid.tile(0, Pos::new(4, 2))?.glyph(), '|');
    /// assert_eq!(grid.span_owner(0, 4, 2), Some(Pos::new(1, 1)));
    /// # Some(())
    /// # }
    /// # run().unwrap();
    /// # }
    /// ```
    pub fn write_span<S: AsRef<str>>(
        &mut self,
        layer: u8,
        x: u16,
        y: u16,
        rows: &[S],
        style: Style,
    ) -> Option<()> {
        let cols = rows.first()?.as_ref().chars().count();
        if cols == 0 || rows.iter().any(|r| r.as_ref().chars().count() != cols) {
            return None;
        }
        // `Tile` stores a span's dimensions in one byte each (see `Tile::span_w`), so a span
        // wider or taller than 255 cells is not representable.
        let footprint = (u8::try_from(cols).ok()?, u8::try_from(rows.len()).ok()?);

        self.write_span_cells(
            layer,
            Pos::new(x, y),
            footprint,
            style,
            rows.iter().map(|row| row.as_ref().chars()),
        )
    }

    /// Writes a `size` multi-cell span at `pos` on `layer`: `anchor` in the anchor cell, `fill`
    /// in every other cell of the footprint.
    ///
    /// The uniform case of [`write_span`](Self::write_span), and the shape a sheet-driven
    /// renderer usually wants: one sprite, chosen at runtime, with the cells it covers blanked so
    /// nothing shows through its transparent pixels. Spelling that as an array of blank rows
    /// carries no information and, for a computed anchor, has to be allocated per draw.
    ///
    /// `fill` is what a *cell* backend prints for the covered cells (a pixel backend skips them
    /// and draws the sprite instead), so it is the span's text fallback: `' '` blanks them, and a
    /// visible character keeps the footprint legible in a terminal. See
    /// [`write_span`](Self::write_span) for the full write semantics.
    ///
    /// # Returns
    ///
    /// `Some(())` once the whole span is written, or `None` having written nothing at all when
    /// either axis of `size` is `0` or exceeds 255 cells, or the footprint would not fit in the
    /// grid at `pos`.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() {
    /// # fn run() -> Option<()> {
    /// use retroglyph_core::{Grid, Pos, Style};
    ///
    /// let mut grid = Grid::new(8, 4);
    /// let anchor = '\u{E000}'; // chosen at runtime from a tilesheet
    /// grid.write_span_uniform(0, (1, 1), (2, 2), anchor, ' ', Style::default())?;
    ///
    /// assert_eq!(grid.tile(0, Pos::new(1, 1))?.span(), (2, 2));
    /// assert_eq!(grid.span_owner(0, 2, 2), Some(Pos::new(1, 1)));
    /// # Some(())
    /// # }
    /// # run().unwrap();
    /// # }
    /// ```
    pub fn write_span_uniform(
        &mut self,
        layer: u8,
        pos: impl Into<Pos>,
        size: impl Into<Size>,
        anchor: char,
        fill: char,
        style: Style,
    ) -> Option<()> {
        let size = size.into();
        // `Tile` stores a span's dimensions in one byte each (see `Tile::span_w`), so a span
        // wider or taller than 255 cells is not representable.
        let footprint = (
            u8::try_from(size.width()).ok()?,
            u8::try_from(size.height()).ok()?,
        );
        if footprint.0 == 0 || footprint.1 == 0 {
            return None;
        }

        let rows = (0..footprint.1).map(move |row| {
            (0..footprint.0).map(move |col| if (row, col) == (0, 0) { anchor } else { fill })
        });
        self.write_span_cells(layer, pos.into(), footprint, style, rows)
    }

    /// Writes a `footprint` (`w`, `h`) span at `pos` on `layer`, taking its glyphs row by row.
    ///
    /// The shared body of [`write_span`](Self::write_span) and
    /// [`write_span_uniform`](Self::write_span_uniform): both have already narrowed the footprint
    /// to a `u8` per axis, so all that is left is the grid-fit check and the write itself.
    /// `rows` must yield exactly `footprint.1` rows of exactly `footprint.0` glyphs.
    fn write_span_cells<R: Iterator<Item = char>>(
        &mut self,
        layer: u8,
        pos: Pos,
        footprint: (u8, u8),
        style: Style,
        rows: impl Iterator<Item = R>,
    ) -> Option<()> {
        let (footprint_w, footprint_h) = footprint;
        let (x, y) = (pos.x, pos.y);

        let grid_w = usize::from(self.width);
        if usize::from(x) + usize::from(footprint_w) > grid_w
            || usize::from(y) + usize::from(footprint_h) > usize::from(self.height)
        {
            return None;
        }

        // Clear anything the footprint would partially overwrite. Every rejection above happens
        // first, so a refused write can never have already destroyed the caller's content.
        for row in 0..footprint_h {
            let cy = y + u16::from(row);
            self.clear_span_overlap(layer, x, cy, u16::from(footprint_w));
            #[cfg(feature = "egc")]
            self.clear_overlap(layer, x, cy, u16::from(footprint_w));
        }

        self.has_spans = true;
        let lb = self.layer_or_alloc(layer);
        for (row, line) in rows.enumerate() {
            for (col, ch) in line.enumerate() {
                let idx = (usize::from(y) + row) * grid_w + usize::from(x) + col;
                let mut tile = Tile::new(ch, style);
                if row == 0 && col == 0 {
                    tile.flags = TileFlags::SPAN_ANCHOR;
                    tile.span_w = footprint_w;
                    tile.span_h = footprint_h;
                } else {
                    // Both fit in a `u8`: they are strictly less than the footprint, which was
                    // already narrowed to one above.
                    #[allow(clippy::cast_possible_truncation)]
                    {
                        tile.flags = TileFlags::SPAN_COVERED;
                        tile.span_w = col as u8;
                        tile.span_h = row as u8;
                    }
                }
                lb.buf.as_mut()[idx] = tile;
                lb.extras.remove(&idx);
            }
        }
        Some(())
    }

    /// The anchor of the multi-cell span occupying `(x, y)` on `layer`, or `None` when the cell
    /// belongs to no span or is out of bounds.
    ///
    /// An anchor cell reports itself, so every cell of one span answers with the same position
    /// and hit-testing multi-cell artwork is a single comparison:
    ///
    /// ```
    /// # fn main() {
    /// # fn run() -> Option<()> {
    /// # use retroglyph_core::{Grid, Pos, Style};
    /// # let mut grid = Grid::new(8, 4);
    /// grid.write_span(0, 2, 1, &["[==]", "|__|"], Style::default())?;
    /// let chest = Pos::new(2, 1);
    /// // Any of the eight cells counts as standing on the chest.
    /// assert_eq!(grid.span_owner(0, 2, 1), Some(chest));
    /// assert_eq!(grid.span_owner(0, 5, 2), Some(chest));
    /// assert_eq!(grid.span_owner(0, 6, 2), None);
    /// # Some(())
    /// # }
    /// # run().unwrap();
    /// # }
    /// ```
    ///
    /// O(1): a covered tile stores its offset back to the anchor (see [`Tile::span_offset`]), so
    /// this is a lookup and a subtraction, not a scan.
    #[must_use]
    pub fn span_owner(&self, layer: u8, x: u16, y: u16) -> Option<Pos> {
        self.span_anchor_at(layer, x, y)
    }

    /// Clears the whole multi-cell span that `(x, y)` on `layer` belongs to, anchor included,
    /// resetting every one of its cells to the default (empty) tile.
    ///
    /// Works from any cell of the span, so it pairs with [`span_owner`](Self::span_owner): hit-test
    /// a cell, then clear the artwork it belongs to. Does nothing if the cell is not part of a
    /// span, is out of bounds, or the layer is unallocated.
    pub fn clear_span(&mut self, layer: u8, x: u16, y: u16) {
        if let Some(anchor) = self.span_anchor_at(layer, x, y) {
            self.reset_span_at(layer, anchor);
        }
    }

    /// The anchor of the span `(x, y)` belongs to, treating an anchor cell as its own anchor.
    fn span_anchor_at(&self, layer: u8, x: u16, y: u16) -> Option<Pos> {
        let tile = self.layer(layer)?.buf.get(to_grixy_pos(Pos::new(x, y)))?;
        if tile.flags.contains(TileFlags::SPAN_ANCHOR) {
            return Some(Pos::new(x, y));
        }
        let (dx, dy) = tile.span_offset()?;
        Some(Pos::new(x.checked_sub(dx)?, y.checked_sub(dy)?))
    }

    /// Resets every cell of the span anchored at `anchor` on `layer`. No-op if that cell is not
    /// a [`TileFlags::SPAN_ANCHOR`], or the layer is unallocated.
    fn reset_span_at(&mut self, layer: u8, anchor: Pos) {
        let w = usize::from(self.width);
        let h = usize::from(self.height);
        let Some(lb) = self
            .layers
            .get_mut(usize::from(layer))
            .and_then(Option::as_mut)
        else {
            return;
        };
        let anchor_idx = usize::from(anchor.y) * w + usize::from(anchor.x);
        let Some(anchor_tile) = lb.buf.as_ref().get(anchor_idx).copied() else {
            return;
        };
        if !anchor_tile.flags.contains(TileFlags::SPAN_ANCHOR) {
            return;
        }
        for row in 0..usize::from(anchor_tile.span_h) {
            let cy = usize::from(anchor.y) + row;
            if cy >= h {
                break;
            }
            for col in 0..usize::from(anchor_tile.span_w) {
                let cx = usize::from(anchor.x) + col;
                if cx >= w {
                    break;
                }
                let idx = cy * w + cx;
                lb.buf.as_mut()[idx].reset();
                lb.extras.remove(&idx);
            }
        }
    }

    /// Clears every multi-cell span that a `width`-cell write starting at `(x, y)` on `layer`
    /// would partially overwrite.
    ///
    /// The span analogue of [`clear_overlap`](Self::clear_overlap), and the reason every ordinary
    /// write path calls it: overwriting one cell of a span would otherwise leave an anchor
    /// claiming cells it no longer owns, or a covered cell pointing at an anchor that is gone.
    ///
    /// Returns immediately on a grid that has never had a span written to it, which is what keeps
    /// this off the cost of an ordinary [`put_tile`](Self::put_tile) (see
    /// [`has_spans`](Self::has_spans)).
    fn clear_span_overlap(&mut self, layer: u8, x: u16, y: u16, width: u16) {
        if !self.has_spans {
            return;
        }
        // Collect first: resetting a span mutates cells this scan is still reading. Overlapping
        // writes touch at most a handful of spans, so the linear `contains` beats a set.
        let mut anchors: Vec<Pos> = Vec::new();
        let Some(lb) = self.layer(layer) else {
            return;
        };
        for cx in x..x.saturating_add(width) {
            let Some(tile) = lb.buf.get(to_grixy_pos(Pos::new(cx, y))) else {
                continue;
            };
            let anchor = if tile.flags.contains(TileFlags::SPAN_ANCHOR) {
                Pos::new(cx, y)
            } else if let Some((dx, dy)) = tile.span_offset() {
                match (cx.checked_sub(dx), y.checked_sub(dy)) {
                    (Some(ax), Some(ay)) => Pos::new(ax, ay),
                    _ => continue,
                }
            } else {
                continue;
            };
            if !anchors.contains(&anchor) {
                anchors.push(anchor);
            }
        }
        for anchor in anchors {
            self.reset_span_at(layer, anchor);
        }
    }
}

// ---------------------------------------------------------------------------
// Grid: multi-layer API
// ---------------------------------------------------------------------------

impl Grid {
    /// Write a tile to `layer` at `pos`.
    ///
    /// Allocates the layer if it has not been written to yet. Returns `None`
    /// if `pos` is out of bounds.
    ///
    /// To read back, use [`tile`](Self::tile).
    ///
    /// Any tile written this way has its extra grapheme text cleared, since a
    /// caller-constructed [`Tile`] can never legitimately carry
    /// [`TileFlags::HAS_EXTRA`] (the flag is crate-private). Internal callers
    /// that need to preserve EGC text across a copy (e.g. [`blit`](Self::blit))
    /// follow up with a direct extras-table write. Any multi-cell span the
    /// cell belongs to is cleared first, so a write can never leave an anchor
    /// pointing at cells it no longer owns.
    pub fn put_tile(&mut self, layer: u8, pos: impl Into<Pos>, mut tile: Tile) -> Option<()> {
        let pos = pos.into();
        self.clear_span_overlap(layer, pos.x, pos.y, 1);
        let gpos = to_grixy_pos(pos);
        let idx = usize::from(pos.y) * usize::from(self.width) + usize::from(pos.x);
        let lb = self.layer_or_alloc(layer);
        if !lb.buf.contains(gpos) {
            return None;
        }
        lb.extras.remove(&idx);
        tile.flags.remove(TileFlags::HAS_EXTRA);
        lb.buf[gpos] = tile;
        Some(())
    }

    /// Sets the whole side-table entry for an already-written tile at `(x, y)` on `layer`,
    /// setting [`TileFlags::HAS_EXTRA`] to match. Does nothing if out of bounds. Crate-private:
    /// the external ways in are [`write_grapheme`](Self::write_grapheme) and
    /// [`set_tint`](Self::set_tint).
    ///
    /// An empty entry is removed rather than stored, so the flag means exactly "an entry
    /// exists".
    pub(crate) fn set_extra(&mut self, layer: u8, x: u16, y: u16, extra: TileExtra) {
        let pos = to_grixy_pos(Pos::new(x, y));
        let idx = usize::from(y) * usize::from(self.width) + usize::from(x);
        let lb = self.layer_or_alloc(layer);
        if lb.buf.contains(pos) {
            if extra.is_empty() {
                lb.buf[pos].flags.remove(TileFlags::HAS_EXTRA);
                lb.extras.remove(&idx);
            } else {
                lb.buf[pos].flags.insert(TileFlags::HAS_EXTRA);
                lb.extras.insert(idx, extra);
            }
        }
    }

    /// How a pixel backend recolours the sprite drawn for the cell at `(x, y)` on `layer`.
    ///
    /// [`Tint::None`] for a cell that has never been tinted, for a cell whose glyph was
    /// overwritten since (a glyph write drops the tint with the artwork it belonged to), and for
    /// coordinates outside the grid or on an unallocated layer.
    ///
    /// A tint is grid state rather than [`Tile`] state, for the same reason a multi-codepoint
    /// grapheme is (see [`grapheme`](Self::grapheme)): it is rare per cell and `Tile` has no room
    /// left. So it is read here, not through [`Tile::style`].
    ///
    /// Cell backends have no sprite to recolour and ignore this entirely.
    #[must_use]
    pub fn tint(&self, layer: u8, x: u16, y: u16) -> Tint {
        let Some(lb) = self.layer(layer) else {
            return Tint::None;
        };
        let Some(tile) = lb.buf.get(to_grixy_pos(Pos::new(x, y))) else {
            return Tint::None;
        };
        let idx = usize::from(y) * usize::from(self.width) + usize::from(x);
        lb.tint_for(idx, tile)
    }

    /// Sets how a pixel backend recolours the sprite drawn for the cell at `(x, y)` on `layer`.
    ///
    /// Applies to the cell as it stands, so it belongs *after* the write that put the glyph
    /// there: writing a glyph over a tinted cell drops the tint, on the grounds that a tint
    /// describes the artwork rather than the position. For a multi-cell span, tint the anchor;
    /// that is the cell a pixel backend draws the sprite from.
    ///
    /// Setting [`Tint::None`] clears the tint, and drops the cell's side-table entry entirely if
    /// it held nothing else. Does nothing if `(x, y)` is out of bounds.
    pub fn set_tint(&mut self, layer: u8, x: u16, y: u16, tint: Tint) {
        let idx = usize::from(y) * usize::from(self.width) + usize::from(x);
        let pos = to_grixy_pos(Pos::new(x, y));
        let lb = self.layer_or_alloc(layer);
        if !lb.buf.contains(pos) {
            return;
        }
        // Preserve any grapheme already stored for this cell: the two members of the entry are
        // written by separate calls and neither should clobber the other.
        let grapheme = if lb.buf[pos].flags.contains(TileFlags::HAS_EXTRA) {
            lb.extras.get(&idx).and_then(|e| e.grapheme.clone())
        } else {
            None
        };
        let entry = TileExtra { grapheme, tint };
        if entry.is_empty() {
            lb.buf[pos].flags.remove(TileFlags::HAS_EXTRA);
            lb.extras.remove(&idx);
        } else {
            lb.buf[pos].flags.insert(TileFlags::HAS_EXTRA);
            lb.extras.insert(idx, entry);
        }
    }

    /// Read a tile on `layer` at `pos`, or `None` if the layer is
    /// unallocated or `pos` is out of bounds.
    #[must_use]
    pub fn tile(&self, layer: u8, pos: impl Into<Pos>) -> Option<&Tile> {
        let pos = to_grixy_pos(pos.into());
        self.layer(layer)?.buf.get(pos)
    }

    /// Mutably borrow a tile on `layer` at `pos`, or `None` if the layer is
    /// unallocated or `pos` is out of bounds.
    ///
    /// This hands out a direct `&mut Tile`, so it cannot intercept a write the way
    /// [`put_tile`](Self::put_tile) does: it does not clear a multi-cell span `pos` belongs to,
    /// and it does not clear grapheme extras stored for the tile. Call
    /// [`clear_span`](Self::clear_span) first if `pos` may belong to a span.
    pub fn tile_mut(&mut self, layer: u8, pos: impl Into<Pos>) -> Option<&mut Tile> {
        let pos = to_grixy_pos(pos.into());
        self.layers
            .get_mut(usize::from(layer))?
            .as_mut()?
            .buf
            .get_mut(pos)
    }

    /// Copy tiles from `src` within `src_rect` to `self` at `(dst_x, dst_y)`
    /// on `layer`. Empty tiles (nothing written; see [`Tile::is_empty`]) are
    /// treated as transparent and skipped. An explicit space is copied and
    /// overwrites the destination.
    ///
    /// Multi-cell spans (see [`write_span`](Self::write_span)) do **not** survive a blit: copied
    /// tiles keep their glyphs but lose [`TileFlags::SPAN_ANCHOR`]/[`TileFlags::SPAN_COVERED`],
    /// so a span degrades to exactly its text fallback. `src_rect` can clip a span in half, and
    /// half a span is not a thing the grid can represent; degrading to the fallback glyphs is
    /// both representable and the same content a cell backend would have drawn anyway.
    ///
    /// Walks `src`'s and `self`'s layer buffers directly by flat index instead of going through
    /// [`tile`](Self::tile)/[`put_tile`](Self::put_tile) per cell (see retroglyph#263):
    /// each of those recomputes a coordinate conversion and a bounds check per cell, which this
    /// does once per row instead. The destination layer is allocated once, up front, rather than
    /// as a side effect of the first written cell, but only if `src_rect` (clamped to `src`'s
    /// bounds) contains at least one non-empty tile, matching `put_tile`'s original
    /// allocate-on-first-write behavior for a `src_rect` that is entirely transparent.
    pub fn blit(&mut self, layer: u8, src: &Self, src_rect: Rect, dst_x: u16, dst_y: u16) {
        self.blit_with(
            layer,
            src,
            layer,
            src_rect,
            dst_x,
            dst_y,
            |tile, _dst_tile| *tile,
        );
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
    /// [`BlendMode::Linear`]'s per-channel color lerp is delegated to [`gem::Mix`], so this
    /// method is always available. The other modes delegate to [`alpha_blend::BlendMode`]
    /// (imported in this module as `SeparableBlendMode` to avoid colliding with this crate's own
    /// [`BlendMode`]) and require the `blend-modes` feature (default on).
    ///
    /// Like [`blit`](Self::blit) (see retroglyph#262/#263), walks `src`'s and `self`'s layer
    /// buffers directly by flat index instead of per-cell [`tile`](Self::tile)/
    /// [`put_tile`](Self::put_tile), and allocates the destination layer once, up front, rather
    /// than as a side effect of the first written cell.
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
        self.blit_with(
            layer,
            src,
            layer,
            src_rect,
            dst_x,
            dst_y,
            |tile, dst_tile| {
                let mut blended = *tile;
                // `fg_alpha == 1.0` only lets `Linear` skip the call: `Linear` at `t ==
                // 1.0` is `src` by definition, but a `Screen`/`Dodge`/`Burn`/`Overlay`
                // mix at full alpha still needs to run the mode's formula: it isn't
                // equivalent to the raw source color (see `blend_color`'s matching guard).
                if mode != BlendMode::Linear || fg_alpha != 1.0 {
                    blended.style.fg = blend_fg(mode, tile.style.fg, dst_tile.style.fg, fg_alpha);
                }
                if mode != BlendMode::Linear || bg_alpha != 1.0 {
                    blended.style.bg = blend_bg(mode, tile.style.bg, dst_tile.style.bg, bg_alpha);
                }
                blended
            },
        );
    }

    /// Same as [`blit`](Self::blit), except the source tiles are read from `src_layer` on `src`
    /// rather than from `dst_layer` (the layer this writes to on `self`).
    ///
    /// [`blit`](Self::blit) uses one `layer` for both sides, which is exactly right for two
    /// grids sharing the same layer scheme (e.g. [`Surface::on_layer`](crate::Surface::on_layer)
    /// copying within itself), but wrong for [`Surface::blit`](crate::Surface::blit)'s case: a
    /// `src` that is a standalone, layer-0-only `Grid` (composed content like `BoxStyle::render`'s
    /// output), stamped onto a destination surface that may currently be on any layer. Calling
    /// [`blit`](Self::blit) with the destination's layer there looks up that same layer on `src`,
    /// finds nothing (`src` only ever populated layer 0), and silently copies nothing
    /// (retroglyph#824). This method exists so a caller in that position can pin `src_layer` to
    /// `0` independently of `dst_layer`.
    pub(crate) fn blit_cross_layer(
        &mut self,
        dst_layer: u8,
        src: &Self,
        src_layer: u8,
        src_rect: Rect,
        dst_x: u16,
        dst_y: u16,
    ) {
        self.blit_with(
            dst_layer,
            src,
            src_layer,
            src_rect,
            dst_x,
            dst_y,
            |tile, _dst_tile| *tile,
        );
    }

    /// Shared copy loop behind [`blit`](Self::blit), [`blit_alpha`](Self::blit_alpha), and
    /// [`blit_cross_layer`](Self::blit_cross_layer): clamps `src_rect` to `src`'s bounds, skips
    /// the whole call if nothing in it is visible, clears any destination span the copy is about
    /// to partially overwrite (retroglyph#710), and walks matching `src`/destination cells by
    /// flat index (retroglyph#262/#263), applying `transform` to each non-empty source tile
    /// (given the source tile and, for context, the destination tile it's about to replace)
    /// before writing it and fixing up grapheme extras. `dst_x`/`dst_y` saturate on overflow
    /// (retroglyph#268) rather than wrapping; the bounds checks below always catch a saturated
    /// `u16::MAX` origin.
    ///
    /// `dst_layer` and `src_layer` are separate parameters (rather than the one `layer` [`blit`]
    /// and [`blit_alpha`] expose) so [`blit_cross_layer`](Self::blit_cross_layer) can read a
    /// different source layer than the one it writes: see that method's own doc for why (this is
    /// the retroglyph#824 fix).
    #[allow(clippy::too_many_arguments)]
    fn blit_with(
        &mut self,
        dst_layer: u8,
        src: &Self,
        src_layer: u8,
        src_rect: Rect,
        dst_x: u16,
        dst_y: u16,
        transform: impl Fn(&Tile, &Tile) -> Tile,
    ) {
        let Some(src_lb) = src.layer(src_layer) else {
            return;
        };
        let src_width = usize::from(src.width);
        let sx0 = src_rect.left().min(src.width);
        let sx1 = src_rect.right().min(src.width);
        let sy0 = src_rect.top().min(src.height);
        let sy1 = src_rect.bottom().min(src.height);
        if sx0 >= sx1 || sy0 >= sy1 {
            return;
        }

        // Matches the original's implicit allocate-on-first-write: only touch the destination
        // layer at all if there's at least one visible (non-empty) source tile to copy.
        let has_visible = (sy0..sy1).any(|sy| {
            let start = usize::from(sy) * src_width + usize::from(sx0);
            let end = usize::from(sy) * src_width + usize::from(sx1);
            src_lb.buf.as_ref()[start..end]
                .iter()
                .any(|t| !t.flags.contains(TileFlags::EMPTY))
        });
        if !has_visible {
            return;
        }

        let dst_width = usize::from(self.width);
        let dst_height = usize::from(self.height);

        // A blit writes straight into the destination buffer below, bypassing `put_tile`, so it
        // has to do `put_tile`'s `clear_span_overlap` call itself or a cell that used to anchor
        // (or be covered by) a multi-cell span would keep claiming cells this blit just
        // overwrote (retroglyph#710). Only the cells actually being overwritten (in bounds,
        // non-empty source tile) are cleared: an empty source tile is transparent and leaves the
        // destination untouched, so clearing a whole row's footprint up front would wipe out
        // spans the blit never actually touches. Gated on `has_spans` so a grid that never uses
        // spans pays only the one `bool` check.
        if self.has_spans {
            for sy in sy0..sy1 {
                let dy = dst_y.saturating_add(sy - src_rect.top());
                if usize::from(dy) >= dst_height {
                    continue;
                }
                for sx in sx0..sx1 {
                    let dx = dst_x.saturating_add(sx - src_rect.left());
                    if usize::from(dx) >= dst_width {
                        continue;
                    }
                    let src_idx = usize::from(sy) * src_width + usize::from(sx);
                    if src_lb.buf.as_ref()[src_idx]
                        .flags
                        .contains(TileFlags::EMPTY)
                    {
                        continue;
                    }
                    self.clear_span_overlap(dst_layer, dx, dy, 1);
                }
            }
        }

        let dst_lb = self.layer_or_alloc(dst_layer);
        let mut pending_extras: Vec<(usize, TileExtra)> = Vec::new();

        for sy in sy0..sy1 {
            let dy = dst_y.saturating_add(sy - src_rect.top());
            if usize::from(dy) >= dst_height {
                continue;
            }
            for sx in sx0..sx1 {
                let dx = dst_x.saturating_add(sx - src_rect.left());
                if usize::from(dx) >= dst_width {
                    continue;
                }
                let src_idx = usize::from(sy) * src_width + usize::from(sx);
                let tile = &src_lb.buf.as_ref()[src_idx];
                if tile.flags.contains(TileFlags::EMPTY) {
                    continue;
                }
                let dst_idx = usize::from(dy) * dst_width + usize::from(dx);
                let dst_tile = dst_lb.buf.as_ref()[dst_idx];
                let mut out_tile = transform(tile, &dst_tile);
                out_tile.flags.remove(TileFlags::HAS_EXTRA);
                out_tile.clear_span();
                dst_lb.buf.as_mut()[dst_idx] = out_tile;
                if tile.flags.contains(TileFlags::HAS_EXTRA) {
                    if let Some(extra) = src_lb.extra_entry_for(src_idx, tile) {
                        pending_extras.push((dst_idx, extra));
                    }
                } else {
                    dst_lb.extras.remove(&dst_idx);
                }
            }
        }

        for (idx, extra) in pending_extras {
            dst_lb.buf.as_mut()[idx].flags.insert(TileFlags::HAS_EXTRA);
            dst_lb.extras.insert(idx, extra);
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
    pub fn layers(&self) -> impl Iterator<Item = DrawCell<'_>> + '_ {
        let width = usize::from(self.width);
        (0..=self.max_layer)
            .filter_map(move |id| self.layer(id).map(|lb| (id, lb)))
            .flat_map(move |(id, lb)| {
                lb.buf.as_ref().iter().enumerate().map(move |(i, tile)| {
                    #[allow(clippy::cast_possible_truncation)]
                    let x = (i % width) as u16;
                    #[allow(clippy::cast_possible_truncation)]
                    let y = (i / width) as u16;
                    DrawCell {
                        layer: id,
                        pos: Pos::new(x, y),
                        tile,
                        grapheme: lb.extra_for(i, tile),
                        tint: lb.tint_for(i, tile),
                    }
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
    ///   offsets, flags, span, and extra; if its background is not
    ///   [`Color::Default`], replace the background.
    ///
    /// The span fields travel with the flags they are keyed by (see [`Tile::span`]): a
    /// multi-cell span on a higher layer must arrive at a cell backend intact, or its covered
    /// cells lose the anchor they name.
    ///
    /// Because an explicit space is not empty, drawing one on a higher layer
    /// overwrites (erases) the glyph beneath it.
    ///
    /// `dst` must have the same dimensions as `self`.
    ///
    /// Walks layer buffers directly by flat index instead of calling
    /// [`tile`](Self::tile) per cell (see retroglyph#262): that recomputes a coordinate
    /// conversion and a bounds check per cell, which a flat scan over each layer's backing
    /// buffer (the same style [`layers`](Self::layers) and [`diff`](Self::diff) already use)
    /// avoids entirely.
    pub(crate) fn flatten_into(&self, dst: &mut Self) {
        dst.has_spans |= self.has_spans;
        let layer0 = self.layer0();
        let cell_count = layer0.buf.as_ref().len();

        // Seed every destination cell from layer 0: its tile verbatim, and its extra text
        // filtered through `HAS_EXTRA` (the flag is authoritative, see `LayerBuf::extras`'
        // doc comment, so a stale, unflagged entry in `layer0.extras` is not carried over).
        let dst_layer0 = dst.layer0_mut();
        dst_layer0.buf.as_mut().copy_from_slice(layer0.buf.as_ref());
        dst_layer0.extras.clear();
        for (&idx, extra) in &layer0.extras {
            if layer0.buf.as_ref()[idx]
                .flags
                .contains(TileFlags::HAS_EXTRA)
            {
                dst_layer0.extras.insert(idx, extra.clone());
            }
        }

        // Overlay every higher allocated layer, in ascending order, index-for-index.
        for id in 1..=self.max_layer {
            let Some(lb) = self.layer(id) else {
                continue;
            };
            let src_buf = lb.buf.as_ref();
            debug_assert_eq!(src_buf.len(), cell_count);
            let dst_layer0 = dst.layer0_mut();
            for (idx, tile) in src_buf.iter().enumerate() {
                if !tile.flags.contains(TileFlags::EMPTY) {
                    {
                        let out = &mut dst_layer0.buf.as_mut()[idx];
                        out.glyph = tile.glyph;
                        out.width = tile.width;
                        out.style.fg = tile.style.fg;
                        out.dx = tile.dx;
                        out.dy = tile.dy;
                        out.flags = tile.flags;
                        out.span_w = tile.span_w;
                        out.span_h = tile.span_h;
                    }
                    if tile.flags.contains(TileFlags::HAS_EXTRA) {
                        if let Some(extra) = lb.extra_entry_for(idx, tile) {
                            dst_layer0.extras.insert(idx, extra);
                        }
                    } else {
                        dst_layer0.extras.remove(&idx);
                    }
                }
                if tile.style.bg != Color::Default {
                    dst_layer0.buf.as_mut()[idx].style.bg = tile.style.bg;
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
    /// - Layer in both, and `self` and `other` have matching dimensions: only
    ///   positions where the `Tile` or its grapheme text differs are yielded.
    /// - Layer in both, but `self` and `other` have different dimensions: all
    ///   positions in `self` are considered changed, same as a newly
    ///   allocated layer.
    ///
    /// This iterator is zero-allocation: it walks the layer buffers inline.
    pub fn diff<'a>(&'a self, other: &'a Self) -> impl Iterator<Item = DrawCell<'a>> + 'a {
        let width = usize::from(self.width);
        let max = self.max_layer;
        let same_size = self.width == other.width && self.height == other.height;
        (0..=max).flat_map(move |id| {
            // A size mismatch is treated the same as `other` never having allocated this layer:
            // `other`'s buffer can't be indexed with `self`'s flat index once the sizes differ,
            // so every position in `self` is considered changed, matching grixy's `GridDiff`
            // double-buffering contract.
            let other_layer = if same_size { other.layer(id) } else { None };
            match (self.layer(id), other_layer) {
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
                            DrawCell {
                                layer: id,
                                pos: Pos::new(x, y),
                                tile,
                                grapheme: cur_lb.extra_for(i, tile),
                                tint: cur_lb.tint_for(i, tile),
                            }
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
                            // The whole entry, not just its grapheme: a `Tile`-only comparison
                            // cannot see a change to either member of the side table, and a
                            // tint-only change is as real a redraw as a combining-mark change.
                            let cur_extra = cur_lb.entry_for(i, tile);
                            let prev_extra = prev_lb.entry_for(i, prev_tile);
                            if tile == prev_tile && cur_extra == prev_extra {
                                return None;
                            }
                            #[allow(clippy::cast_possible_truncation)]
                            let x = (i % width) as u16;
                            #[allow(clippy::cast_possible_truncation)]
                            let y = (i / width) as u16;
                            Some(DrawCell {
                                layer: id,
                                pos: Pos::new(x, y),
                                tile,
                                grapheme: cur_extra.and_then(|e| e.grapheme.as_deref()),
                                tint: cur_extra.map_or(Tint::None, |e| e.tint),
                            })
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
    F: Iterator<Item = DrawCell<'a>>,
    D: Iterator<Item = DrawCell<'a>>,
{
    type Item = DrawCell<'a>;

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
/// `t`) delegated to [`gem::Mix`], which is `no_std`-safe (round-half-
/// away via `floor(x + 0.5)`, no `std`/`libm` float intrinsics). With the `blend-modes` feature,
/// the other modes evaluate [`SeparableBlendMode::mix`] per channel in `0.0..=1.0`
/// (converting u8 <-> f32 at the boundary; see [`blend_separable_channel`]),
/// then lerp that fully mixed color against the destination by `t`, same as
/// `Linear`.
#[cfg(feature = "blend-modes")]
#[allow(clippy::float_cmp)]
fn blend_color(mode: BlendMode, src: Color, dst: Color, t: f32) -> Color {
    use gem::Mix as _;
    use gem::rgb::{HasBlue as _, HasGreen as _, HasRed as _, Rgb888};
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
            // the other modes must still run their mix formula at `t == 1.0`: see `blit_alpha`.
            let (r, g, b) = mode.separable().map_or_else(
                || {
                    // `dst.mix(src, t)`, not `src.mix(dst, t)`: at `t == 0.0` this must return
                    // `dst` ("keep destination", per `blit_alpha`'s doc comment) and only reach
                    // `src` at `t == 1.0`: the same `0.0 == dst, 1.0 == fully blended` contract
                    // every other `BlendMode` follows (see `blend_separable_channel`).
                    let out = Rgb888::from_rgb(dr, dg, db).mix(Rgb888::from_rgb(sr, sg, sb), t);
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

/// [`blend_color`] without the `blend-modes` feature: [`BlendMode::Linear`] is then the only
/// variant, so this always takes the [`gem::Mix`] per-channel lerp path, without pulling in
/// `alpha-blend` or its float math.
#[cfg(not(feature = "blend-modes"))]
#[allow(clippy::float_cmp)]
fn blend_color(mode: BlendMode, src: Color, dst: Color, t: f32) -> Color {
    use gem::Mix as _;
    use gem::rgb::{HasBlue as _, HasGreen as _, HasRed as _, Rgb888};
    debug_assert_eq!(
        mode,
        BlendMode::Linear,
        "the only BlendMode without `blend-modes`"
    );
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
        ) if t != 1.0 => {
            // `Linear` at `t == 1.0` is `src` by definition (skip to the catch-all arm below).
            let out = Rgb888::from_rgb(dr, dg, db).mix(Rgb888::from_rgb(sr, sg, sb), t);
            Color::Rgb {
                r: out.red(),
                g: out.green(),
                b: out.blue(),
            }
        }
        (src, _) => src,
    }
}

/// Evaluates `sep`'s per-channel mixing function for one RGB channel (`src`/`dst` are u8, `sep`
/// operates in `0.0..=1.0` f32), then lerps that mixed value against `dst` by `t`: `0.0` keeps
/// `dst`, `1.0` uses the fully mixed color. Rounds with `libm::roundf` rather than `f32::round`
/// (a `std`-only method not available in `core`, same reasoning as `libm::fmaf` in
/// `animate::easing`) and clamps before converting back to u8, since `ColorDodge`/`ColorBurn`'s
/// `min(1.0, ...)` branches can round a hair outside `0.0..=1.0` at the float boundary.
#[cfg(feature = "blend-modes")]
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

fn blend_fg(mode: BlendMode, src: Color, dst: Color, t: f32) -> Color {
    blend_color(mode, src, dst, t)
}

fn blend_bg(mode: BlendMode, src: Color, dst: Color, t: f32) -> Color {
    blend_color(mode, src, dst, t)
}

// ---------------------------------------------------------------------------
// Index / IndexMut: layer 0
// ---------------------------------------------------------------------------

impl Index<Pos> for Grid {
    type Output = Tile;

    /// Reads the tile on layer 0 at `pos`.
    ///
    /// # Panics
    ///
    /// Panics if `pos` is outside the grid's `0..width` x `0..height` bounds. This is the
    /// unchecked, layer-0-only counterpart to [`tile`](Self::tile), which instead returns `None`
    /// on either an out-of-bounds `pos` or an unallocated layer; reach for `tile` when `pos`
    /// isn't already known to be in bounds.
    fn index(&self, pos: Pos) -> &Tile {
        &self.layer0().buf[to_grixy_pos(pos)]
    }
}

impl IndexMut<Pos> for Grid {
    /// Mutably borrows the tile on layer 0 at `pos`.
    ///
    /// # Panics
    ///
    /// Panics if `pos` is outside the grid's `0..width` x `0..height` bounds, the same bound as
    /// [`Index`]'s `index`. Reach for [`tile_mut`](Self::tile_mut) when `pos` isn't already known
    /// to be in bounds; it returns `None` instead of panicking.
    fn index_mut(&mut self, pos: Pos) -> &mut Tile {
        let pos = to_grixy_pos(pos);
        &mut self.layer0_mut().buf[pos]
    }
}

// ---------------------------------------------------------------------------
// Display / Debug: layer 0
// ---------------------------------------------------------------------------

impl fmt::Display for Grid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for y in 0..self.height() {
            for x in 0..self.width() {
                let tile = &self[Pos::new(x, y)];
                #[cfg(feature = "egc")]
                let is_spacer = tile.flags.contains(TileFlags::WIDE_CHAR_SPACER);
                #[cfg(not(feature = "egc"))]
                let is_spacer = tile.glyph == '\0';
                let c = if is_spacer {
                    ' ' // right half of a wide char, don't print twice
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

        grid.put_tile(0, (5, 5), tile);
        assert_eq!(grid[Pos::new(5, 5)].glyph(), 'X');
    }

    #[test]
    fn test_grid_checked_put_get() {
        let mut grid = Grid::new(10, 10);
        let tile = Tile::default().with_glyph('Y');

        assert!(grid.put_tile(0, (5, 5), tile).is_some());
        assert_eq!(grid.tile(0, (5, 5)).unwrap().glyph(), 'Y');

        assert!(grid.tile(0, (10, 0)).is_none());
        assert!(grid.put_tile(0, (0, 10), Tile::default()).is_none());
    }

    #[test]
    fn test_grid_put_tile_out_of_bounds_returns_none() {
        let mut grid = Grid::new(10, 10);
        assert!(grid.put_tile(0, (10, 0), Tile::default()).is_none());
    }

    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn test_grid_index_panics_out_of_bounds() {
        let grid = Grid::new(10, 10);
        let _ = &grid[Pos::new(0, 10)];
    }

    #[test]
    fn test_grid_diff() {
        let mut g1 = Grid::new(2, 2);
        let g2 = Grid::new(2, 2);

        g1.put_tile(0, (0, 0), Tile::default().with_glyph('A'));

        let diffs: Vec<_> = g1.diff(&g2).collect();
        assert_eq!(diffs.len(), 1);
        assert_eq!(
            diffs[0],
            DrawCell::on_layer(0, Pos::new(0, 0), &g1[Pos::new(0, 0)])
        );
    }

    #[test]
    fn test_grid_resize_expand() {
        let mut grid = Grid::new(3, 3);
        grid.put_tile(0, (1, 1), Tile::default().with_glyph('X'));
        grid.resize(6, 6);
        assert_eq!(grid.width(), 6);
        assert_eq!(grid.height(), 6);
        assert_eq!(grid[Pos::new(1, 1)].glyph(), 'X'); // preserved
        assert_eq!(grid[Pos::new(5, 5)].glyph(), ' '); // new cells default
    }

    #[test]
    fn test_grid_resize_shrink() {
        let mut grid = Grid::new(10, 10);
        grid.put_tile(0, (1, 1), Tile::default().with_glyph('A'));
        grid.resize(5, 5);
        assert_eq!(grid.width(), 5);
        assert_eq!(grid.height(), 5);
        assert_eq!(grid[Pos::new(1, 1)].glyph(), 'A'); // still in bounds, preserved
    }

    #[test]
    fn test_grid_resize_preserves_overlap() {
        let mut grid = Grid::new(4, 4);
        grid.put_tile(0, (0, 0), Tile::default().with_glyph('@'));
        grid.put_tile(0, (3, 3), Tile::default().with_glyph('X'));
        grid.resize(3, 3); // shrink: (3,3) falls outside
        assert_eq!(grid[Pos::new(0, 0)].glyph(), '@');
        assert_eq!(grid[Pos::new(2, 2)].glyph(), ' '); // was default, still default
    }

    #[test]
    fn test_grid_display() {
        let mut grid = Grid::new(3, 2);
        grid.put_tile(0, (0, 0), Tile::default().with_glyph('A'));

        let s = alloc::format!("{grid}");
        assert_eq!(s, "A··\n···\n");
    }

    #[test]
    fn test_grid_display_wide_char_spacer() {
        // A wide char's right-half spacer cell prints as a plain space, not the wide
        // char's own glyph repeated.
        let mut grid = Grid::new(3, 1);
        grid.write_grapheme(0, 0, 0, "\u{4e2d}", Style::default()); // wide (CJK)

        let s = alloc::format!("{grid}");
        assert_eq!(s, "\u{4e2d} \u{b7}\n");
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
        for (x, y, tile) in grid.cells_mut(0).unwrap() {
            #[allow(clippy::cast_possible_truncation)]
            let idx = (y * 2 + x) as u8;
            *tile = Tile::new(char::from(b'A' + idx), Style::default());
        }
        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'A');
        assert_eq!(grid[Pos::new(1, 0)].glyph(), 'B');
        assert_eq!(grid[Pos::new(0, 1)].glyph(), 'C');
        assert_eq!(grid[Pos::new(1, 1)].glyph(), 'D');
    }

    #[test]
    fn test_grid_cells_mut_unallocated_layer_is_none() {
        // Mirrors `cells`: an unwritten layer is `None`, not an empty iterator, and critically
        // it must not allocate the layer as a side effect of checking.
        let mut grid = Grid::new(2, 2);
        assert!(grid.cells_mut(3).is_none());
        assert!(grid.tile(3, (0, 0)).is_none());
    }

    #[test]
    fn test_grid_cells_mut_or_alloc_allocates_an_unwritten_layer() {
        use crate::style::Style;
        let mut grid = Grid::new(2, 2);
        assert!(grid.cells_mut(2).is_none());
        for (_, _, tile) in grid.cells_mut_or_alloc(2) {
            *tile = Tile::new('x', Style::default());
        }
        // Now allocated: the non-allocating accessor finds it too.
        assert!(grid.cells_mut(2).is_some());
        assert_eq!(grid.tile(2, (0, 0)).unwrap().glyph(), 'x');
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
        assert_eq!(s, Size::new(80, 25));
        let t: (u16, u16) = s.into();
        assert_eq!(t, (80, 25));
    }

    #[test]
    fn test_offset_from_tuple() {
        let o: Offset = (-3i16, 7i16).into();
        assert_eq!(o, Offset::new(-3, 7));
        let t: (i16, i16) = o.into();
        assert_eq!(t, (-3, 7));
    }

    #[test]
    fn test_offset_default_is_zero() {
        assert_eq!(Offset::default(), Offset::new(0, 0));
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
        assert!(Size::new(1, 2) < Size::new(2, 1));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_size_serializes_and_deserializes() {
        let size = Size::new(80, 25);
        let json = serde_json::to_string(&size).expect("serialize");
        assert_eq!(
            serde_json::from_str::<Size>(&json).expect("deserialize"),
            size
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_pos_and_rect_serialize_via_ixy() {
        let pos = Pos::new(2, 1);
        let json = serde_json::to_string(&pos).expect("serialize");
        assert_eq!(
            serde_json::from_str::<Pos>(&json).expect("deserialize"),
            pos
        );

        let rect = Rect::new(0, 0, 10, 4);
        let json = serde_json::to_string(&rect).expect("serialize");
        assert_eq!(
            serde_json::from_str::<Rect>(&json).expect("deserialize"),
            rect
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
        g.put_tile(3, (0, 0), Tile::new('@', Style::default()));
        assert!(g.layer(3).is_some());
        assert!(g.layer(4).is_none());
    }

    #[test]
    fn test_grid_new_layer_table_starts_at_a_single_slot() {
        // retroglyph#264: the layer-table `Vec` itself should start small (a single slot for
        // layer 0), not pre-allocate all 256 possible slots up front.
        let g = Grid::new(5, 5);
        assert_eq!(g.layers.len(), 1);
        assert_eq!(g.max_layer(), 0);
    }

    #[test]
    fn test_grid_layer_or_alloc_grows_table_lazily_to_the_written_id() {
        let mut g = Grid::new(5, 5);
        g.put_tile(10, (0, 0), Tile::new('@', Style::default()));
        // The table grows to exactly `id + 1` slots, not all 256.
        assert_eq!(g.layers.len(), 11);
        assert_eq!(g.max_layer(), 10);
        assert!(g.layer(10).is_some());
        for id in 1u8..10 {
            assert!(g.layer(id).is_none(), "layer {id} should be None");
        }
    }

    #[test]
    fn test_grid_layer_beyond_table_length_reads_as_none() {
        // A layer id past the current table length (never written) must read identically to an
        // in-bounds `None` slot, not panic or error.
        let g = Grid::new(5, 5);
        assert_eq!(g.layers.len(), 1);
        assert!(g.layer(255).is_none());
        assert!(g.tile(255, (0, 0)).is_none());
        assert!(g.grapheme(255, 0, 0).is_none());
    }

    #[test]
    fn test_grid_clear_beyond_table_length_is_a_no_op() {
        // Clearing an id past the current table length must not panic: it's equivalent to
        // clearing an unallocated in-bounds layer (does nothing).
        let mut g = Grid::new(5, 5);
        g.clear(255);
        assert_eq!(g.layers.len(), 1);
    }

    #[test]
    fn test_grid_layer_table_growth_is_monotonic_across_writes() {
        // Writing to a lower layer id after a higher one must not shrink the table, and must
        // preserve the higher layer's content.
        let mut g = Grid::new(5, 5);
        g.put_tile(20, (1, 1), Tile::new('H', Style::default()));
        assert_eq!(g.layers.len(), 21);
        g.put_tile(2, (0, 0), Tile::new('L', Style::default()));
        assert_eq!(
            g.layers.len(),
            21,
            "writing a lower id must not shrink the table"
        );
        assert_eq!(g.max_layer(), 20);
        assert_eq!(g.tile(20, (1, 1)).unwrap().glyph, 'H');
        assert_eq!(g.tile(2, (0, 0)).unwrap().glyph, 'L');
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
        cur.put_tile(0, (2, 3), Tile::new('X', Style::default()));
        let diffs: Vec<_> = cur.diff(&prev).collect();
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].layer, 0);
        assert_eq!(diffs[0].pos, Pos::new(2, 3));
        assert_eq!(diffs[0].tile.glyph, 'X');
    }

    #[test]
    fn test_grid_diff_new_layer_yields_all_cells() {
        let mut cur = Grid::new(3, 4);
        let prev = Grid::new(3, 4);
        cur.put_tile(1, (0, 0), Tile::new('A', Style::default()));
        let diffs: Vec<_> = cur.diff(&prev).collect();
        // All 12 cells of the newly allocated layer 1 are yielded.
        assert_eq!(diffs.len(), 12);
        assert!(diffs.iter().all(|c| c.layer == 1));
    }

    #[test]
    fn test_grid_diff_mismatched_sizes_yields_full_diff() {
        // A smaller `other` must not panic; every cell in `self` is reported as changed instead.
        let mut cur = Grid::new(3, 2);
        let prev = Grid::new(2, 2);
        cur.put_tile(0, (0, 0), Tile::new('X', Style::default()));
        let diffs: Vec<_> = cur.diff(&prev).collect();
        assert_eq!(diffs.len(), 6);
        assert!(diffs.iter().all(|c| c.layer == 0));
    }

    #[test]
    fn test_grid_diff_layer_major_order() {
        let mut cur = Grid::new(3, 3);
        let prev = Grid::new(3, 3);
        cur.put_tile(2, (0, 0), Tile::new('B', Style::default()));
        cur.put_tile(0, (1, 0), Tile::new('A', Style::default()));
        let layers: Vec<u8> = cur.diff(&prev).map(|c| c.layer).collect();
        // Layer 0's change appears first, then all of layer 2.
        assert_eq!(layers[0], 0);
        assert!(layers[1..].iter().all(|&l| l == 2));
    }

    #[test]
    fn test_grid_put_and_get_on_layer_2() {
        use crate::style::Style;
        let mut g = Grid::new(5, 5);
        g.put_tile(2, (1, 1), Tile::new('Z', Style::default()));
        assert_eq!(g.tile(2, (1, 1)).unwrap().glyph, 'Z');
        // Layer 0 at same position should still be default.
        assert_eq!(g[Pos::new(1, 1)].glyph, ' ');
        // Unallocated layer returns None.
        assert!(g.tile(3, (0, 0)).is_none());
    }

    #[test]
    fn test_grid_tile_mut_writes_in_place_without_clearing_spans() {
        let mut g = Grid::new(4, 4);
        g.write_span(0, 0, 0, &["C=", "[]"], Style::default())
            .unwrap();

        // Unlike `put_tile`, `tile_mut` hands out a direct `&mut Tile` and does not intercept
        // the write, so the span's other cells are left dangling on purpose here.
        g.tile_mut(0, (0, 0)).unwrap().glyph = 'x';
        assert_eq!(g[Pos::new(0, 0)].glyph(), 'x');

        // Unallocated layer and out-of-bounds position both report `None`, not a panic.
        assert!(g.tile_mut(1, (0, 0)).is_none());
        assert!(g.tile_mut(0, (10, 10)).is_none());
    }

    #[test]
    fn test_grid_clear_layer() {
        let mut g = Grid::new(5, 5);
        g.put_tile(1, (0, 0), Tile::new('Z', Style::default()));
        g.put_tile(0, (0, 0), Tile::new('A', Style::default()));
        g.clear(1);
        assert_eq!(g.tile(0, (0, 0)).unwrap().glyph, 'A');
        assert!(g.tile(1, (0, 0)).is_some());
        assert_eq!(g.tile(1, (0, 0)).unwrap().glyph, ' '); // cleared
    }

    #[test]
    fn test_grid_clear_all() {
        let mut g = Grid::new(5, 5);
        g.put_tile(1, (0, 0), Tile::new('Z', Style::default()));
        g.put_tile(0, (0, 0), Tile::new('A', Style::default()));
        g.clear_all();
        // Both layers reset to default (space).
        assert_eq!(g[Pos::new(0, 0)].glyph, ' ');
        assert_eq!(g.tile(1, (0, 0)).unwrap().glyph, ' ');
    }

    #[test]
    fn test_grid_clone_is_independent() {
        let mut g = Grid::new(3, 3);
        g.put_tile(0, (0, 0), Tile::new('A', Style::default()));
        g.put_tile(2, (1, 1), Tile::new('B', Style::default()));

        let mut cloned = g.clone();
        assert_eq!(cloned[Pos::new(0, 0)].glyph, 'A');
        assert_eq!(cloned.tile(2, (1, 1)).unwrap().glyph, 'B');
        assert_eq!(cloned.max_layer(), g.max_layer());

        // Mutating the clone must not affect the original (deep copy).
        cloned.put_tile(0, (0, 0), Tile::new('Z', Style::default()));
        assert_eq!(cloned[Pos::new(0, 0)].glyph, 'Z');
        assert_eq!(g[Pos::new(0, 0)].glyph, 'A');
    }

    // --- Extra grapheme text (EGC side-table) ---

    #[cfg(feature = "egc")]
    #[test]
    fn test_grid_write_grapheme_stores_and_reads_extra() {
        let mut g = Grid::new(5, 5);
        g.write_grapheme(0, 1, 1, "e\u{0301}", Style::default());
        assert_eq!(g[Pos::new(1, 1)].glyph, 'e');
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
        g.put_tile(0, (0, 0), Tile::new('X', Style::default()));
        assert_eq!(g.grapheme(0, 0, 0), None);
        assert!(!g[Pos::new(0, 0)].flags().contains(TileFlags::HAS_EXTRA));
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
        assert_eq!(g[Pos::new(3, 1)].glyph, 'e');
        assert_eq!(g.grapheme(0, 3, 1), Some("e\u{0301}"));
        // No ghost entry landed on some other cell at the old flat index.
        assert_eq!(g.grapheme(0, 7, 0), None);

        // Shrinking past the cell drops its extras entry along with the tile.
        g.resize(2, 4);
        assert_eq!(g.grapheme(0, 3, 1), None);
    }

    // ── Tint storage ──────────────────────────────────────────────────────
    //
    // A tint lives in the same sparse side table as a grapheme, so it inherits every path that
    // table already has to get right: rekeying on resize, copying on blit, and being dropped
    // when the cell it belongs to is overwritten or cleared. These cover each of those, plus the
    // interaction between the two members now sharing one entry and one flag.

    #[test]
    fn tint_round_trips_and_defaults_to_none() {
        let mut g = Grid::new(4, 4);
        assert_eq!(g.tint(0, 1, 1), Tint::None);

        g.write_grapheme(0, 1, 1, "@", Style::default());
        g.set_tint(0, 1, 1, Tint::multiply(128, 64, 32));
        assert_eq!(g.tint(0, 1, 1), Tint::multiply(128, 64, 32));

        // Setting None clears it again.
        g.set_tint(0, 1, 1, Tint::None);
        assert_eq!(g.tint(0, 1, 1), Tint::None);
    }

    #[test]
    fn tint_is_per_layer_and_per_cell() {
        let mut g = Grid::new(4, 4);
        g.set_tint(0, 1, 1, Tint::multiply(10, 20, 30));
        g.set_tint(3, 1, 1, Tint::mix(1, 2, 3, 4));

        assert_eq!(g.tint(0, 1, 1), Tint::multiply(10, 20, 30));
        assert_eq!(g.tint(3, 1, 1), Tint::mix(1, 2, 3, 4));
        assert_eq!(g.tint(0, 1, 2), Tint::None);
        assert_eq!(g.tint(1, 1, 1), Tint::None);
    }

    #[test]
    fn tint_out_of_bounds_reads_none_and_writes_nothing() {
        let mut g = Grid::new(2, 2);
        g.set_tint(0, 9, 9, Tint::multiply(1, 2, 3));
        assert_eq!(g.tint(0, 9, 9), Tint::None);
        assert_eq!(g.tint(0, 0, 0), Tint::None);
    }

    #[test]
    fn writing_a_glyph_over_a_tinted_cell_drops_the_tint() {
        let mut g = Grid::new(4, 4);
        g.write_grapheme(0, 1, 1, "@", Style::default());
        g.set_tint(0, 1, 1, Tint::multiply(128, 128, 128));

        // A tint describes the artwork that was drawn, not the position, so replacing the
        // artwork drops it rather than silently recolouring whatever lands there next.
        g.write_grapheme(0, 1, 1, "#", Style::default());
        assert_eq!(g.tint(0, 1, 1), Tint::None);
    }

    #[test]
    fn put_tile_drops_the_tint() {
        let mut g = Grid::new(4, 4);
        g.set_tint(0, 1, 1, Tint::multiply(128, 128, 128));
        g.put_tile(0, Pos::new(1, 1), Tile::new('x', Style::default()));
        assert_eq!(g.tint(0, 1, 1), Tint::None);
    }

    #[test]
    fn clear_drops_every_tint_on_the_layer() {
        let mut g = Grid::new(4, 4);
        g.set_tint(0, 1, 1, Tint::multiply(1, 2, 3));
        g.set_tint(1, 1, 1, Tint::multiply(4, 5, 6));

        g.clear(0);
        assert_eq!(g.tint(0, 1, 1), Tint::None);
        assert_eq!(g.tint(1, 1, 1), Tint::multiply(4, 5, 6));
    }

    #[test]
    fn resize_remaps_a_tint_to_the_new_stride() {
        let mut g = Grid::new(4, 4);
        g.write_grapheme(0, 3, 1, "@", Style::default());
        g.set_tint(0, 3, 1, Tint::mix(200, 100, 50, 128));

        // Widening changes the row stride, so (3, 1)'s flat index moves.
        g.resize(8, 4);
        assert_eq!(g.tint(0, 3, 1), Tint::mix(200, 100, 50, 128));
        // No ghost entry landed on whatever cell now holds the old flat index.
        assert_eq!(g.tint(0, 7, 0), Tint::None);

        // Shrinking past the cell drops its entry with the tile.
        g.resize(2, 4);
        assert_eq!(g.tint(0, 3, 1), Tint::None);
    }

    #[test]
    fn blit_carries_a_tint_across_grids() {
        let mut src = Grid::new(4, 4);
        src.write_grapheme(0, 1, 1, "@", Style::default());
        src.set_tint(0, 1, 1, Tint::multiply(64, 128, 192));

        let mut dst = Grid::new(4, 4);
        // Pre-existing tint on the destination cell, to prove the copy replaces rather than
        // merges with whatever was there.
        dst.set_tint(0, 1, 1, Tint::mix(9, 9, 9, 9));
        dst.blit(0, &src, Rect::new(0, 0, 4, 4), 0, 0);

        assert_eq!(dst.tint(0, 1, 1), Tint::multiply(64, 128, 192));
        assert_eq!(dst.tint(0, 0, 0), Tint::None);
    }

    #[test]
    fn blit_clears_a_destination_tint_where_the_source_has_none() {
        let mut src = Grid::new(2, 2);
        src.write_grapheme(0, 0, 0, "@", Style::default());

        let mut dst = Grid::new(2, 2);
        dst.set_tint(0, 0, 0, Tint::multiply(1, 2, 3));
        dst.blit(0, &src, Rect::new(0, 0, 2, 2), 0, 0);

        assert_eq!(dst.tint(0, 0, 0), Tint::None);
    }

    #[cfg(feature = "egc")]
    #[test]
    fn a_tint_and_a_grapheme_share_one_entry_without_clobbering_each_other() {
        let mut g = Grid::new(4, 4);
        g.write_grapheme(0, 1, 1, "e\u{0301}", Style::default());
        g.set_tint(0, 1, 1, Tint::multiply(128, 128, 128));

        // Both members survive: `set_tint` preserves the grapheme already stored.
        assert_eq!(g.grapheme(0, 1, 1), Some("e\u{0301}"));
        assert_eq!(g.tint(0, 1, 1), Tint::multiply(128, 128, 128));

        // Clearing the tint leaves the grapheme, and so leaves the entry in place.
        g.set_tint(0, 1, 1, Tint::None);
        assert_eq!(g.grapheme(0, 1, 1), Some("e\u{0301}"));
        assert_eq!(g.tint(0, 1, 1), Tint::None);
    }

    #[cfg(feature = "egc")]
    #[test]
    fn a_tint_alone_keeps_grapheme_reads_answering_none() {
        let mut g = Grid::new(4, 4);
        g.write_grapheme(0, 1, 1, "@", Style::default());
        g.set_tint(0, 1, 1, Tint::multiply(128, 128, 128));

        // HAS_EXTRA is now set for a cell with no grapheme text. `grapheme` must still say None
        // rather than reaching into the entry and finding an empty slot.
        assert_eq!(g.grapheme(0, 1, 1), None);
        assert_eq!(g.tint(0, 1, 1), Tint::multiply(128, 128, 128));
    }

    #[cfg(feature = "egc")]
    #[test]
    fn test_grid_diff_detects_grapheme_only_change() {
        // Same glyph, style, and flags on both sides: only the combining
        // mark differs. A `Tile`-only diff would miss this.
        let mut cur = Grid::new(2, 2);
        let mut prev = Grid::new(2, 2);
        cur.write_grapheme(0, 0, 0, "e\u{0301}", Style::default());
        prev.write_grapheme(0, 0, 0, "e\u{0300}", Style::default());

        let diffs: Vec<_> = cur.diff(&prev).collect();
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].pos, Pos::new(0, 0));
        assert_eq!(diffs[0].grapheme, Some("e\u{0301}"));

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
        assert_eq!(dst[Pos::new(0, 0)].glyph, 'e');
        assert_eq!(dst.grapheme(0, 0, 0), Some("e\u{0301}"));
    }

    #[test]
    fn test_grid_blit_empty_rect_is_a_no_op() {
        // A zero-area `src_rect` has no cells at all: `sx0 >= sx1` should short-circuit before
        // touching the destination.
        let src = Grid::new(2, 2);
        let mut dst = Grid::new(2, 2);
        dst.put_tile(0, (0, 0), Tile::new('x', Style::default()));
        dst.blit(0, &src, Rect::new(0, 0, 0, 0), 0, 0);
        assert_eq!(dst[Pos::new(0, 0)].glyph(), 'x');
        assert_eq!(dst.max_layer(), 0);
    }

    #[test]
    fn test_grid_blit_fully_transparent_source_does_not_allocate_dst_layer() {
        // Perf refactor (#263): the destination layer is allocated up front, but only after
        // confirming the (clamped) source region has at least one non-empty tile, matching
        // `put_tile`'s original allocate-on-first-write behavior for an all-transparent blit.
        let src = Grid::new(2, 2);
        let mut dst = Grid::new(2, 2);
        dst.blit(3, &src, Rect::new(0, 0, 2, 2), 0, 0);
        assert_eq!(dst.max_layer(), 0);
    }

    #[test]
    fn test_grid_blit_skips_out_of_bounds_source_and_dest_regions() {
        let mut src = Grid::new(4, 4);
        for y in 0..4 {
            for x in 0..4 {
                src.put_tile(0, (x, y), Tile::new('#', Style::default()));
            }
        }

        let mut dst = Grid::new(2, 2);
        // `src_rect` extends past `src`'s bounds and the destination offset pushes part of the
        // copied region past `dst`'s bounds too; both should be silently clamped, not panic.
        dst.blit(0, &src, Rect::new(2, 2, 10, 10), 1, 1);
        assert_eq!(dst[Pos::new(1, 1)].glyph(), '#');
        assert_eq!(dst[Pos::new(0, 0)].glyph(), ' ');
        assert_eq!(dst[Pos::new(0, 1)].glyph(), ' ');
        assert_eq!(dst[Pos::new(1, 0)].glyph(), ' ');
    }

    #[test]
    fn test_grid_blit_sub_cell_offset_and_transparency() {
        let mut src = Grid::new(2, 2);
        src.put_tile(0, (0, 0), Tile::new('A', Style::default()));
        // (1, 0) and (1, 1) stay at their default (empty) tile: transparent, should not
        // overwrite the destination.
        src.put_tile(0, (0, 1), Tile::new('B', Style::default()));

        let mut dst = Grid::new(3, 3);
        dst.put_tile(0, (2, 2), Tile::new('Z', Style::default()));
        dst.blit(0, &src, Rect::new(0, 0, 2, 2), 1, 1);

        assert_eq!(dst[Pos::new(1, 1)].glyph(), 'A');
        assert_eq!(dst[Pos::new(1, 2)].glyph(), 'B');
        // Untouched by the (transparent) source cells at (1, 0) and (1, 1).
        assert_eq!(dst[Pos::new(2, 1)].glyph(), ' ');
        assert_eq!(dst[Pos::new(2, 2)].glyph(), 'Z');
    }

    #[test]
    fn test_grid_blit_multi_layer_independent() {
        let mut src = Grid::new(2, 2);
        src.put_tile(0, (0, 0), Tile::new('a', Style::default()));
        src.put_tile(2, (0, 0), Tile::new('b', Style::default()));

        let mut dst = Grid::new(2, 2);
        dst.blit(0, &src, Rect::new(0, 0, 2, 2), 0, 0);
        dst.blit(2, &src, Rect::new(0, 0, 2, 2), 0, 0);

        assert_eq!(dst.tile(0, (0, 0)).map(Tile::glyph), Some('a'));
        assert_eq!(dst.tile(2, (0, 0)).map(Tile::glyph), Some('b'));
        // Layer 1 was never written by either blit call.
        assert!(dst.tile(1, (0, 0)).is_none());
    }

    #[test]
    fn test_grid_blit_dest_origin_near_u16_max_does_not_wrap() {
        // retroglyph#268: with a plain (non-saturating) `dst_x + (sx - src_rect.left())`, an
        // origin this close to `u16::MAX` overflows and wraps back into a small, in-bounds
        // value: silently corrupting an unrelated cell instead of being clamped out. Picked so
        // that `dst_x + 3` overflows `u16` and wraps to `1`, which *is* in-bounds for this small
        // `dst` grid: `65534u16.wrapping_add(3) == 1`.
        let mut src = Grid::new(4, 1);
        src.put_tile(0, (3, 0), Tile::new('Q', Style::default()));

        let mut dst = Grid::new(4, 1);
        dst.blit(0, &src, Rect::new(0, 0, 4, 1), u16::MAX - 1, 0);

        // The would-be-wrapped cell (index 1) must not have been touched.
        assert_eq!(dst[Pos::new(1, 0)].glyph(), ' ');
        // No other cell was touched either: the whole row's writes overflowed and were
        // skipped (dst_x saturates to u16::MAX for every column in this row).
        for x in 0..4 {
            assert_eq!(
                dst[Pos::new(x, 0)].glyph(),
                ' ',
                "cell ({x}, 0) unexpectedly written"
            );
        }
    }

    #[test]
    fn test_grid_blit_normal_offset_unaffected_by_overflow_fix() {
        // A typical, non-overflowing blit must still work exactly as before.
        let mut src = Grid::new(2, 2);
        src.put_tile(0, (0, 0), Tile::new('A', Style::default()));
        src.put_tile(0, (1, 1), Tile::new('B', Style::default()));

        let mut dst = Grid::new(4, 4);
        dst.blit(0, &src, Rect::new(0, 0, 2, 2), 1, 1);

        assert_eq!(dst[Pos::new(1, 1)].glyph(), 'A');
        assert_eq!(dst[Pos::new(2, 2)].glyph(), 'B');
    }

    // --- `BlendMode` / `blit_alpha` ---

    #[cfg(feature = "blend-modes")]
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

    #[cfg(feature = "blend-modes")]
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

    #[cfg(feature = "blend-modes")]
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

    #[cfg(feature = "blend-modes")]
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
    #[cfg(feature = "blend-modes")]
    #[test]
    fn test_grid_blit_alpha_screen_blends_fg() {
        let mut src = Grid::new(1, 1);
        src.put_tile(
            0,
            (0, 0),
            Tile::default()
                .with_glyph('X')
                .with_style(Style::new().fg(Color::Rgb {
                    r: 204,
                    g: 204,
                    b: 204,
                })),
        );

        let mut dst = Grid::new(1, 1);
        dst.put_tile(
            0,
            (0, 0),
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
            dst[Pos::new(0, 0)].style.fg,
            Color::Rgb {
                r: 224,
                g: 224,
                b: 224
            }
        );
    }

    /// retroglyph#268: same wraparound guard as `blit`'s
    /// `test_grid_blit_dest_origin_near_u16_max_does_not_wrap`, but through `blit_alpha`'s
    /// separate `dst_x`/`dst_y` computation. `blit_alpha` is always available (only `Linear` is
    /// used here), so this test isn't gated on `blend-modes`.
    #[test]
    fn test_grid_blit_alpha_dest_origin_near_u16_max_does_not_wrap() {
        let mut src = Grid::new(4, 1);
        src.put_tile(0, (3, 0), Tile::new('Q', Style::default()));

        let mut dst = Grid::new(4, 1);
        dst.blit_alpha(
            0,
            &src,
            Rect::new(0, 0, 4, 1),
            u16::MAX - 1,
            0,
            BlendMode::Linear,
            1.0,
            1.0,
        );

        for x in 0..4 {
            assert_eq!(
                dst[Pos::new(x, 0)].glyph(),
                ' ',
                "cell ({x}, 0) unexpectedly written"
            );
        }
    }

    /// `BlendMode::Linear` at `t == 0.0` keeps the destination and at `t == 1.0` uses the source,
    /// matching `blit_alpha`'s doc comment (this direction was actually inverted before this
    /// change: the underlying `gem::Mix` call had `src`/`dst` swapped, so `t == 0.0` used
    /// to return `src` and `t == 1.0` returned `dst`. No prior tests covered `blit_alpha`, so
    /// this had shipped unnoticed). `Linear` is always available, so this test isn't gated on
    /// `blend-modes`.
    #[test]
    fn test_grid_blit_alpha_linear_direction() {
        let mut src = Grid::new(1, 1);
        src.put_tile(
            0,
            (0, 0),
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
            dst.put_tile(
                0,
                (0, 0),
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
            dst[Pos::new(0, 0)].style.fg
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
    #[cfg(feature = "blend-modes")]
    #[test]
    fn test_blend_color_non_rgb_passthrough_all_modes() {
        for mode in [
            BlendMode::Linear,
            BlendMode::Screen,
            BlendMode::Dodge,
            BlendMode::Burn,
            BlendMode::Overlay,
            BlendMode::Multiply,
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

    /// Same as `test_blend_color_non_rgb_passthrough_all_modes`, but only `Linear` (the only
    /// variant available without `blend-modes`), so this coverage survives with the feature off.
    #[cfg(not(feature = "blend-modes"))]
    #[test]
    fn test_blend_color_non_rgb_passthrough_linear() {
        assert_eq!(
            blend_color(
                BlendMode::Linear,
                Color::Default,
                Color::Rgb { r: 1, g: 2, b: 3 },
                0.5
            ),
            Color::Default
        );
        assert_eq!(
            blend_color(BlendMode::Linear, Color::BLACK, Color::WHITE, 0.5),
            Color::BLACK
        );
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
        assert_eq!(flattened[Pos::new(0, 0)].glyph, 'e');
        assert_eq!(flattened.grapheme(0, 0, 0), Some("e\u{0301}"));
    }

    #[test]
    fn test_grid_flatten_into_single_layer_is_a_plain_copy() {
        let mut g = Grid::new(2, 2);
        g.put_tile(0, (0, 0), Tile::new('a', Style::default()));
        g.put_tile(0, (1, 1), Tile::new('b', Style::default()));
        let mut flattened = Grid::new(2, 2);
        g.flatten_into(&mut flattened);
        assert_eq!(flattened[Pos::new(0, 0)].glyph(), 'a');
        assert_eq!(flattened[Pos::new(1, 1)].glyph(), 'b');
        assert_eq!(flattened[Pos::new(1, 0)].glyph(), ' ');
    }

    #[test]
    fn test_grid_flatten_into_higher_layer_overwrites_glyph_and_fg_but_not_default_bg() {
        let mut g = Grid::new(1, 1);
        g.put_tile(
            0,
            (0, 0),
            Tile::new('a', Style::new().fg(Color::BLACK).bg(Color::WHITE)),
        );
        g.put_tile(1, (0, 0), Tile::new('b', Style::new().fg(Color::WHITE)));

        let mut flattened = Grid::new(1, 1);
        g.flatten_into(&mut flattened);
        let out = flattened[Pos::new(0, 0)];
        assert_eq!(out.glyph(), 'b');
        assert_eq!(out.style().fg, Color::WHITE);
        // Layer 1's tile has a `Default` background, so layer 0's background shows through.
        assert_eq!(out.style().bg, Color::WHITE);
    }

    #[test]
    fn test_grid_flatten_into_empty_higher_layer_cell_is_transparent() {
        let mut g = Grid::new(2, 1);
        g.put_tile(0, (0, 0), Tile::new('a', Style::default()));
        g.put_tile(0, (1, 0), Tile::new('b', Style::default()));
        // Only touch (0, 0) on layer 1; (1, 0) on layer 1 stays at its default (EMPTY) tile.
        g.put_tile(1, (0, 0), Tile::new('c', Style::default()));

        let mut flattened = Grid::new(2, 1);
        g.flatten_into(&mut flattened);
        assert_eq!(flattened[Pos::new(0, 0)].glyph(), 'c');
        // Untouched by the transparent layer-1 cell: layer 0's glyph shows through.
        assert_eq!(flattened[Pos::new(1, 0)].glyph(), 'b');
    }

    #[test]
    fn test_grid_flatten_into_multi_layer_stale_dst_extra_is_cleared() {
        // `dst` may be a reused scratch buffer with stale content from a previous frame (see
        // `Terminal::present`): `flatten_into` must fully overwrite it, not merge with it.
        let mut flattened = Grid::new(1, 1);
        flattened.put_tile(0, (0, 0), Tile::new('z', Style::default()));

        let g = Grid::new(1, 1);
        g.flatten_into(&mut flattened);
        assert_eq!(flattened[Pos::new(0, 0)].glyph(), ' ');
    }

    // ── Multi-cell spans (retroglyph#412) ────────────────────────────────

    /// The anchor owns the footprint; every other cell names the anchor and keeps its own glyph.
    #[test]
    fn write_span_marks_anchor_and_covered_cells() {
        let mut grid = Grid::new(4, 4);
        grid.write_span(0, 1, 1, &["C=", "[]"], Style::default())
            .expect("2x2 span fits in a 4x4 grid");

        let anchor = grid.tile(0, (1, 1)).unwrap();
        assert!(anchor.flags().contains(TileFlags::SPAN_ANCHOR));
        assert_eq!(anchor.span(), (2, 2));
        assert_eq!(anchor.span_offset(), None);
        assert_eq!(anchor.glyph(), 'C');

        for (x, y, glyph, offset) in [
            (2, 1, '=', (1, 0)),
            (1, 2, '[', (0, 1)),
            (2, 2, ']', (1, 1)),
        ] {
            let tile = grid.tile(0, (x, y)).unwrap();
            assert!(
                tile.flags().contains(TileFlags::SPAN_COVERED),
                "({x}, {y}) should be covered"
            );
            assert_eq!(tile.glyph(), glyph, "({x}, {y}) keeps its fallback glyph");
            assert_eq!(tile.span_offset(), Some(offset));
            // A covered cell is inside a footprint, it does not own one.
            assert_eq!(tile.span(), (1, 1));
        }
    }

    /// The whole point of `SPAN_COVERED` differing from `WIDE_CHAR_SPACER`: cell backends read
    /// these glyphs, so they must survive the write intact.
    #[test]
    fn write_span_keeps_the_fallback_glyphs_readable() {
        let mut grid = Grid::new(4, 4);
        grid.write_span(0, 0, 0, &["C=", "[]"], Style::default())
            .unwrap();
        let read = |x, y| grid.tile(0, (x, y)).unwrap().glyph();
        assert_eq!(
            [read(0, 0), read(1, 0), read(0, 1), read(1, 1)],
            ['C', '=', '[', ']']
        );
    }

    #[test]
    fn span_owner_reports_the_anchor_from_every_cell_of_the_span() {
        let mut grid = Grid::new(6, 6);
        grid.write_span(0, 2, 3, &["AB", "CD", "EF"], Style::default())
            .unwrap();

        // Every cell of the footprint, the anchor included, answers with the same position, so
        // hit-testing is one comparison.
        for (x, y) in [(2, 3), (3, 3), (2, 4), (3, 4), (2, 5), (3, 5)] {
            assert_eq!(grid.span_owner(0, x, y), Some(Pos::new(2, 3)), "({x}, {y})");
        }
        // A free cell, an out-of-bounds one, and one on an unallocated layer belong to no span.
        assert_eq!(grid.span_owner(0, 0, 0), None);
        assert_eq!(grid.span_owner(0, 99, 99), None);
        assert_eq!(grid.span_owner(3, 3, 3), None);
    }

    #[test]
    fn write_span_rejects_malformed_input_without_writing() {
        let mut grid = Grid::new(4, 4);
        assert_eq!(
            grid.write_span(0, 0, 0, &[] as &[&str], Style::default()),
            None
        );
        assert_eq!(grid.write_span(0, 0, 0, &[""], Style::default()), None);
        // Ragged rows.
        assert_eq!(
            grid.write_span(0, 0, 0, &["ab", "c"], Style::default()),
            None
        );
        // Too wide / too tall for the grid at this origin.
        assert_eq!(grid.write_span(0, 3, 0, &["ab"], Style::default()), None);
        assert_eq!(
            grid.write_span(0, 0, 3, &["a", "b"], Style::default()),
            None
        );
        // Nothing was written by any of the above.
        for y in 0..4 {
            for x in 0..4 {
                assert!(
                    grid[Pos::new(x, y)].is_empty(),
                    "({x}, {y}) should be untouched"
                );
            }
        }
    }

    #[test]
    fn write_span_takes_any_as_ref_str_row() {
        let mut grid = Grid::new(4, 4);
        // A footprint computed at runtime: owned rows, no borrowing pass over them.
        let rows: Vec<String> = (0..2)
            .map(|row| {
                (0..2)
                    .map(|col| if (row, col) == (0, 0) { 'C' } else { ' ' })
                    .collect()
            })
            .collect();

        assert_eq!(grid.write_span(0, 0, 0, &rows, Style::default()), Some(()));
        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'C');
        assert_eq!(grid[Pos::new(0, 0)].span(), (2, 2));
    }

    #[test]
    fn write_span_uniform_writes_the_anchor_once_and_fills_the_rest() {
        let mut grid = Grid::new(4, 4);
        assert_eq!(
            grid.write_span_uniform(0, (1, 1), (2, 2), 'C', '.', Style::default()),
            Some(())
        );

        assert_eq!(grid[Pos::new(1, 1)].glyph(), 'C');
        assert_eq!(grid[Pos::new(1, 1)].span(), (2, 2));
        for (x, y) in [(2, 1), (1, 2), (2, 2)] {
            assert_eq!(grid[Pos::new(x, y)].glyph(), '.', "({x}, {y})");
            assert_eq!(grid.span_owner(0, x, y), Some(Pos::new(1, 1)));
        }
    }

    #[test]
    fn write_span_uniform_matches_the_equivalent_write_span() {
        let mut uniform = Grid::new(4, 4);
        uniform
            .write_span_uniform(0, (0, 0), (3, 2), 'C', ' ', Style::default())
            .unwrap();

        let mut rows = Grid::new(4, 4);
        rows.write_span(0, 0, 0, &["C  ", "   "], Style::default())
            .unwrap();

        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(
                    uniform[Pos::new(x, y)],
                    rows[Pos::new(x, y)],
                    "({x}, {y}) differs"
                );
            }
        }
    }

    #[test]
    fn write_span_uniform_rejects_a_degenerate_or_oversized_footprint() {
        let mut grid = Grid::new(4, 4);
        let style = Style::default();

        assert_eq!(
            grid.write_span_uniform(0, (0, 0), (0, 2), 'C', ' ', style),
            None
        );
        assert_eq!(
            grid.write_span_uniform(0, (0, 0), (2, 0), 'C', ' ', style),
            None
        );
        // A span's dimensions are one byte each.
        assert_eq!(
            grid.write_span_uniform(0, (0, 0), (256, 1), 'C', ' ', style),
            None
        );
        // Does not fit the grid at this origin.
        assert_eq!(
            grid.write_span_uniform(0, (3, 0), (2, 1), 'C', ' ', style),
            None
        );

        for y in 0..4 {
            for x in 0..4 {
                assert!(
                    grid[Pos::new(x, y)].is_empty(),
                    "({x}, {y}) should be untouched"
                );
            }
        }
    }

    #[test]
    fn writing_into_a_covered_cell_clears_the_whole_span() {
        let mut grid = Grid::new(4, 4);
        grid.write_span(0, 0, 0, &["C=", "[]"], Style::default())
            .unwrap();
        grid.put_tile(0, (1, 1), Tile::new('x', Style::default()));

        assert_eq!(grid[Pos::new(1, 1)].glyph(), 'x');
        for (x, y) in [(0, 0), (1, 0), (0, 1)] {
            let tile = grid[Pos::new(x, y)];
            assert!(tile.is_empty(), "({x}, {y}) should have been cleared");
            assert_eq!(tile.flags(), TileFlags::EMPTY);
        }
    }

    #[test]
    fn writing_over_the_anchor_clears_the_whole_span() {
        let mut grid = Grid::new(4, 4);
        grid.write_span(0, 0, 0, &["C=", "[]"], Style::default())
            .unwrap();
        grid.put_tile(0, (0, 0), Tile::new('x', Style::default()));

        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'x');
        assert_eq!(grid[Pos::new(0, 0)].span(), (1, 1));
        for (x, y) in [(1, 0), (0, 1), (1, 1)] {
            assert!(
                grid[Pos::new(x, y)].is_empty(),
                "({x}, {y}) should be cleared"
            );
        }
    }

    #[test]
    fn overlapping_spans_erase_the_old_one_entirely() {
        let mut grid = Grid::new(4, 4);
        grid.write_span(0, 0, 0, &["AB", "CD"], Style::default())
            .unwrap();
        // Overlaps the first span's bottom-right cell only; all four of its cells must go.
        grid.write_span(0, 1, 1, &["EF", "GH"], Style::default())
            .unwrap();

        assert!(grid[Pos::new(0, 0)].is_empty());
        assert!(grid[Pos::new(1, 0)].is_empty());
        assert!(grid[Pos::new(0, 1)].is_empty());
        assert_eq!(grid[Pos::new(1, 1)].glyph(), 'E');
        assert_eq!(grid.span_owner(0, 2, 2), Some(Pos::new(1, 1)));
    }

    #[test]
    fn clear_span_works_from_any_cell_of_the_span() {
        let mut grid = Grid::new(4, 4);
        for from in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            grid.write_span(0, 0, 0, &["C=", "[]"], Style::default())
                .unwrap();
            grid.clear_span(0, from.0, from.1);
            for y in 0..2 {
                for x in 0..2 {
                    assert!(
                        grid[Pos::new(x, y)].is_empty(),
                        "clearing from {from:?}: ({x}, {y})"
                    );
                }
            }
        }
        // A cell that is not part of a span is left alone.
        grid.put_tile(0, (3, 3), Tile::new('z', Style::default()));
        grid.clear_span(0, 3, 3);
        assert_eq!(grid[Pos::new(3, 3)].glyph(), 'z');
    }

    #[test]
    fn spans_are_layer_scoped() {
        let mut grid = Grid::new(4, 4);
        grid.write_span(1, 0, 0, &["C=", "[]"], Style::default())
            .unwrap();
        assert_eq!(grid.span_owner(1, 1, 1), Some(Pos::new(0, 0)));
        // Layer 0 knows nothing about layer 1's span, and writing there leaves it intact.
        assert_eq!(grid.span_owner(0, 1, 1), None);
        assert_eq!(grid.span_owner(0, 0, 0), None);
        grid.put_tile(0, (1, 1), Tile::new('x', Style::default()));
        assert_eq!(grid.span_owner(1, 1, 1), Some(Pos::new(0, 0)));
    }

    #[test]
    fn flatten_into_carries_span() {
        // Cell backends receive the flattened grid, so a span on a higher layer has to survive
        // flattening with both its flags *and* its span fields, or every covered cell ends up
        // naming an anchor that isn't there.
        let mut grid = Grid::new(4, 4);
        grid.write_span(2, 1, 1, &["C=", "[]"], Style::default())
            .unwrap();

        let mut flat = Grid::new(4, 4);
        grid.flatten_into(&mut flat);

        assert_eq!(flat[Pos::new(1, 1)].span(), (2, 2));
        assert!(
            flat[Pos::new(1, 1)]
                .flags()
                .contains(TileFlags::SPAN_ANCHOR)
        );
        assert_eq!(flat.span_owner(0, 2, 2), Some(Pos::new(1, 1)));
        assert_eq!(flat[Pos::new(2, 2)].glyph(), ']');
    }

    #[test]
    fn blit_degrades_a_span_to_its_fallback_glyphs() {
        // `src_rect` can clip a footprint in half, and half a span is not representable, so
        // `blit` drops the span role and keeps the glyphs (which are the text fallback anyway).
        let mut src = Grid::new(4, 4);
        src.write_span(0, 0, 0, &["C=", "[]"], Style::default())
            .unwrap();

        let mut dst = Grid::new(4, 4);
        dst.blit(0, &src, Rect::new(0, 0, 2, 2), 0, 0);

        assert_eq!(dst[Pos::new(0, 0)].glyph(), 'C');
        assert_eq!(dst[Pos::new(1, 1)].glyph(), ']');
        assert_eq!(dst[Pos::new(0, 0)].span(), (1, 1));
        assert_eq!(dst.span_owner(0, 1, 1), None);
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let flags = dst[Pos::new(x, y)].flags();
            assert!(!flags.contains(TileFlags::SPAN_ANCHOR), "({x}, {y})");
            assert!(!flags.contains(TileFlags::SPAN_COVERED), "({x}, {y})");
        }
    }

    #[test]
    fn blit_leaves_a_dangling_span_anchor_in_the_destination() {
        // retroglyph#710: `blit` writes straight into the destination buffer, bypassing
        // `put_tile`'s `clear_span_overlap` call, so overwriting a span's covered cell used to
        // leave the anchor still claiming a cell the blit had just replaced.
        let mut dst = Grid::new(4, 1);
        dst.write_span(0, 0, 0, &["ab"], Style::default()).unwrap();

        let mut src = Grid::new(4, 1);
        src.put_tile(0, (1, 0), Tile::new('X', Style::default()));
        dst.blit(0, &src, Rect::new(1, 0, 1, 1), 1, 0);

        assert_eq!(dst[Pos::new(1, 0)].glyph(), 'X');
        assert_eq!(dst.tile(0, Pos::new(0, 0)).map(Tile::span), Some((1, 1)));
        assert!(!dst[Pos::new(0, 0)].flags().contains(TileFlags::SPAN_ANCHOR));
        assert!(
            !dst[Pos::new(1, 0)]
                .flags()
                .contains(TileFlags::SPAN_COVERED)
        );
    }

    #[test]
    fn blit_alpha_leaves_a_dangling_span_anchor_in_the_destination() {
        // Same bug as `blit_leaves_a_dangling_span_anchor_in_the_destination`, but through
        // `blit_alpha`'s separate copy path.
        let mut dst = Grid::new(4, 1);
        dst.write_span(0, 0, 0, &["ab"], Style::default()).unwrap();

        let mut src = Grid::new(4, 1);
        src.put_tile(0, (1, 0), Tile::new('X', Style::default()));
        dst.blit_alpha(
            0,
            &src,
            Rect::new(1, 0, 1, 1),
            1,
            0,
            BlendMode::Linear,
            1.0,
            1.0,
        );

        assert_eq!(dst[Pos::new(1, 0)].glyph(), 'X');
        assert_eq!(dst.tile(0, Pos::new(0, 0)).map(Tile::span), Some((1, 1)));
    }

    #[test]
    fn clear_region_clears_a_span_it_only_partly_covers() {
        let mut grid = Grid::new(4, 4);
        grid.write_span(0, 0, 0, &["C=", "[]"], Style::default())
            .unwrap();
        // Only the anchor cell is inside the region, but the whole span must go.
        grid.put_tile(0, (0, 0), Tile::default());
        for y in 0..2 {
            for x in 0..2 {
                assert!(grid[Pos::new(x, y)].is_empty(), "({x}, {y})");
            }
        }
    }
}

/// Property tests for the wide-character (EGC) grid invariants.
///
/// These exercise the trickiest code in the crate (`write_grapheme` and its
/// `clear_overlap` helper) by hammering a small grid with random sequences of
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
                let flags = grid[Pos::new(x, y)].flags();
                let lead = flags.contains(TileFlags::WIDE_CHAR);
                let spacer = flags.contains(TileFlags::WIDE_CHAR_SPACER);

                assert!(
                    !(lead && spacer),
                    "cell ({x}, {y}) is both wide lead and spacer"
                );

                if lead {
                    assert!(x + 1 < grid.width(), "wide lead at ({x}, {y}) has no room");
                    assert!(
                        grid[Pos::new(x + 1, y)]
                            .flags()
                            .contains(TileFlags::WIDE_CHAR_SPACER),
                        "wide lead at ({x}, {y}) is missing its spacer"
                    );
                }

                if spacer {
                    assert!(x > 0, "orphan spacer at ({x}, {y}) (no cell to the left)");
                    assert!(
                        grid[Pos::new(x - 1, y)]
                            .flags()
                            .contains(TileFlags::WIDE_CHAR),
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
                // at the end: an intermediate orphan would be a real bug.
                assert_wide_invariants(&grid);
            }
        }
    }
}
