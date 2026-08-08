//! The layered tile grid: [`Grid`](crate::grid::Grid), plus the [`Size`](crate::grid::Size), [`Pos`](crate::grid::Pos), and [`Rect`](crate::grid::Rect)
//! coordinate types used throughout the crate.
//!
//! # Layers, draw order, and compositing
//!
//! A [`Grid`](crate::grid::Grid) holds up to 256 independent layers (`u8` ids `0..=255`), one
//! [`Tile`](crate::tile::Tile) per cell on each. Layer 0 is always allocated; layers 1-255 are
//! allocated lazily, on first write to that layer (see
//! [`put_tile`](crate::grid::Grid::put_tile)): a
//! single-layer game pays zero overhead for layers it never writes to. This is the
//! crate's most distinctive feature and the one most worth understanding
//! before reaching for a second layer.
//!
//! Each cell carries a glyph, foreground/background [`Color`](crate::color::Color), and
//! sub-cell pixel offsets. [`Color`](crate::color::Color) covers the full spectrum: the
//! terminal's default foreground/background, the 16 standard ANSI colors, the 256-color
//! palette, and 24-bit RGB.
//!
//! ## Draw order
//!
//! ```text
//!   painted last  ->  layer 3   UI / effects     (topmost)
//!                     layer 2   actors
//!                     layer 1   items
//!   painted first ->  layer 0   terrain          (always allocated)
//!
//!                     ascending id == ascending z; no separate depth field.
//!                     each layer holds one Tile per cell of the whole grid.
//! ```
//!
//! Layers composite bottom-to-top, in ascending id order: 0 first, then every
//! allocated layer up to [`max_layer`](crate::grid::Grid::max_layer), each painted over
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
//! (see [`crate::backend::Output::composites_layers`]):
//!
//! - **Cell backends** (`Headless`, `retroglyph-crossterm`) do not composite
//!   layers themselves. [`crate::terminal::Terminal::present`] calls
//!   `flatten_into` (crate-private) to collapse every allocated layer
//!   into a single-layer frame *before* handing it to the backend, so
//!   layers 1+ behave identically on every cell backend.
//! - **Pixel backends** (`retroglyph-software`) composite per pixel: they
//!   receive the raw layered stream from
//!   [`crate::backend::Output::draw_layers`] (layer-major, ascending id) and paint
//!   each layer's cells directly onto the pixel buffer in that order.
//!
//! ## The `EMPTY` flag: transparency vs. opaque occlusion
//!
//! Every [`Tile`](crate::tile::Tile) carries [`TileFlags::EMPTY`], set on [`Tile::default`](crate::tile::Tile::default) and
//! cleared by every write (`put_tile`, `write_grapheme`, indexing, ...).
//! Compositing treats it as the transparency bit:
//!
//! - An **untouched cell** (`EMPTY` set) is fully transparent:
//!   [`blit`](crate::grid::Grid::blit) skips it, and `flatten_into` (crate-private)
//!   leaves whatever the layers below already drew.
//! - An **explicit space** (`Tile::new(' ', style)`, `EMPTY` clear) is
//!   opaque: it overwrites the glyph and foreground below it, same as any
//!   other character. This is the one sharp edge in the model: `' '`
//!   painted on a higher layer *erases* content underneath, it does not
//!   reveal it.
//!
//! Background color follows its own rule, independent of `EMPTY`: a tile's
//! background only overwrites the composited background when it is not
//! [`Color::Default`](crate::color::Color::Default). A non-empty tile with a `Default`
//! background still lets a lower layer's background show through even though its glyph is
//! opaque. See `flatten_into` (crate-private) for the exact rule.
//!
//! ## Multi-cell spans
//!
//! [`write_span`](crate::grid::Grid::write_span) writes one piece of artwork across a `w x h` block of cells:
//! the top-left cell is the **anchor** ([`TileFlags::SPAN_ANCHOR`], carrying the footprint), and
//! every other cell is **covered** ([`TileFlags::SPAN_COVERED`], carrying its offset back to the
//! anchor). [`span_owner`](crate::grid::Grid::span_owner) resolves any cell of a span to its anchor in O(1),
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
//! ([`put_tile`](crate::grid::Grid::put_tile), [`write_grapheme`](crate::grid::Grid::write_grapheme))
//! clears the entire span first, so an anchor can never be left claiming cells it no longer owns.
//! The exceptions are the escape hatches that hand out a `&mut Tile` directly
//! ([`tile_mut`](crate::grid::Grid::tile_mut), `IndexMut`), which cannot intercept the
//! write; use [`clear_span`](crate::grid::Grid::clear_span) first if you reach for one of those on a grid
//! that uses spans.
//!
//! ## Naming: `put_*`/`write_*`/`print_*`
//!
//! `put_*` names a raw single-slot write: it stores exactly the [`Tile`](crate::tile::Tile)/`char`/grapheme
//! given, with no text interpretation. [`Grid::put_tile`](crate::grid::Grid::put_tile) is the base case; [`Surface::put`](crate::surface::Surface::put)
//! and its `put_grapheme`/`put_signed`/`put_offset`/`put_span*` siblings are all `put_tile` plus
//! surface-local bookkeeping (clipping, tint, coordinate translation), never more than one
//! caller-given unit per call.
//!
//! `write_*` is reserved for a [`Grid`](crate::grid::Grid) write that does more than copy a slot: [`write_grapheme`](crate::grid::Grid::write_grapheme)
//! adds wide-character/extra-storage bookkeeping on top of `put_tile`, and [`write_span`](crate::grid::Grid::write_span)/
//! [`write_span_uniform`](crate::grid::Grid::write_span_uniform) claim a multi-cell footprint (see "Multi-cell spans" above). If a new
//! `Grid` method's job is "store this one thing, with some extra rules for what \"this one
//! thing\" means", it's `write_*`; if it's "store exactly the tile I hand you", it's `put_*`.
//!
//! `print_*` lives only on [`Surface`](crate::surface::Surface), for its string/[`Line`](crate::text::Line)-oriented
//! convenience layer: [`Surface::print`](crate::surface::Surface::print)/[`print_line`](crate::surface::Surface::print_line)/[`print_aligned`](crate::surface::Surface::print_aligned) take a whole
//! run of text and handle newlines, wrapping, and alignment by making repeated `put`/`put_grapheme`
//! calls internally; they never appear on `Grid`, which has no concept of a text run.
//!
//! This mirrors how `retroglyph-ui`'s `Ui::show`/`Ui::draw` settled its own naming split: named once
//! here, rather than re-litigated per method, and a mismatch (a `put`-shaped operation named with
//! `write_`, or vice versa) is a bug to fix, not a style choice to leave alone.
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
//! layer 255). See [`max_layer`](crate::grid::Grid::max_layer) for the iteration cost this
//! implies and [`Grid::new`](crate::grid::Grid::new) for the allocation cost of a first write.

use crate::color::Tint;
use crate::tile::Tile;
use crate::tile::TileFlags;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
// Aliased rather than imported as `BlendMode`: this module already defines its own `BlendMode`
// (below), and alpha-blend 0.3 renamed `blend_modes::SeparableBlendMode` to a top-level
// `BlendMode` of its own, which would otherwise collide.
use alpha_blend::BlendMode as SeparableBlendMode;
use grixy::buf::GridBuf;
use grixy::ops::layout::{LinearLayout, RowMajor};

/// `.width()`/`.height()` accessors for [`Size`](crate::grid::Size) (and [`Rect`](crate::grid::Rect)): re-exported so callers don't need
/// a direct `ixy` dependency just to call them on this crate's own type aliases.
pub use ixy::HasSize;

mod api;
mod diff;
mod layers;
mod spans;
mod trait_impls;

/// Blend mode for [`Grid::blit_alpha`](crate::grid::Grid::blit_alpha), selecting how source and destination colors combine
/// before the `fg_alpha`/`bg_alpha` factor is applied.
///
/// [`Linear`](Self::Linear) is a straight per-channel color lerp, delegated to [`gem::Mix`]. The
/// remaining variants are the [W3C separable blend modes] libtcod also offers: each computes a
/// fully blended color per channel via [`alpha_blend::BlendMode`] (imported here under its old
/// name, [`SeparableBlendMode`], to avoid colliding with this module's own [`BlendMode`](crate::grid::BlendMode)), and
/// *that* result is what gets lerped against the destination by the alpha factor, in place of the
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
    Screen,
    /// Brightens the destination to reflect the source (aka "color dodge").
    Dodge,
    /// Darkens the destination to reflect the source (aka "color burn").
    Burn,
    /// Multiplies or screens the colors, depending on the destination.
    Overlay,
    /// Darkens: `dst * src`. Always at least as dark as either input; the complement of Screen.
    Multiply,
}

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
/// `width`/`height` are public fields, readable directly (`size.width`). For method-call style
/// (`size.width()`), bring [`HasSize`] into scope: it's re-exported from this module rather than
/// `ixy` so callers never need a direct `ixy` dependency just to call it.
///
/// # Examples
///
/// ```
/// use retroglyph_core::grid::{HasSize, Size};
///
/// let size = Size::new(80, 24);
/// assert_eq!(size.width, 80);
/// assert_eq!(size.width(), 80);
/// ```
///
/// This crate's `serde` feature forwards to [`ixy`]'s own `serde` feature, so `Size` gains
/// `Serialize`/`Deserialize` from its upstream definition rather than one defined here.
pub type Size = ixy::Size<u16>;

/// Pos in the grid, in (x = column, y = row) order.
///
/// Implements [`Ord`] in row-major order (y primary, then x), which is the
/// natural ordering for terminal rendering: top-to-bottom, left-to-right within
/// each row.
///
/// # Examples
///
/// ```
/// use retroglyph_core::grid::Pos;
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
/// `.width()`/`.height()` are inherent methods here (unlike on [`Size`]), so no [`HasSize`]
/// import is needed to call them.
///
/// # Examples
///
/// ```
/// use retroglyph_core::grid::Rect;
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
/// This crate's `serde` feature adds `Serialize`/`Deserialize` impls for `Offset` directly (unlike
/// [`Size`]/[`Pos`]/[`Rect`], which forward to [`ixy`]'s own `serde` feature).
///
/// # Examples
///
/// ```
/// use retroglyph_core::grid::Offset;
///
/// let offset = Offset::new(3, -2);
/// assert_eq!(offset.dx, 3);
/// assert_eq!(offset.dy, -2);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

// ---------------------------------------------------------------------------
// Helpers: coordinate conversion between u16 and usize
// ---------------------------------------------------------------------------

fn to_grixy_pos(pos: Pos) -> grixy::core::Pos {
    grixy::core::Pos::new(usize::from(pos.x), usize::from(pos.y))
}

/// Decodes a flat row-major buffer index into `(x, y)`, given the buffer's `width`.
///
/// Delegates to [`RowMajor`]'s [`LinearLayout::index_to_pos`](grixy::ops::layout::LinearLayout)
/// instead of hand-rolling `i % width` / `i / width` at each flat-buffer iterator below.
fn flat_index_to_xy(i: usize, width: usize) -> (u16, u16) {
    let pos = RowMajor::index_to_pos(i, width);
    #[allow(clippy::cast_possible_truncation)]
    (pos.x as u16, pos.y as u16)
}

// ---------------------------------------------------------------------------
// LayerBuf: a single layer's flat buffer
// ---------------------------------------------------------------------------

/// A single layer in the grid: a flat 2D buffer of one tile per cell.
///
/// Layer 0 is always allocated. Layers 1–255 are allocated on first write
/// (see [`Grid::put_tile`](crate::grid::Grid::put_tile)).
#[derive(Clone)]
pub(crate) struct LayerBuf {
    pub(crate) buf: GridBuf<Tile, Vec<Tile>, RowMajor>,
    /// Sparse side-table: flat row-major index -> the cell's out-of-line data, for tiles with
    /// [`TileFlags::HAS_EXTRA`] set. Empty until something writes a multi-codepoint grapheme or
    /// a tint, which is what keeps [`Tile`](crate::tile::Tile) itself small (see
    /// [`DrawCell::grapheme`](crate::backend::DrawCell::grapheme) and [`Grid::tint`](crate::grid::Grid::tint)).
    ///
    /// The `HAS_EXTRA` flag is authoritative: readers must check it before
    /// consulting this map, since some write paths (`put_tile`,
    /// `IndexMut`) can leave a stale entry behind when they
    /// overwrite a tile that used to carry extra data without an explicit
    /// cleanup call. Since those paths only ever hand out or store tiles
    /// with `HAS_EXTRA` clear, a stale entry is harmless: it is simply
    /// never looked up until the slot is reused by `write_grapheme` or `set_tint`, which
    /// always overwrite it.
    extras: BTreeMap<usize, TileExtra>,
}

/// One cell's out-of-line data: everything that belongs to a tile but does not fit in one.
///
/// [`Tile`](crate::tile::Tile) is exactly 20 bytes with no padding to spare, and both members here are rare enough
/// per cell that inlining either would grow every tile of every layer to pay for a minority of
/// them. They share one table, one flag, and one set of rekeying paths rather than each bringing
/// their own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TileExtra {
    /// The full grapheme cluster, when [`Tile::glyph`](crate::tile::Tile::glyph) holds only its first codepoint.
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

    /// Returns the tint for the tile at flat index `idx`, or [`Tint::None`](crate::color::Tint::None) if `tile` doesn't
    /// have [`TileFlags::HAS_EXTRA`] set.
    fn tint_for(&self, idx: usize, tile: &Tile) -> Tint {
        self.entry_for(idx, tile).map_or(Tint::None, |e| e.tint)
    }

    /// Returns a clone of the side-table entry at flat index `idx`, or `None` if `tile` doesn't
    /// have [`TileFlags::HAS_EXTRA`] set. Used to copy a cell's out-of-line data between grids
    /// (e.g. [`Grid::blit`](crate::grid::Grid::blit)); the grapheme rides along as an `Arc` clone rather than a fresh
    /// allocation.
    fn extra_entry_for(&self, idx: usize, tile: &Tile) -> Option<TileExtra> {
        self.entry_for(idx, tile).cloned()
    }
}

// ---------------------------------------------------------------------------
// Grid
// ---------------------------------------------------------------------------

/// A 2D buffer of [`Tile`](crate::tile::Tile)s, addressable across up to 256 stacked layers.
///
/// Layer 0 is always allocated; higher layers are allocated on first write, growing the
/// layer-table `Vec` up to that layer's id as needed (see [`Grid::new`](crate::grid::Grid::new)). Single-layer use pays
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
/// use retroglyph_core::color::{Color, Style};
/// use retroglyph_core::grid::{Grid, Pos};
///
/// let mut grid = Grid::new(10, 5);
/// grid.put_tile(0, Pos::new(2, 1), retroglyph_core::tile::Tile::new('@', Style::new().fg(Color::GREEN)));
/// assert_eq!(grid[Pos::new(2, 1)].glyph(), '@');
/// ```
#[derive(Clone)]
#[doc(alias = "buffer")] // ratatui
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
    /// Borrows a specific layer, or `None` if unallocated.
    ///
    /// `id` may be beyond the current layer-table `Vec`'s length: the table only grows as far
    /// as the highest layer id ever written (see [`layer_or_alloc`](Self::layer_or_alloc)), so an
    /// id past the end simply means "never written", same as an in-bounds `None` slot.
    fn layer(&self, id: u8) -> Option<&LayerBuf> {
        self.layers.get(usize::from(id))?.as_ref()
    }

    /// Borrows a specific layer mutably, allocating it if necessary.
    ///
    /// Grows the layer-table `Vec` up to `id + 1` slots on demand, rather than the table always
    /// holding all 256 possible slots (see retroglyph#264): a `Grid` that only ever writes to
    /// layer 0, or a handful of low ids, never pays for the 250+ slots it never touches.
    ///
    /// A first write allocates one `width x height` buffer of [`Tile`](crate::tile::Tile)s, the same cost regardless
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
        self.layers[idx]
            .as_mut()
            .expect("idx was just allocated above if it wasn't already Some")
    }

    /// Borrows layer 0 (always allocated).
    fn layer0(&self) -> &LayerBuf {
        // INVARIANT: layer 0 is always `Some` (set in `new`).
        self.layers[0]
            .as_ref()
            .expect("layer 0 is always Some (set in Grid::new)")
    }

    /// Borrows layer 0 mutably (always allocated).
    fn layer0_mut(&mut self) -> &mut LayerBuf {
        self.layers[0]
            .as_mut()
            .expect("layer 0 is always Some (set in Grid::new)")
    }

    /// Copy `layer` from `src` into `self` verbatim: the raw tile buffer (including every
    /// flag, so [`TileFlags::SPAN_ANCHOR`]/
    /// [`TileFlags::SPAN_COVERED`] survive) and every
    /// extra (grapheme, tint), with no transparency rule skipping empty cells and no span
    /// degradation.
    ///
    /// Unlike [`blit`](Self::blit), this is not a clipping/positioning copy: it requires `self`
    /// and `src` to share the same dimensions and always writes `layer` at the same coordinates
    /// it reads it from, so a caller can't use it to move or crop content, only to make one
    /// grid's layer an exact replica of another's. That is exactly what [`crate::terminal::Terminal::present`]'s
    /// `retain_layer` support needs: a retained layer has to be indistinguishable from what was
    /// presented last frame, and `blit`'s clipping-copy contract (degrade spans to their text
    /// fallback, treat empty tiles as transparent) is wrong for a copy that is supposed to be a
    /// full replacement of identical geometry.
    ///
    /// If `layer` is unallocated on `src`, it becomes unallocated on `self` too (mirroring an
    /// always-empty layer exactly). Layer 0 can never hit this case: it is always allocated on
    /// every `Grid` (see [`Grid::new`](crate::grid::Grid::new)), on `src` as much as on `self`.
    ///
    /// # Panics
    ///
    /// Panics if `self` and `src` do not have the same dimensions.
    pub(crate) fn copy_layer_from(&mut self, layer: u8, src: &Self) {
        assert_eq!(
            (self.width, self.height),
            (src.width, src.height),
            "copy_layer_from requires matching dimensions"
        );
        let idx = usize::from(layer);
        match src.layers.get(idx).and_then(Option::as_ref) {
            Some(src_lb) => {
                if idx >= self.layers.len() {
                    self.layers.resize_with(idx + 1, || None);
                }
                self.layers[idx] = Some(src_lb.clone());
                if layer > self.max_layer {
                    self.max_layer = layer;
                }
            }
            None => {
                if idx < self.layers.len() {
                    self.layers[idx] = None;
                }
            }
        }
        self.has_spans |= src.has_spans;
    }
}

/// Test-only readback for a tile's grapheme text, standing in for the removed
/// `Grid::grapheme` (see retroglyph#1016): every real consumer reads this off the
/// [`DrawCell`](crate::backend::DrawCell) stream returned by [`Grid::layers`], so tests do too.
#[cfg(test)]
pub(crate) fn grapheme_at(grid: &Grid, layer: u8, x: u16, y: u16) -> Option<&str> {
    grid.layers()
        .find(|c| c.layer == layer && c.pos == Pos::new(x, y))
        .and_then(|c| c.grapheme)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_pos_is_true_for_edges_and_false_for_the_exclusive_far_edge() {
        let r = Rect::new(2, 3, 4, 5);
        assert!(r.contains_pos(Pos::new(2, 3)));
        assert!(r.contains_pos(Pos::new(5, 7)));
        assert!(!r.contains_pos(Pos::new(6, 3))); // x == x+width, exclusive
        assert!(!r.contains_pos(Pos::new(2, 8))); // y == y+height, exclusive
        assert!(!r.contains_pos(Pos::new(1, 3)));
    }

    #[test]
    fn area_is_width_times_height_and_zero_for_default() {
        assert_eq!(Rect::new(0, 0, 5, 3).area(), 15);
        assert_eq!(Rect::default().area(), 0);
    }

    #[test]
    fn top_left_and_bottom_right_reflect_position_and_size() {
        let r = Rect::new(1, 2, 3, 4);
        assert_eq!(r.top_left(), Pos::new(1, 2));
        assert_eq!(r.bottom_right(), Pos::new(4, 6));
    }

    #[test]
    fn intersect_is_non_empty_only_when_rects_overlap() {
        let a = Rect::new(0, 0, 4, 4);
        let b = Rect::new(2, 2, 4, 4);
        let c = Rect::new(4, 0, 4, 4); // touches edge, no overlap
        assert!(!a.intersect(b).is_empty());
        assert!(a.intersect(c).is_empty());
    }

    #[test]
    fn pos_iter_yields_every_position_row_major() {
        use alloc::vec;
        use alloc::vec::Vec;

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
    fn pos_converts_to_and_from_a_tuple() {
        let p: Pos = (3u16, 7u16).into();
        assert_eq!(p, Pos::new(3, 7));
        let t: (u16, u16) = p.into();
        assert_eq!(t, (3, 7));
    }

    #[test]
    fn size_converts_to_and_from_a_tuple() {
        let s: Size = (80u16, 25u16).into();
        assert_eq!(s, Size::new(80, 25));
        let t: (u16, u16) = s.into();
        assert_eq!(t, (80, 25));
    }

    #[test]
    fn offset_converts_to_and_from_a_tuple() {
        let o: Offset = (-3i16, 7i16).into();
        assert_eq!(o, Offset::new(-3, 7));
        let t: (i16, i16) = o.into();
        assert_eq!(t, (-3, 7));
    }

    #[test]
    fn offset_default_is_zero() {
        assert_eq!(Offset::default(), Offset::new(0, 0));
    }

    #[test]
    fn pos_ord_sorts_row_major() {
        use alloc::vec;

        let mut positions = vec![Pos::new(5, 0), Pos::new(0, 1), Pos::new(3, 0)];
        positions.sort();
        assert_eq!(
            positions,
            vec![Pos::new(3, 0), Pos::new(5, 0), Pos::new(0, 1),]
        );
    }

    #[test]
    fn size_orders_by_width_then_height() {
        assert!(Size::new(1, 2) < Size::new(2, 1));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn size_serializes_and_deserializes() {
        let size = Size::new(80, 25);
        let json = serde_json::to_string(&size).expect("serialize");
        assert_eq!(
            serde_json::from_str::<Size>(&json).expect("deserialize"),
            size
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn pos_and_rect_serialize_via_ixy() {
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

    #[cfg(feature = "serde")]
    #[test]
    fn offset_serializes_and_deserializes() {
        let offset = Offset::new(3, -2);
        let json = serde_json::to_string(&offset).expect("serialize");
        assert_eq!(
            serde_json::from_str::<Offset>(&json).expect("deserialize"),
            offset
        );
    }
}
