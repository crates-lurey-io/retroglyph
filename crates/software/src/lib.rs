//! CPU rasterization backend: renders grid cells into a pixel buffer and
//! blits it to a window surface via `softbuffer`.
//!
//! # Architecture
//!
//! [`SoftwareBackend`] holds configuration only (font chain, grid size, scale); it
//! does not implement [`Backend`](retroglyph_core::backend::Backend). Call
//! [`into_renderer`](SoftwareBackend::into_renderer) to build a
//! [`SoftwareRenderer`], which does the actual rendering work:
//!
//! ```text
//! SoftwareBackend (config: font chain, grid size, scale)
//!   |  .into_renderer()
//!   v
//! SoftwareRenderer
//!   implements retroglyph_core::{Output, Input, Cursor} (= Backend)
//!   implements retroglyph_window::presenter::Presenter (an Output supertrait)
//!   |                                |
//!   |                                v
//!   |                     wrapped in retroglyph_window::backend::WindowBackend,
//!   |                     driven by a windowing loop (retroglyph-window's
//!   |                     winit integration, or any other source of
//!   |                     raw window handles)
//!   v                                |
//! Terminal<SoftwareRenderer>          v
//! (headless / pixel tests,   softbuffer::Surface -> OS window
//!  inspect via .pixels())
//! ```
//!
//! This crate does not depend on winit. [`SoftwareRenderer`] implements
//! [`Presenter`](retroglyph_window::presenter::Presenter) against raw window handles
//! ([`WindowHandle`]), so anything that produces those (winit via
//! `retroglyph-window`, or another windowing library) can drive it. Because
//! `Presenter` is an [`Output`] supertrait, `SoftwareRenderer`'s single
//! `Output` implementation satisfies both `Backend`'s output half and `Presenter` directly, with
//! no duplicated method bodies. `retroglyph-window`'s
//! [`WindowBackend`](retroglyph_window::backend::WindowBackend) wraps a `Presenter` to provide the full
//! [`Backend`](retroglyph_core::backend::Backend) for windowed use, owning the input event queue that this
//! crate does not.
//!
//! For headless use (in-memory rendering, pixel-level tests) skip windowing
//! entirely: [`SoftwareRenderer`] implements [`Output`],
//! [`Input`], and [`Cursor`] directly (bundled
//! as [`Backend`](retroglyph_core::backend::Backend)), so `Terminal<SoftwareRenderer>` works without a
//! window, and [`pixels`](SoftwareRenderer::pixels) gives direct access to the rendered
//! buffer.
//!
//! # Features
//!
//! <!-- gen-features:start -->
//! This crate has no default features; every feature below is optional and off unless enabled.
//!
//! ### `default-font`
//!
//! ⚪ Optional.
//!
//! Embeds the Unscii 16 bitmap font as a ready-to-use default `FontChain`.
//!
//! Forwards to `retroglyph-window`'s `default-font` feature.
//!
//! ### `dev`
//!
//! ⚪ Optional.
//!
//! Forwards `retroglyph-core`'s `dev` feature, which forces development diagnostics on in a build
//! that would otherwise compile them out (see [`retroglyph_core::dev`]).
//!
//! ### `tilesets`
//!
//! ⚪ Optional.
//!
//! PNG sprite sheet tilesets with alpha-blended CPU blit support.
//!
//! Adds `alpha-blend` and forwards to `retroglyph-window`'s `tilesets` feature for decode/config.
//! <!-- gen-features:end -->

#![doc(
    html_logo_url = "https://raw.githubusercontent.com/crates-lurey-io/retroglyph/main/docs/public/assets/logo.svg"
)]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/crates-lurey-io/retroglyph/main/docs/public/assets/logo.svg"
)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod config;

// The sprite/tileset decode + config now lives in `retroglyph-window` (winit-free), shared with
// `retroglyph-gl` the same way `BitmapFont` is. Re-exported here so the existing
// `retroglyph_software::tileset` / `::sprite_cache` paths keep working.
#[cfg(feature = "tilesets")]
pub use retroglyph_window::{sprite_cache, tileset};

// Platform-specific window surface. Both modules expose a `WindowSurface` with
// the same `new`/`resize`/`present` API and their own `SurfaceError`, so the
// renderer below drives either without `cfg` in its body. This is the same
// module-swap pattern std uses for `std::sys`.
#[cfg(not(target_arch = "wasm32"))]
#[path = "surface_native.rs"]
pub mod surface;
#[cfg(target_arch = "wasm32")]
#[path = "surface_wasm.rs"]
pub mod surface;

use surface::SurfaceError;
use surface::WindowSurface;

// Compile the code blocks in this crate's own README as doctests so its quick start is
// type-checked on every test run and cannot silently rot. The `cfg(doctest)` gate keeps this out
// of the rendered crate documentation: see `retroglyph-crossterm`'s matching include for the
// same pattern applied to the workspace root README.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

use retroglyph_core::backend::DrawCell;
use retroglyph_core::backend::{Compositing, Cursor, Input, Output};
use retroglyph_core::color::Color;

// The bitmap font lives in `retroglyph-window`'s winit-free `font` module (both graphical
// backends already depend on that crate for `Presenter`), so `retroglyph-gl` shares the exact
// same glyph source. Re-exported here for ergonomics (the builder's `font()` takes either),
// sparing a consumer an extra direct dependency on `retroglyph-window` just for this type, the
// same rationale as `retroglyph-core`'s kept `HasSize` re-export; `unscii16` etc. are reached
// through `retroglyph_window::font` directly.
pub use retroglyph_window::font::{BitmapFont, FontChain};

#[cfg(feature = "tilesets")]
use alpha_blend::rgba::U8x4Rgba;
use config::{SoftwareBackend, SoftwareBackendError};
use grixy::buf::GridBuf;
use grixy::ops::GridWrite;
use grixy::ops::layout::{LinearLayout, RowMajor};
use retroglyph_core::color::Tint;
use retroglyph_core::event::{Event, coalesces_with};
use retroglyph_core::grid::HasSize;
use retroglyph_core::grid::{Pos, Size};
use retroglyph_core::tile::Tile;
use retroglyph_window::diagnostics::warn_notdef_glyph;
use retroglyph_window::geometry::CellGeometry;
use retroglyph_window::palette::{DEFAULT_BG, DEFAULT_FG};
use retroglyph_window::presenter::WindowHandle;
use retroglyph_window::presenter::cell_art_glyph;
#[cfg(feature = "tilesets")]
use retroglyph_window::sprite_cache::{
    Sprite, SpriteCache, SpriteTint, warn_sprite_needs_span, warn_tint_needs_sprite,
};
use std::collections::BTreeSet;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

// ── Public types ──────────────────────────────────────────────────────────────

/// A running software renderer, produced by [`SoftwareBackend::into_renderer`].
///
/// Unlike [`SoftwareBackend`] (which is just configuration), this type
/// always has an active rendering context: its pixel buffer is always
/// available, and the `ctx` field is never `None`, so [`Output`] methods
/// never panic for missing initialization.
///
/// Call [`pixels`](Self::pixels) to inspect the rendered output, or use
/// [`Output::draw`] and [`Output::draw_layers`] to render into it.
///
/// If the `tilesets` feature is enabled, the sprite tileset is loaded once, at
/// [`into_renderer`](SoftwareBackend::into_renderer) time, into an internal
/// [`SpriteCache`]. That cache has no reload/hot-swap support (see its
/// docs); to pick up a changed tileset, rebuild the renderer via a fresh [`SoftwareBackend`]
/// configuration rather than mutating this one.
pub struct SoftwareRenderer {
    options: SoftwareBackend,
    /// The font chain glyphs are resolved through, extracted from `options.fonts` at construction
    /// time. Always present; the `Option` wrapper in `SoftwareBackend` is only for the builder
    /// validation step.
    fonts: FontChain<'static>,
    ctx: RenderContext,
    #[cfg(feature = "tilesets")]
    sprite_cache: Arc<SpriteCache>,
}

struct RenderContext {
    event_buffer: VecDeque<Event>,
    pixel_buf: GridBuf<u32, Vec<u32>, RowMajor>,
    window_surface: Option<WindowSurface>,
    /// Cell/surface pixel geometry (glyph size x scale); constant for the renderer's lifetime
    /// (a grid resize changes cols/rows, never the cell size). The single source of the
    /// `cell_size` contract, delegated to by [`Presenter::cell_size`].
    geometry: CellGeometry,
    /// Shadow copy of `pixel_buf` from the previous frame, used to compute
    /// the damaged row range in [`present`](SoftwareRenderer::present).
    /// Kept the same length as `pixel_buf`; resized (and the whole frame
    /// marked damaged) whenever the buffer is resized.
    prev_pixels: Vec<u32>,
    /// Row range `[y0, y1)` changed since the last present, computed in
    /// `draw_layers` by diffing against `prev_pixels` and unioned into any
    /// existing band, since `draw_layers` may be called more than once
    /// between two `present()` calls. `None` means no rows changed
    /// (nothing to present).
    damage_rows: Option<(u32, u32)>,
    /// Shadow copy of every allocated layer's tiles and tints from the last `draw_layers` call,
    /// one [`LayerShadow`] per layer indexed `[layer_id]`. Used to find dirty cells without
    /// touching core's diff model: `draw_layers` already receives every cell on every allocated
    /// layer every frame (see
    /// [`Compositing::PixelLayered`](crate::backend::Compositing::PixelLayered)), so comparing
    /// against this shadow copy in place is enough to tell which cells actually changed, with no
    /// new core API needed. Each layer's `GridBuf`s are always replaced wholesale (via
    /// `GridBuf::new_filled`), never resized in place, whenever the grid dimensions change, so a
    /// layer's buffer and its declared width/height can never drift apart the way two parallel
    /// `Vec`s could (retroglyph#567); grown (never shrunk) as new layer ids are seen. Tiles and
    /// tints for the same layer are always reset together through [`RenderContext::invalidate_shadow`],
    /// so the two can no longer fall out of sync the way separate `prev_tiles`/`prev_tints` `Vec`s
    /// once did (retroglyph#567, retroglyph#694).
    layers: Vec<LayerShadow>,
    /// Reusable per-cell dirty scratch buffer, `true` at index `y * cols + x` when any layer's
    /// tile at that position changed this frame. Indexed the same way as each `layers` entry;
    /// resized alongside it.
    dirty_mask: Vec<bool>,
    /// Number of layers (`max layer id + 1`) present in the last `draw_layers` call. A change in
    /// this count between frames (a layer being newly allocated or fully deallocated) forces a
    /// full repaint next frame, since the dirty-cell path can only compare cells within layers
    /// present in both frames.
    prev_layer_count: usize,
    /// Whether any tile in the last `draw_layers` call had a nonzero sub-cell offset
    /// (`Tile::dx`/`Tile::dy`). A frame that removes the last offset needs a full repaint just as
    /// much as one that introduces one: the spilled pixels it painted into a neighbor cell live
    /// outside that neighbor's own tile, so a neighbor whose own tile is unchanged is never
    /// revisited by the dirty-cell path and the stale spill survives unless this frame's
    /// `full_repaint` also accounts for what the *previous* frame offset.
    prev_offset: bool,
    /// Glyphs already reported by [`warn_oversized_sprite`] as needing a span, so a 60fps redraw
    /// loop logs each one once instead of every frame.
    #[cfg(feature = "tilesets")]
    warned_oversized: BTreeSet<char>,
    /// Glyphs already reported by [`warn_tint_needs_sprite`] as having a dropped tint, so a 60fps
    /// redraw loop logs each one once instead of every frame.
    #[cfg(feature = "tilesets")]
    warned_dropped_tint: BTreeSet<char>,
    /// Characters already reported by [`warn_notdef_glyph`] as resolving to the fallback rather
    /// than their own glyph, so a 60fps redraw loop logs each one once instead of every frame.
    warned_notdef: BTreeSet<char>,
}

impl RenderContext {
    /// Discards the whole per-frame shadow (tiles, tints, dirty mask, layer count, offset flag)
    /// in one call, so the five members that must go stale together (retroglyph#567, #694) can no
    /// longer be reset in a hand-written, independently-forgettable sequence at each call site.
    fn invalidate_shadow(&mut self) {
        self.layers.clear();
        self.dirty_mask.clear();
        // Sentinel distinct from any real layer count (always < 256): forces the next
        // `draw_layers` call onto the full-repaint path, since the dirty-cell path can't diff
        // against a shadow that no longer describes the current frame's layer set.
        self.prev_layer_count = usize::MAX;
        self.prev_offset = false;
    }
}

/// One layer's per-cell shadow copy from the last `draw_layers` call: its tiles and their tints,
/// always allocated and replaced together so the two can't independently drift the way the
/// `prev_tiles`/`prev_tints` parallel `Vec`s they replace once did (retroglyph#567, retroglyph#694).
struct LayerShadow {
    tiles: GridBuf<Tile, Vec<Tile>, RowMajor>,
    tints: GridBuf<Tint, Vec<Tint>, RowMajor>,
}

impl LayerShadow {
    /// Allocates a shadow filled with default tiles/no tint, sized for a `cols` x `rows` grid.
    fn new(cols: usize, rows: usize) -> Self {
        Self {
            tiles: GridBuf::new_filled(cols, rows, Tile::default()),
            tints: GridBuf::new_filled(cols, rows, Tint::None),
        }
    }

    /// Whether this shadow's dimensions already match `cols` x `rows`, i.e. it can be reused as-is
    /// rather than replaced wholesale via [`LayerShadow::new`].
    fn fits(&self, cols: usize, rows: usize) -> bool {
        self.tiles.as_ref().len() == cols * rows
    }
}

impl SoftwareRenderer {
    /// Creates a new renderer with the given buffer and cell dimensions.
    pub(crate) fn create(
        options: SoftwareBackend,
        fonts: FontChain<'static>,
        buf_w: usize,
        buf_h: usize,
        geometry: CellGeometry,
        #[cfg(feature = "tilesets")] sprite_cache: Arc<SpriteCache>,
    ) -> Self {
        Self {
            options,
            fonts,
            ctx: RenderContext {
                event_buffer: VecDeque::new(),
                pixel_buf: GridBuf::from_buffer(vec![0u32; buf_w * buf_h], buf_w),
                window_surface: None,
                geometry,
                prev_pixels: vec![0u32; buf_w * buf_h],
                damage_rows: None,
                layers: Vec::new(),
                dirty_mask: Vec::new(),
                // Sentinel distinct from any real layer count (always < 256), so the very first
                // `draw_layers` call is unconditionally treated as a layer-set change and takes
                // the full-repaint path once, seeding `layers` for every subsequent frame.
                prev_layer_count: usize::MAX,
                prev_offset: false,
                #[cfg(feature = "tilesets")]
                warned_oversized: BTreeSet::new(),
                #[cfg(feature = "tilesets")]
                warned_dropped_tint: BTreeSet::new(),
                warned_notdef: BTreeSet::new(),
            },
            #[cfg(feature = "tilesets")]
            sprite_cache,
        }
    }

    /// The rendered pixel buffer, row-major, one `u32` per physical pixel.
    ///
    /// Each pixel is `0x00RRGGBB`: eight bits per channel with the top byte unused (not an alpha
    /// channel, and not premultiplied). Index a pixel as `y * width + x`, where `width = cols *
    /// glyph_width * scale` and the buffer length is `width * rows * glyph_height * scale`. Both
    /// dimensions come from this renderer's
    /// [`CellGeometry`], so prefer deriving them from
    /// [`surface_size`](CellGeometry::surface_size) over recomputing the
    /// product by hand.
    ///
    /// The contents are whatever the last draw call left behind: [`Output::draw_layers`] writes
    /// directly into this buffer rather than through an intermediate frame, so calling this
    /// between draw calls (before the frame is complete) yields a partially drawn buffer rather
    /// than an error. A [`resize`](Output::resize) reallocates the buffer, so the returned
    /// slice's contents and length are only valid until the next resize.
    ///
    /// This is always available: there is no `Option` wrapper because
    /// `SoftwareRenderer` is guaranteed to have an active rendering context.
    #[must_use]
    pub fn pixels(&self) -> &[u32] {
        self.ctx.pixel_buf.as_ref()
    }

    /// Pushes an event into the internal buffer, to be drained by
    /// [`Input::poll_event`].
    ///
    /// Coalesces consecutive `Mouse(Moved)` or same-button `Mouse(Drag)` events: winit can
    /// deliver `CursorMoved`/drag motion at device polling rate (hundreds/sec) though only the
    /// latest position matters once the next frame polls the queue, so this replaces the queue's
    /// tail in place instead of growing it unbounded (retroglyph#294, retroglyph#768), matching
    /// `retroglyph-window`'s `WindowBackend::push_event`. Every other event kind (clicks,
    /// scrolls, keys, resize, ...) still pushes in O(1) as before; only a back-to-back `Moved` or
    /// same-button `Drag` run collapses. See [`coalesces_with`] for the shared rule.
    pub fn push_event(&mut self, event: Event) {
        if let Some(back) = self.ctx.event_buffer.back_mut()
            && coalesces_with(&event, back)
        {
            *back = event;
            return;
        }
        self.ctx.event_buffer.push_back(event);
    }

    /// Initializes the window surface from a raw window/display handle.
    ///
    /// The concrete surface is platform-specific (softbuffer on native, a
    /// `Canvas2D` context on wasm32); see the `surface` module.
    ///
    /// # Errors
    ///
    /// On native, returns `SurfaceError::Context`/`SurfaceError::Surface` if softbuffer
    /// cannot create a graphics context or surface from `window` (for example, the handle's
    /// display or window system connection is invalid). On wasm32, returns
    /// `SurfaceError::Canvas` if winit's `<canvas>` element or its 2D rendering context
    /// cannot be located in the DOM. Either way, `self` is left without a surface, so
    /// [`present`](Self::present) keeps behaving as headless (a no-op) until `init_surface`
    /// is called again successfully.
    pub fn init_surface(&mut self, window: Arc<dyn WindowHandle>) -> Result<(), SurfaceError> {
        self.ctx.window_surface = Some(WindowSurface::new(window)?);
        Ok(())
    }

    /// Resizes the window surface to `width` x `height` pixels. No-op if the
    /// surface has not been initialized via [`init_surface`](Self::init_surface).
    pub fn resize_surface(&mut self, width: u32, height: u32) {
        if let Some(surf) = &mut self.ctx.window_surface {
            surf.resize(width, height);
        }
    }

    /// Presents the pixel buffer to the window surface. No-op in headless
    /// mode (no surface initialized).
    ///
    /// # Errors
    ///
    /// On native, returns `SurfaceError::Surface` if softbuffer cannot acquire or present
    /// its buffer (for example, the window was destroyed or the platform surface was lost).
    /// On wasm32, returns `SurfaceError::Canvas` if building the damaged-row `ImageData` or
    /// the canvas 2D context's `put_image_data` call fails. The pixel buffer and damage
    /// tracking are unaffected by a failed present, so the next successful present resends
    /// the current frame rather than a stale one.
    pub fn present(&mut self) -> Result<(), SurfaceError> {
        let Some(surface) = self.ctx.window_surface.as_mut() else {
            return Ok(()); // headless mode, nothing to present
        };
        // No damage since the last present: nothing changed, so skip the copy
        // + upload round trip entirely.
        let Some(damage) = self.ctx.damage_rows else {
            return Ok(());
        };
        let result = surface.present(self.ctx.pixel_buf.as_ref(), damage);
        // Only drop the damage once it's actually been presented, so a later
        // present() with no new draw_layers() call is a no-op instead of
        // re-presenting stale damage. On failure, leave it set so the next
        // present() attempt retries the same band instead of losing it.
        if result.is_ok() {
            self.ctx.damage_rows = None;
        }
        result
    }

    /// Diffs `pixel_buf` against `prev_pixels` row by row to find the
    /// smallest contiguous `[y0, y1)` band that changed and unions it into
    /// `damage_rows`.
    ///
    /// The newly diffed band is unioned with (not overwritten onto) any
    /// existing `damage_rows`, since `draw_layers` can be called more than
    /// once between two `present()` calls: `present()` is the only place
    /// that clears `damage_rows`, so a band from an earlier `draw_layers`
    /// call in the same present cycle must survive a later call that
    /// touches a disjoint set of rows, or those earlier rows would never
    /// reach the window surface (see retroglyph#724).
    ///
    /// Only the `[y0, y1)` band (not the whole buffer) is copied from
    /// `pixel_buf` into `prev_pixels` afterwards, since every other row is
    /// already known to match; when nothing changed, the copy is skipped
    /// entirely. `draw_layers` always repaints every cell (see
    /// [`Compositing::PixelLayered`](crate::backend::Compositing::PixelLayered)), so `prev_pixels`
    /// still has to hold a full previous-frame pixel buffer to diff against: this only removes
    /// the copy's cost from being proportional to the whole buffer instead of
    /// the changed region, which is what actually dominates this function's
    /// cost on an unchanged or near-unchanged frame.
    ///
    /// If the buffers differ in length (a resize raced with this call)
    /// the whole frame is marked damaged and `prev_pixels` is resized to
    /// match; that already covers any previously pending band, so it's an
    /// overwrite rather than a union.
    fn update_damage(&mut self, buf_w: usize) {
        let pixels = self.ctx.pixel_buf.as_ref();
        if self.ctx.prev_pixels.len() != pixels.len() {
            self.ctx.prev_pixels.clear();
            self.ctx.prev_pixels.extend_from_slice(pixels);
            let rows = pixels.len().checked_div(buf_w).unwrap_or(0);
            #[allow(clippy::cast_possible_truncation)]
            let rows_u32 = rows as u32;
            self.ctx.damage_rows = if rows == 0 { None } else { Some((0, rows_u32)) };
            return;
        }

        if buf_w == 0 {
            self.ctx.damage_rows = None;
            return;
        }

        let rows = pixels.len() / buf_w;
        let mut y0 = None;
        let mut y1 = 0usize;
        for row in 0..rows {
            let start = row * buf_w;
            let end = start + buf_w;
            if pixels[start..end] != self.ctx.prev_pixels[start..end] {
                if y0.is_none() {
                    y0 = Some(row);
                }
                y1 = row + 1;
            }
        }

        let new_band = y0.map(|y0| {
            #[allow(clippy::cast_possible_truncation)]
            (y0 as u32, y1 as u32)
        });
        self.ctx.damage_rows = match (self.ctx.damage_rows, new_band) {
            (Some((py0, py1)), Some((ny0, ny1))) => Some((py0.min(ny0), py1.max(ny1))),
            (existing, None) => existing,
            (None, new) => new,
        };

        // Only the changed band needs copying: every row outside `[y0, y1)`
        // already matched `pixels` in the loop above, so re-copying it would
        // just repeat work for no effect. When `y0` is `None` (nothing
        // changed), skip the copy entirely.
        if let Some(y0) = y0 {
            let start = y0 * buf_w;
            let end = y1 * buf_w;
            self.ctx.prev_pixels[start..end].copy_from_slice(&pixels[start..end]);
        }
    }

    /// Whether `glyph` resolves to a registered sprite in the `tilesets` sprite cache.
    ///
    /// Sprites carry their own per-pixel alpha, so a tile that dispatches to one does not fit
    /// [`resolve_bg_fill`]'s "an occupied tile is opaque" rule: see its doc comment. Without the
    /// `tilesets` feature there is no sprite cache at all, so this always returns `false`.
    #[cfg(feature = "tilesets")]
    fn has_sprite(&self, glyph: char) -> bool {
        self.sprite_cache.get(glyph).is_some()
    }

    // Signature has to match the `tilesets` arm above (both are called uniformly as
    // `self.has_sprite(glyph)`), so `_glyph` and `self` stay unused here rather than becoming a
    // `const fn` associated function: see retroglyph#954.
    #[cfg(not(feature = "tilesets"))]
    #[allow(clippy::unused_self, clippy::missing_const_for_fn)]
    fn has_sprite(&self, _glyph: char) -> bool {
        false
    }

    /// Grows `layers` to cover `layer_idx` if this is the first time it has been seen, or
    /// replaces that layer's [`LayerShadow`] wholesale if a previous grid size left it stale.
    /// Each replacement uses [`LayerShadow::new`], never a resize-in-place: a stale layer's
    /// content is never meaningful at the new dimensions, so there is nothing worth preserving
    /// (a `LayerShadow`'s own width/height can't independently drift from its contents; see
    /// retroglyph#567).
    fn ensure_layer_shadow(&mut self, layer_idx: usize, cols: usize, rows: usize) {
        if layer_idx >= self.ctx.layers.len() {
            self.ctx
                .layers
                .resize_with(layer_idx + 1, || LayerShadow::new(cols, rows));
        } else if !self.ctx.layers[layer_idx].fits(cols, rows) {
            self.ctx.layers[layer_idx] = LayerShadow::new(cols, rows);
        }
    }

    /// Determines the background `layer_id`'s cell at flat index `idx` should paint, if any.
    ///
    /// Wraps [`resolve_bg_fill`] with the one thing that function cannot see on its own: whether
    /// this cell is inside a multi-cell span whose *anchor* dispatches to a sprite. A covered
    /// cell holds the span's text fallback glyph (`'='`, `'['`, ...), which has no sprite of its
    /// own, so asking [`resolve_bg_fill`] about that glyph would make the covered cells paint an
    /// opaque background while the anchor cell stays transparent: one sprite, drawn over two
    /// different backdrops. Resolving the sprite question against the anchor keeps the whole
    /// footprint consistent.
    ///
    /// The *position* stays this cell's own, so background inheritance from lower layers is still
    /// resolved per cell rather than smeared from the anchor's column.
    fn resolve_cell_bg(&self, layer_id: u8, idx: usize, cols: usize) -> Option<u32> {
        let tile = self.ctx.layers[usize::from(layer_id)].tiles.as_ref()[idx];
        let anchor_glyph = tile.span_anchor_index(idx, cols).map_or_else(
            || tile.glyph(),
            |anchor_idx| {
                self.ctx.layers[usize::from(layer_id)]
                    .tiles
                    .as_ref()
                    .get(anchor_idx)
                    .map_or_else(|| tile.glyph(), Tile::glyph)
            },
        );
        let has_sprite = self.has_sprite(anchor_glyph);
        resolve_bg_fill(&self.ctx.layers, layer_id, idx, has_sprite)
    }

    /// Fills a cell's background rectangle when `bg_fill` is opaque. The rectangle is always the
    /// full, unshifted cell: sub-cell `dx`/`dy` offsets move only the glyph, never the background.
    fn fill_cell_bg(&mut self, cell_w: usize, cell_h: usize, pos: Pos, bg_fill: Option<u32>) {
        if let Some(bg) = bg_fill {
            let cell = ixy::Rect::new(usize::from(pos.x), usize::from(pos.y), 1, 1);
            let rect = cell * ixy::Size::new(cell_w, cell_h);
            self.ctx.pixel_buf.fill_rect_solid(rect, bg);
        }
    }

    /// Blits a cell's glyph (a cached sprite if one matches, else the bitmap font), shifted by the
    /// tile's sub-cell `dx`/`dy` offset. The glyph may spill past the cell edge into neighbors, so
    /// callers that want that spill preserved must lay down every background first (see the
    /// two-pass repaint in [`draw_layers`](Self::draw_layers)).
    ///
    /// A sprite is additionally shifted by its alignment inside the tile's span box (see
    /// [`Sprite::align_offset`]), which is `(0, 0)` unless the span reserves more cells than the
    /// artwork fills.
    #[allow(clippy::too_many_arguments)]
    fn blit_cell_glyph(
        &mut self,
        buf_w: usize,
        cell_w: usize,
        cell_h: usize,
        scale: usize,
        pos: Pos,
        tile: Tile,
        // Only ever read inside the `tilesets`-gated sprite path below: a bitmap-font glyph is
        // always drawn in the cell's own foreground color, never tinted (tints apply to
        // sprites only, per `Surface::with_tint`), so a `tilesets`-off build has no use for it.
        tint: Tint,
    ) {
        #[cfg(not(feature = "tilesets"))]
        let _ = tint;

        // A span-covered or blank cell draws no art at all (see `cell_art_glyph`): neither a
        // sprite nor a bitmap-font glyph. Deciding that once here, before either lookup, is the
        // fix for retroglyph#762: the sprite lookup below used to run unconditionally, so a
        // covered cell whose text-fallback glyph happened to have its own sprite painted that
        // sprite over the span's artwork instead of drawing nothing.
        let Some(art_glyph) = cell_art_glyph(&tile) else {
            return;
        };

        let px_x = usize::from(pos.x) * cell_w;
        let px_y = usize::from(pos.y) * cell_h;

        // Sprite cache dispatch: sprite wins over bitmap font.
        #[cfg(feature = "tilesets")]
        {
            let buf_h = self.ctx.pixel_buf.as_ref().len() / buf_w;
            let (glyph_w, glyph_h) = (self.ctx.geometry.glyph_w, self.ctx.geometry.glyph_h);
            if let Some(sprite) = self.sprite_cache.get(art_glyph) {
                let (span_w, span_h) = tile.span();
                let align = sprite.align_offset(span_w, span_h, glyph_w, glyph_h);
                let recolor =
                    SpriteTint::resolve(sprite.color, tile.style().foreground(), tint, DEFAULT_FG);
                blit_sprite(
                    self.ctx.pixel_buf.as_mut(),
                    buf_w,
                    buf_h,
                    px_x,
                    px_y,
                    tile.dx() + align.0,
                    tile.dy() + align.1,
                    sprite,
                    scale,
                    recolor,
                );
                if !tile.is_span_anchor() {
                    warn_sprite_needs_span(
                        &mut self.ctx.warned_oversized,
                        art_glyph,
                        (sprite.pixel_width, sprite.pixel_height),
                        (u32::from(glyph_w), u32::from(glyph_h)),
                    );
                }
                return;
            }
            // No sprite for this glyph: it falls back to the bitmap font below, which is
            // `fg`-colored, so a tint that would otherwise recolor a sprite silently has no
            // effect here (retroglyph#564, #537's exact trap).
            warn_tint_needs_sprite(&mut self.ctx.warned_dropped_tint, art_glyph, tint);
        }

        blit_glyph(
            self.ctx.pixel_buf.as_mut(),
            buf_w,
            px_x,
            px_y,
            &tile,
            art_glyph,
            &self.fonts,
            scale,
            &mut self.ctx.warned_notdef,
        );
    }
}

// ── Renderer construction ────────────────────────────────────────────────────────────

impl SoftwareBackend {
    /// Builds a [`SoftwareRenderer`] from this configuration.
    ///
    /// This does not block: it returns a [`SoftwareRenderer`] immediately.
    /// The renderer's pixel buffer can be inspected via
    /// [`SoftwareRenderer::pixels`] for headless / pixel-level use, or the renderer can be
    /// handed to `retroglyph_window::winit::run_windowed` to drive a window. Flushing
    /// is a no-op (the buffer stays in memory).
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::backend::Output;
    /// use retroglyph_core::tile::Tile;
    /// use retroglyph_core::color::Style;
    /// use retroglyph_core::grid::Pos;
    /// use retroglyph_core::backend::DrawCell;
    /// use retroglyph_core::color::Color;
    /// use retroglyph_software::config::SoftwareBackendBuilder;
    ///
    /// let mut renderer = SoftwareBackendBuilder::new()
    ///     .grid_size(1, 1)
    ///     .scale(1)
    ///     .build()
    ///     .unwrap()
    ///     .into_renderer()
    ///     .unwrap();
    ///
    /// // Render a red cell on layer 0.
    /// let tile = Tile::new(' ', Style::new().bg(Color::rgb(255, 0, 0)));
    /// renderer
    ///     .draw_layers([DrawCell::on_layer(0, Pos::new(0, 0), &tile)].into_iter())
    ///     .unwrap();
    ///
    /// assert!(renderer.pixels().iter().all(|&p| p == 0x00FF_0000));
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`SoftwareBackendError::NoFont`] if no font is set, or
    /// [`SoftwareBackendError::MixedGlyphSizes`] if the font chain's fonts disagree on their
    /// glyph size (both only reachable if
    /// [`SoftwareBackendBuilder::build`](config::SoftwareBackendBuilder::build) was bypassed),
    /// [`SoftwareBackendError::ZeroScale`] if `scale` is `0` (likewise only reachable if `build`
    /// was bypassed, since a caller mutated the field after construction),
    /// [`SoftwareBackendError::ZeroGrid`] if `cols` or `rows` is `0`, and
    /// [`SoftwareBackendError::Tileset`] if a registered tileset fails to load.
    ///
    /// # Panics
    ///
    /// Panics only on a `u32`-to-`usize` conversion that cannot fail on any target
    /// this crate supports (`usize` is at least 32 bits on every 32- and 64-bit
    /// platform), so this is not reachable in practice.
    pub fn into_renderer(self) -> Result<SoftwareRenderer, SoftwareBackendError> {
        let Some(fonts) = self.fonts else {
            return Err(SoftwareBackendError::NoFont);
        };
        let Some((glyph_w, glyph_h)) = fonts.glyph_size() else {
            return Err(SoftwareBackendError::MixedGlyphSizes);
        };
        if self.scale == 0 {
            return Err(SoftwareBackendError::ZeroScale);
        }
        if self.cols == 0 || self.rows == 0 {
            return Err(SoftwareBackendError::ZeroGrid);
        }

        let geometry = CellGeometry::new(glyph_w, glyph_h, self.scale);
        let (buf_w, buf_h) = geometry.surface_size(self.cols, self.rows);
        // u32 always fits in usize (all targets: 32- and 64-bit).
        let buf_w = usize::try_from(buf_w).expect("surface width fits usize");
        let buf_h = usize::try_from(buf_h).expect("surface height fits usize");

        #[cfg(feature = "tilesets")]
        let sprite_cache = if self.tilesets.is_empty() {
            Arc::new(SpriteCache::new())
        } else {
            Arc::new(
                SpriteCache::from_tilesets(&self.tilesets)
                    .map_err(SoftwareBackendError::Tileset)?,
            )
        };

        Ok(SoftwareRenderer::create(
            self,
            fonts,
            buf_w,
            buf_h,
            geometry,
            #[cfg(feature = "tilesets")]
            sprite_cache,
        ))
    }
}

// ── Output impl ─────────────────────────────────────────────────────────────────

impl Output for SoftwareRenderer {
    type Error = core::convert::Infallible;

    // No `draw` override: this backend always composites (`compositing` returns `PixelLayered`
    // below), so `Terminal::present` never calls single-layer `draw` and the default
    // implementation (forwards to `draw_layers`) is exactly right. See retroglyph#561.

    /// Composite the raw layer stream into the pixel buffer.
    ///
    /// Layers arrive layer-major (0 first), so painting them in order gives the
    /// correct z-order. Layer 0 always fills its cell background; a higher layer's
    /// occupied (non-empty) tile always fills a background too, and an empty tile
    /// never does: see the private `resolve_bg_fill` helper for the exact color each of those
    /// cases paints (it is not always the tile's own background, to mirror
    /// `Grid::flatten_into`'s background-inheritance rule exactly). The `is_empty`
    /// guard matters because this receives the full frame (see
    /// [`Compositing::PixelLayered`]), including empty
    /// higher-layer cells that must not overwrite layer 0.
    ///
    /// This matches cell backends (retroglyph#304): an occupied space with a
    /// [`Color::Default`] background on a higher layer erases the glyph beneath it
    /// when flattened, and this backend now does too, by repainting that cell's
    /// background (see the private `resolve_bg_fill` helper) even though the occupied tile's own
    /// background is the default one.
    ///
    /// A [`TileFlags::SPAN_COVERED`](retroglyph_core::tile::TileFlags::SPAN_COVERED) cell (retroglyph#412) paints its background but not its
    /// glyph: the span's anchor already drew one sprite across the whole footprint, and the
    /// covered cell's glyph is that sprite's text fallback, for backends that cannot draw it. The
    /// sprite-transparency rule that decides whether a background is painted at all is resolved
    /// against the *anchor* (see `resolve_cell_bg`), so one span never sits on two different
    /// backdrops.
    fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = DrawCell<'a>>,
    {
        let cols = usize::from(self.options.cols);
        let rows = usize::from(self.options.rows);
        let scale = usize::from(self.options.scale);
        let cell_w = usize::from(self.ctx.geometry.glyph_w) * scale;
        let cell_h = usize::from(self.ctx.geometry.glyph_h) * scale;
        let buf_w = cols * cell_w;
        let cell_count = cols * rows;

        // `compositing()` always returns `needs_full_frame: true` for this backend, so this
        // receives every cell on every allocated layer on every call: `Terminal::present`'s
        // diff-only path (used when `needs_full_frame` is `false`) never applies here, and
        // changing that would
        // be a `retroglyph-core` API change (retroglyph#302). Instead, this method keeps its own
        // per-cell shadow copy of the last frame's tiles (`RenderContext::layers`) and diffs
        // incoming cells against it below, entirely internally: cells whose tile is unchanged
        // since the last call are skipped instead of being cleared and repainted.
        if self.ctx.dirty_mask.len() == cell_count {
            self.ctx.dirty_mask.iter_mut().for_each(|d| *d = false);
        } else {
            self.ctx.dirty_mask.clear();
            self.ctx.dirty_mask.resize(cell_count, false);
        }

        let mut any_offset = false;
        let mut any_dirty = false;
        let mut max_layer_seen: i32 = -1;

        for draw_cell in content {
            let (layer_id, pos, tile) = (draw_cell.layer, draw_cell.pos, draw_cell.tile);
            // Silently drop cells positioned outside the grid, the same as `Headless`'s
            // `put_tile` (which bounds-checks internally): a caller-supplied `pos` is not
            // trusted input, and indexing it unchecked below would panic instead.
            if usize::from(pos.x) >= cols || usize::from(pos.y) >= rows {
                continue;
            }
            let layer_idx = usize::from(layer_id);
            max_layer_seen = max_layer_seen.max(i32::from(layer_id));
            self.ensure_layer_shadow(layer_idx, cols, rows);

            let idx = usize::from(pos.y) * cols + usize::from(pos.x);
            let shadow = &mut self.ctx.layers[layer_idx];
            let slot = &mut shadow.tiles.as_mut()[idx];
            let tint_slot = &mut shadow.tints.as_mut()[idx];
            if *slot != *tile || *tint_slot != draw_cell.tint {
                // `dirty_mask` is a single array shared across layers, not one per layer: marking
                // an index dirty here forces every layer to repaint that cell below, even ones
                // unchanged at this position, because a lower layer's background fill covers the
                // whole cell rect and would otherwise erase an unchanged higher layer's
                // already-composited glyph pixels on top of it.
                self.ctx.dirty_mask[idx] = true;
                any_dirty = true;
                *slot = *tile;
                *tint_slot = draw_cell.tint;
            }
            if tile.dx() != 0 || tile.dy() != 0 {
                any_offset = true;
            }
        }

        if any_dirty {
            // Runs after the whole stream, so every layer's shadow copy is current: a span's
            // footprint has to be read off an anchor this frame actually wrote. A covered cell's
            // tile does not change when only the anchor's artwork does, so a dirty cell anywhere
            // in a span has to dirty the whole span here; without that, the previous sprite's
            // pixels would survive in cells the diff considers unchanged.
            for layer in &self.ctx.layers {
                expand_dirty_spans(&mut self.ctx.dirty_mask, layer.tiles.as_ref(), cols, rows);
            }
        }

        #[allow(clippy::cast_sign_loss)]
        let layer_count_now = (max_layer_seen + 1) as usize;
        let layers_changed = layer_count_now != self.ctx.prev_layer_count;
        self.ctx.prev_layer_count = layer_count_now;

        // Falls back to a full clear-and-repaint of every cell when either:
        // - any tile this frame or the last one has a nonzero sub-cell offset (`Tile::dx`/`dy`):
        //   offsets can spill glyph pixels into neighboring cells by an amount `Tile` does not
        //   bound, so containing the repaint to a neighborhood around the changed cells isn't
        //   possible without a magnitude cap core doesn't provide. The previous frame's offsets
        //   matter just as much as this frame's: a frame that removes the last offset can leave a
        //   neighbor cell's own tile byte-identical to last frame, so the dirty-cell path never
        //   revisits it to clear the spill that landed there, exactly like `layers_changed` below
        //   already compares against last frame's state instead of only this frame's; or
        // - the number of allocated layers changed since the last call: a layer's cells falling
        //   out of (or into) the frame can't be diffed against a shadow copy that no longer
        //   describes this frame's layer set.
        let full_repaint = any_offset || self.ctx.prev_offset || layers_changed;
        self.ctx.prev_offset = any_offset;

        if full_repaint {
            self.ctx.pixel_buf.clear();
            for layer_id in 0..layer_count_now {
                #[allow(clippy::cast_possible_truncation)]
                let layer_id = layer_id as u8;
                // Pass 1: lay down every cell's background on this layer first.
                for idx in 0..cell_count {
                    let bg_fill = self.resolve_cell_bg(layer_id, idx, cols);
                    let (x, y) = flat_index_to_xy(idx, cols);
                    let pos = Pos::new(x, y);
                    self.fill_cell_bg(cell_w, cell_h, pos, bg_fill);
                }
                // Pass 2: blit every cell's glyph over those backgrounds. A glyph offset past its
                // right/bottom edge now spills onto the neighbor's already-painted background
                // instead of being clobbered by that neighbor's later background fill, so spill is
                // uniform in all four directions: the two-pass mechanism of the sub-cell
                // offset/spill contract on `retroglyph_window::presenter::Presenter` (see its rustdoc).
                for idx in 0..cell_count {
                    let tile = self.ctx.layers[layer_id as usize].tiles.as_ref()[idx];
                    let tint = self.ctx.layers[layer_id as usize].tints.as_ref()[idx];
                    let (x, y) = flat_index_to_xy(idx, cols);
                    let pos = Pos::new(x, y);
                    self.blit_cell_glyph(buf_w, cell_w, cell_h, scale, pos, tile, tint);
                }
            }
        } else if any_dirty {
            // The same two-pass split as the full repaint, restricted to the dirty cells: every
            // dirty cell's background on a layer goes down before any of that layer's glyphs, so
            // artwork that spills out of its own cell (a multi-cell span's sprite, or a glyph
            // pushed out by a sub-cell offset) is not erased by the neighbour's background fill
            // arriving after it.
            for layer_id in 0..layer_count_now {
                #[allow(clippy::cast_possible_truncation)]
                let layer_id = layer_id as u8;
                for idx in 0..cell_count {
                    if !self.ctx.dirty_mask[idx] {
                        continue;
                    }
                    let bg_fill = self.resolve_cell_bg(layer_id, idx, cols);
                    let (x, y) = flat_index_to_xy(idx, cols);
                    let pos = Pos::new(x, y);
                    self.fill_cell_bg(cell_w, cell_h, pos, bg_fill);
                }
                for idx in 0..cell_count {
                    if !self.ctx.dirty_mask[idx] {
                        continue;
                    }
                    let tile = self.ctx.layers[usize::from(layer_id)].tiles.as_ref()[idx];
                    let tint = self.ctx.layers[usize::from(layer_id)].tints.as_ref()[idx];
                    let (x, y) = flat_index_to_xy(idx, cols);
                    let pos = Pos::new(x, y);
                    self.blit_cell_glyph(buf_w, cell_w, cell_h, scale, pos, tile, tint);
                }
            }
        }

        self.update_damage(buf_w);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        // No-op. Frame is presented via WindowedBackend::present() in windowed
        // mode, or accessed directly via pixels() in headless/testing mode.
        Ok(())
    }

    fn size(&self) -> Size {
        Size::new(self.options.cols, self.options.rows)
    }

    fn resize(&mut self, size: Size) {
        self.options.cols = size.width();
        self.options.rows = size.height();
        // Cell size is constant (glyph x scale); only the surface size changes with the grid.
        let (new_w, new_h) = self.ctx.geometry.surface_size(size.width(), size.height());
        let new_w = usize::try_from(new_w).expect("surface width fits usize");
        let new_h = usize::try_from(new_h).expect("surface height fits usize");
        self.ctx.pixel_buf.resize(new_w, new_h);
        // Buffer dimensions changed: the shadow buffer no longer matches,
        // so drop it and force a full-frame damage rect on the next present.
        self.ctx.prev_pixels.clear();
        self.ctx.prev_pixels.resize(new_w * new_h, 0);
        // The per-cell shadow is keyed by the old grid dimensions; drop every layer's
        // `LayerShadow` entirely rather than resizing them in place (`ensure_layer_shadow` lazily
        // rebuilds each one at the new dimensions on the next `draw_layers` call), so the next
        // call can't misread stale entries against the new layout, and force that call onto the
        // full-repaint path.
        self.ctx.invalidate_shadow();
        self.ctx.damage_rows = if new_h == 0 {
            None
        } else {
            #[allow(clippy::cast_possible_truncation)]
            Some((0, new_h as u32))
        };
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.ctx.pixel_buf.clear();
        // The per-cell shadow is now stale versus what's actually on screen (blank); forget it,
        // mirroring `resize` above, so the next `draw_layers` call can't diff against pre-clear
        // state and takes the full-repaint path instead of painting nothing.
        self.ctx.invalidate_shadow();
        Ok(())
    }

    fn compositing(&self) -> Compositing {
        Compositing::PixelLayered {
            needs_full_frame: true,
        }
    }
}

// ── Input impl ───────────────────────────────────────────────────────────────────

impl Input for SoftwareRenderer {
    fn poll_event(&mut self, _timeout: Duration) -> Option<Event> {
        // Non-blocking: the game loop is driven by `about_to_wait` →
        // `request_redraw`, so there is no background thread to sleep on.
        // All backends return immediately regardless of platform.
        self.ctx.event_buffer.pop_front()
    }

    fn push_event(&mut self, event: Event) {
        Self::push_event(self, event);
    }
}

// ── Cursor impl ──────────────────────────────────────────────────────────────────

impl Cursor for SoftwareRenderer {
    fn set_cursor_visible(&mut self, _visible: bool) {
        // No hardware cursor in software mode.
    }

    fn set_cursor_position(&mut self, _position: Pos) {
        // No hardware cursor in software mode.
    }
}

// ── Presenter impl ───────────────────────────────────────────────────────────

// `SoftwareRenderer`'s own `Output` impl above already satisfies `Presenter: Output`, so this
// only needs the surface lifecycle: no forwarding/duplication of draw/flush/size/clear/resize.
impl retroglyph_window::presenter::Presenter for SoftwareRenderer {
    type SurfaceError = SurfaceError;

    fn init_surface(&mut self, window: Arc<dyn WindowHandle>) -> Result<(), SurfaceError> {
        Self::init_surface(self, window)
    }

    fn resize_surface(&mut self, width: u32, height: u32) {
        Self::resize_surface(self, width, height);
    }

    fn present(&mut self) -> Result<(), SurfaceError> {
        Self::present(self)
    }

    fn cell_size(&self) -> (u32, u32) {
        self.ctx.geometry.cell_size()
    }

    fn geometry(&self) -> CellGeometry {
        self.ctx.geometry
    }
}

// ── Grid compositing ──────────────────────────────────────────────────────────

/// Paints the set ("on") pixels of glyph `glyph_index` in `font` into `buffer` as `color`, with
/// each source pixel scaled to a `scale x scale` destination block.
///
/// Set pixels come from [`BitmapFont::glyph_pixels`], the one place the 1-bit MSB-first bit layout
/// is decoded (shared with the GL atlas builder), so this backend and `retroglyph-gl` can't
/// disagree on which texels a glyph covers, and neither hardcodes an 8-pixel row.
///
/// The glyph's top-left destination corner is `(origin_x, origin_y)`
/// (already including any sub-cell `dx`/`dy` offset, scaled). When the whole
/// glyph's destination bounding box fits inside `buffer` (the overwhelmingly
/// common case, since it only fails for cells with a nonzero `dx`/`dy` that
/// pushes them past a buffer edge), this takes a fast path with no per-pixel
/// bounds check: it fills each `scale`-wide destination run in one slice
/// `fill` call. Otherwise it falls back to a row-clamped path that clips
/// each destination run to the buffer bounds once per row, rather than
/// checking every pixel.
/// Decodes a flat row-major cell index into `(x, y)`, given the grid's `cols`.
///
/// Delegates to [`RowMajor`]'s [`LinearLayout::index_to_pos`] instead of hand-rolling
/// `idx % cols` / `idx / cols` at each flat-buffer call site below.
fn flat_index_to_xy(idx: usize, cols: usize) -> (u16, u16) {
    let pos = RowMajor::index_to_pos(idx, cols);
    #[allow(clippy::cast_possible_truncation)]
    (pos.x as u16, pos.y as u16)
}

#[allow(clippy::too_many_arguments, clippy::cast_possible_truncation)]
fn blit_glyph_mask(
    buffer: &mut [u32],
    buf_w: usize,
    buf_h: usize,
    origin_x: i64,
    origin_y: i64,
    font: &BitmapFont,
    glyph_index: u8,
    scale: usize,
    color: u32,
) {
    let glyph_w = usize::from(font.glyph_width()) * scale;
    let glyph_h = usize::from(font.glyph_height()) * scale;

    #[allow(clippy::cast_sign_loss)]
    let in_bounds = origin_x >= 0
        && origin_y >= 0
        && origin_x as usize + glyph_w <= buf_w
        && origin_y as usize + glyph_h <= buf_h;

    if in_bounds {
        #[allow(clippy::cast_sign_loss)]
        let ox = origin_x as usize;
        #[allow(clippy::cast_sign_loss)]
        let oy = origin_y as usize;
        for (src_x, src_y) in font.glyph_pixels(glyph_index) {
            let x0 = ox + usize::from(src_x) * scale;
            let y0 = oy + usize::from(src_y) * scale;
            for sdy in 0..scale {
                let row_start = (y0 + sdy) * buf_w + x0;
                buffer[row_start..row_start + scale].fill(color);
            }
        }
        return;
    }

    #[allow(
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        clippy::similar_names
    )]
    for (src_x, src_y) in font.glyph_pixels(glyph_index) {
        for sdy in 0..scale {
            let y = origin_y + (usize::from(src_y) * scale + sdy) as i64;
            if y < 0 || y as usize >= buf_h {
                continue;
            }
            let y = y as usize;
            let x_start = origin_x + (usize::from(src_x) * scale) as i64;
            let x_end = x_start + scale as i64;
            let x0 = x_start.max(0);
            let x1 = x_end.min(buf_w as i64);
            if x0 >= x1 {
                continue;
            }
            let row_start = y * buf_w + x0 as usize;
            let row_end = y * buf_w + x1 as usize;
            buffer[row_start..row_end].fill(color);
        }
    }
}

/// Blits a glyph's set bits into `buffer` at `(px_x, px_y)` plus sub-cell
/// offset from `tile.dx`/`tile.dy`. Only the foreground (glyph) pixels are
/// painted; background is left untouched.
///
/// `art_glyph` is the caller-resolved [`cell_art_glyph`] answer for `tile`: whether this cell
/// draws at all (span-covered and blank cells are filtered before this is ever called) is decided
/// once by the caller, not re-derived here.
#[allow(clippy::cast_possible_truncation, clippy::too_many_arguments)]
fn blit_glyph(
    buffer: &mut [u32],
    buf_w: usize,
    px_x: usize,
    px_y: usize,
    tile: &Tile,
    art_glyph: char,
    fonts: &FontChain<'static>,
    scale: usize,
    warned_notdef: &mut BTreeSet<char>,
) {
    let fg = resolve_color(tile.style().foreground(), DEFAULT_FG);

    #[allow(clippy::cast_possible_wrap)]
    let origin_x = px_x as i64 + i64::from(tile.dx()) * scale as i64;
    #[allow(clippy::cast_possible_wrap)]
    let origin_y = px_y as i64 + i64::from(tile.dy()) * scale as i64;

    // Nothing in the chain can draw this character, not even a substitute box: leave the cell
    // at its background rather than pointing at a glyph index some font doesn't have.
    let Some(glyph) = fonts.resolve(art_glyph) else {
        return;
    };
    // A resolved glyph flagged `is_notdef` drew the fallback, not `art_glyph`'s own shape -- a
    // legitimate cell on its own (a solid block can be drawn on purpose), so this is the only
    // place a caller finds out it happened at all (retroglyph#1292).
    if glyph.is_notdef() {
        warn_notdef_glyph(warned_notdef, art_glyph);
    }
    let buf_h = buffer.len() / buf_w;

    blit_glyph_mask(
        buffer,
        buf_w,
        buf_h,
        origin_x,
        origin_y,
        &glyph.font(),
        glyph.index(),
        scale,
        fg,
    );
}

/// Blit a decoded RGBA8 sprite into `buffer` with alpha blending.
///
/// The sprite's top-left corner is at pixel `(cell_px_x + offset_x * scale,
/// cell_px_y + offset_y * scale)`, where the offset is in unscaled pixels and combines the
/// tile's own sub-cell `dx`/`dy` with the sprite's alignment inside its span box (see
/// [`Sprite::align_offset`]). A sprite larger than one cell extends past the anchor cell into
/// the cells its span covers.
///
/// Pixels outside `buffer` bounds are silently clipped.
///
/// Blending uses pure integer `U8x4Rgba::source_over`. Fully opaque pixels
/// (alpha == 255) skip blending entirely and write directly to the buffer.
///
/// That arithmetic runs on sRGB-encoded bytes rather than in linear light, which is deliberate:
/// pixel art is authored in editors that composite the same way, and this blit is the reference
/// the GPU backends are checked against pixel for pixel, so it defines the result for all three.
/// See `docs/references/core/color-space.md`.
#[cfg(feature = "tilesets")]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::too_many_arguments
)]
fn blit_sprite(
    buffer: &mut [u32],
    buf_w: usize,
    buf_h: usize,
    cell_px_x: usize,
    cell_px_y: usize,
    offset_x: i16,
    offset_y: i16,
    sprite: &Sprite,
    scale: usize,
    recolor: SpriteTint,
) {
    let origin_x = cell_px_x as i64 + i64::from(offset_x) * scale as i64;
    let origin_y = cell_px_y as i64 + i64::from(offset_y) * scale as i64;

    let src_w = sprite.pixel_width as usize;
    let src_h = sprite.pixel_height as usize;

    // Precompute once whether the sprite's whole destination bounding box
    // fits inside `buffer`: true for the overwhelmingly common case (no
    // sub-cell offset, sprite fully on-screen). When it does, the fast path
    // below skips the per-destination-pixel bounds check entirely; only
    // sprites clipped by a nonzero `dx`/`dy` or a screen edge fall back to
    // the clamped, per-row-checked slow path.
    let glyph_w = src_w * scale;
    let glyph_h = src_h * scale;
    let in_bounds = origin_x >= 0
        && origin_y >= 0
        && origin_x as usize + glyph_w <= buf_w
        && origin_y as usize + glyph_h <= buf_h;
    let identity = recolor.is_identity();

    for src_y in 0..src_h {
        for src_x in 0..src_w {
            let src_idx = (src_y * src_w + src_x) * 4;
            let src = U8x4Rgba::new(
                sprite.pixels[src_idx],
                sprite.pixels[src_idx + 1],
                sprite.pixels[src_idx + 2],
                sprite.pixels[src_idx + 3],
            );

            if src.is_transparent() {
                continue;
            }

            // `identity` is loop-invariant, hoisted above by the compiler: an untinted sprite
            // (the overwhelming majority) takes exactly the arithmetic it took before this
            // branch existed. Alpha is untouched, so which pixels are opaque, and
            // therefore the blending below and the background showing through, are unaffected.
            let src = if identity {
                src
            } else {
                let (r, g, b) = recolor.apply((src.r, src.g, src.b));
                U8x4Rgba::new(r, g, b, src.a)
            };

            // Fast path: fully opaque pixels write directly, no blending.
            // Most roguelike sprites are opaque, so this skips U8x4Rgba
            // construction + source_over for the common case.
            let rgb = src.to_rgb_u32();
            if src.alpha() == 255 {
                if in_bounds {
                    let x0 = origin_x as usize + src_x * scale;
                    let y0 = origin_y as usize + src_y * scale;
                    for dy in 0..scale {
                        let row_start = (y0 + dy) * buf_w + x0;
                        buffer[row_start..row_start + scale].fill(rgb);
                    }
                } else {
                    for dy in 0..scale {
                        let dst_y = origin_y + (src_y * scale + dy) as i64;
                        if dst_y < 0 || dst_y as usize >= buf_h {
                            continue;
                        }
                        let dst_y = dst_y as usize;
                        let x_start = origin_x + (src_x * scale) as i64;
                        let x_end = x_start + scale as i64;
                        let x0 = x_start.max(0);
                        let x1 = x_end.min(buf_w as i64);
                        if x0 >= x1 {
                            continue;
                        }
                        let row_start = dst_y * buf_w + x0 as usize;
                        let row_end = dst_y * buf_w + x1 as usize;
                        buffer[row_start..row_end].fill(rgb);
                    }
                }
                continue;
            }

            // Each source pixel maps to `scale x scale` destination pixels.
            if in_bounds {
                let x0 = origin_x as usize + src_x * scale;
                let y0 = origin_y as usize + src_y * scale;
                for dy in 0..scale {
                    let row = (y0 + dy) * buf_w;
                    for dx in 0..scale {
                        let dst_idx = row + x0 + dx;
                        let dst = U8x4Rgba::from_rgb_u32(buffer[dst_idx]);
                        let blended = src.source_over(dst);
                        buffer[dst_idx] = blended.to_rgb_u32();
                    }
                }
                continue;
            }

            for dy in 0..scale {
                let dst_y = origin_y + (src_y * scale + dy) as i64;
                if dst_y < 0 || dst_y as usize >= buf_h {
                    continue;
                }
                let dst_y = dst_y as usize;

                for dx in 0..scale {
                    let dst_x = origin_x + (src_x * scale + dx) as i64;
                    if dst_x < 0 || dst_x as usize >= buf_w {
                        continue;
                    }
                    let dst_x = dst_x as usize;

                    let dst_idx = dst_y * buf_w + dst_x;

                    let dst = U8x4Rgba::from_rgb_u32(buffer[dst_idx]);
                    let blended = src.source_over(dst);
                    buffer[dst_idx] = blended.to_rgb_u32();
                }
            }
        }
    }
}

/// Extends `dirty` so that whenever any cell of a multi-cell span is dirty, every cell of that
/// span is.
///
/// A span's artwork is drawn once, from its anchor, across the whole footprint, so repainting
/// part of one is never right: the anchor has to be redrawn, and every cell it paints over has to
/// have its background laid down again first. The diff that fills `dirty` cannot see this, because
/// a covered cell's tile holds only the fallback glyph and the offset back to the anchor: both
/// unchanged while the anchor's artwork changes underneath it.
///
/// Expansion always runs from the anchor outwards over its full declared footprint, so one pass
/// reaches every cell no matter which one of them was dirty to begin with. Cells past the grid
/// edge are skipped: a span cannot be written past the edge, but this reads a shadow buffer that
/// can lag a resize by a frame.
fn expand_dirty_spans(dirty: &mut [bool], layer: &[Tile], cols: usize, rows: usize) {
    if cols == 0 {
        return;
    }
    for idx in 0..layer.len().min(dirty.len()) {
        if !dirty[idx] {
            continue;
        }
        let tile = layer[idx];
        let anchor_idx = if tile.span_offset().is_some() {
            let Some(anchor_idx) = tile.span_anchor_index(idx, cols) else {
                continue;
            };
            anchor_idx
        } else if tile.is_span_anchor() {
            idx
        } else {
            continue;
        };
        let Some(anchor) = layer.get(anchor_idx) else {
            continue;
        };
        let (span_w, span_h) = anchor.span();
        let ixy_pos = RowMajor::index_to_pos(anchor_idx, cols);
        let (ax, ay) = (ixy_pos.x, ixy_pos.y);
        for row in ay..(ay + usize::from(span_h)).min(rows) {
            for col in ax..(ax + usize::from(span_w)).min(cols) {
                dirty[row * cols + col] = true;
            }
        }
    }
}

// The canonical `Color::Default` fallbacks live in `retroglyph-window`'s `palette` module so the
// gl and software backends can't drift; imported above as `(u8, u8, u8)` triples.

/// Determines the background this layer/tile should paint at `idx`, if any,
/// mirroring [`Grid::flatten_into`](retroglyph_core::grid::Grid)'s background-inheritance rule so
/// cell and pixel backends agree (retroglyph#304).
///
/// - Layer 0 always paints: its own background, substituting [`DEFAULT_BG`] for
///   [`Color::Default`].
/// - A higher layer's empty tile paints nothing (`None`): it doesn't contribute to the flattened
///   cell at all.
/// - A higher layer's occupied (non-empty) tile with a non-[`Color::Default`] background paints
///   that color.
/// - A higher layer's occupied tile with a [`Color::Default`] background still paints, *unless*
///   `has_sprite` is `true`: this is the fix for retroglyph#304, an occupied space is opaque and
///   erases whatever glyph a lower layer drew there, even though its own background is the
///   default one. What it paints with is *not* [`DEFAULT_BG`] though: matching `flatten_into`'s
///   `if tile.style.bg != Color::Default` guard, a `Color::Default` background never overwrites
///   the destination background, so this walks back down through the layers below `layer_id`
///   (down to and including layer 0) to find whichever one last established a background, and
///   repaints with that instead. `has_sprite` opts a tile out of this rule entirely: sprites
///   carry genuine per-pixel alpha (see [`SoftwareRenderer::has_sprite`]), so forcing an opaque
///   fill underneath one before it's blended would erase transparency the sprite's own pixels are
///   supposed to let show through: core's `Tile`/`Grid` model has no such per-pixel concept, so
///   the cell-backend-parity rule this function otherwise implements just doesn't apply to them.
fn resolve_bg_fill(
    layers: &[LayerShadow],
    layer_id: u8,
    idx: usize,
    has_sprite: bool,
) -> Option<u32> {
    let layer_idx = usize::from(layer_id);
    let tile = layers[layer_idx].tiles.as_ref()[idx];
    if layer_idx == 0 {
        return Some(resolve_color(tile.style().background(), DEFAULT_BG));
    }
    if tile.is_empty() {
        return None;
    }
    if tile.style().background() != Color::Default {
        return Some(resolve_color(tile.style().background(), DEFAULT_BG));
    }
    if has_sprite {
        return None;
    }
    for below in (0..layer_idx).rev() {
        let bg = layers[below].tiles.as_ref()[idx].style().background();
        if below == 0 || bg != Color::Default {
            return Some(resolve_color(bg, DEFAULT_BG));
        }
    }
    unreachable!("the loop above always terminates at `below == 0`, which always returns")
}

/// Resolve a [`Color`] to a packed `0x00RRGGBB` value, substituting `default` for
/// [`Color::Default`].
///
/// Delegates the actual palette resolution to `retroglyph-core`'s [`Color::resolve_rgb`], the
/// single canonical color-to-RGB path every graphical backend shares (so the CPU rasterizer and
/// `retroglyph-gl`'s GPU atlas agree on every pixel color); this only repacks core's `(r, g, b)`
/// triple into this backend's `0x00RRGGBB` `u32` pixel format. The `default` fallback is a triple
/// too (from [`retroglyph_window::palette`]), so no unpack step is needed.
fn resolve_color(color: Color, default: (u8, u8, u8)) -> u32 {
    let (r, g, b) = color.resolve_rgb(default);
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use config::SoftwareBackendBuilder;
    use retroglyph_core::color::Color;
    use retroglyph_core::color::Style;
    use retroglyph_core::grid::{Pos, Size};

    fn test_renderer() -> SoftwareRenderer {
        SoftwareBackendBuilder::new()
            .font(retroglyph_window::font::unscii16::FONT)
            .grid_size(1, 1)
            .scale(1)
            .build()
            .unwrap()
            .into_renderer()
            .unwrap()
    }

    #[test]
    fn presenter_geometry_and_cell_size_match_the_internal_geometry() {
        use retroglyph_window::presenter::Presenter as _;

        // `test_renderer` builds an 8x16 unscii16 font at scale 1.
        let renderer = test_renderer();
        assert_eq!(renderer.cell_size(), (8, 16));
        assert_eq!(renderer.geometry(), CellGeometry::new(8, 16, 1));
    }

    #[test]
    fn compositing_requests_pixel_layered_full_frames() {
        // Pins `Output::compositing`'s return value directly: sub-cell offsets can spill glyph
        // pixels into neighboring cells, so this backend always needs the full frame redrawn (see
        // `compositing`'s doc).
        let renderer = test_renderer();
        assert_eq!(
            renderer.compositing(),
            Compositing::PixelLayered {
                needs_full_frame: true
            }
        );
    }

    #[test]
    fn layer0_paints_background() {
        let mut renderer = test_renderer();
        let tile = Tile::new(' ', Style::new().bg(Color::rgb(255, 0, 0)));
        let diff: Vec<DrawCell<'_>> = vec![DrawCell::on_layer(0, Pos::new(0, 0), &tile)];
        renderer.draw_layers(diff.into_iter());

        let buf = renderer.pixels();
        assert_eq!(buf.len(), 8 * 16);
        assert!(
            buf.iter().all(|&p| p == 0x00FF_0000),
            "all pixels should be red"
        );
    }

    #[test]
    fn layer1_does_not_paint_background() {
        let mut renderer = test_renderer();

        let bg_tile = Tile::new(' ', Style::new().bg(Color::rgb(255, 0, 0)));
        let space_tile = Tile::new(' ', Style::new().fg(Color::rgb(0, 255, 0)));
        // draw_layers clears buffer first, so pass all layers in one call.
        renderer.draw_layers(
            [
                DrawCell::on_layer(0, Pos::new(0, 0), &bg_tile),
                DrawCell::on_layer(1, Pos::new(0, 0), &space_tile),
            ]
            .into_iter(),
        );

        let buf = renderer.pixels();
        assert_eq!(buf.len(), 8 * 16);
        // All pixels should be red (layer 0 bg).  Green fg from layer 1's
        // space tile is ignored because space has no set bits.
        assert!(
            buf.iter().all(|&p| p == 0x00FF_0000),
            "all pixels should be red, not green"
        );
    }

    #[test]
    fn layer1_glyph_overwrites_layer0() {
        let mut renderer = test_renderer();

        let bg = Tile::new(' ', Style::new().bg(Color::rgb(10, 10, 10)));
        let fg = Tile::new('@', Style::new().fg(Color::rgb(0, 255, 0)));
        // draw_layers clears buffer first, so pass all layers in one call.
        renderer.draw_layers(
            [
                DrawCell::on_layer(0, Pos::new(0, 0), &bg),
                DrawCell::on_layer(1, Pos::new(0, 0), &fg),
            ]
            .into_iter(),
        );

        let buf = renderer.pixels();
        assert!(buf.contains(&0x0000_FF00), "some pixels should be green");
        assert!(buf.iter().all(|&p| p == 0x0000_FF00 || p == 0x000A_0A0A));
    }

    #[test]
    fn layer1_occupied_default_bg_erases_layer0_glyph() {
        // retroglyph#304: an occupied (non-empty) higher-layer tile with a
        // `Color::Default` background is opaque and erases whatever glyph a lower
        // layer drew underneath it, matching cell backends' `Grid::flatten_into`
        // (see the crate README's "Backend parity" section). The erased cell's
        // background is inherited from layer 0 (red here), not reset to this
        // renderer's own default background.
        let mut renderer = test_renderer();

        let glyph_on_red = Tile::new(
            '@',
            Style::new()
                .fg(Color::rgb(0, 255, 0))
                .bg(Color::rgb(255, 0, 0)),
        );
        // Occupied (non-empty, via the `Tile::new` builder) space with no explicit
        // background: `Color::Default`.
        let occupied_default_bg = Tile::new(' ', Style::default());
        // draw_layers clears buffer first, so pass all layers in one call.
        renderer.draw_layers(
            [
                DrawCell::on_layer(0, Pos::new(0, 0), &glyph_on_red),
                DrawCell::on_layer(1, Pos::new(0, 0), &occupied_default_bg),
            ]
            .into_iter(),
        );

        let buf = renderer.pixels();
        assert!(
            buf.iter().all(|&p| p == 0x00FF_0000),
            "layer 1's occupied default-bg space should erase layer 0's glyph, leaving only \
             the inherited red background, but got: {buf:?}"
        );
    }

    #[test]
    fn sub_cell_offset_shifts_glyph() {
        let mut renderer = test_renderer();

        let bg = Tile::new(' ', Style::new().bg(Color::rgb(10, 10, 10)));
        let fg = Tile::new('@', Style::new().fg(Color::rgb(0, 255, 0))).with_offset(1, 0);
        // draw_layers clears buffer first, so pass all layers in one call.
        renderer.draw_layers(
            [
                DrawCell::on_layer(0, Pos::new(0, 0), &bg),
                DrawCell::on_layer(1, Pos::new(0, 0), &fg),
            ]
            .into_iter(),
        );

        let buf = renderer.pixels();
        let has_green = |col: usize| {
            buf.iter()
                .enumerate()
                .any(|(i, &p)| i % 8 == col && p == 0x0000_FF00)
        };
        assert!(!has_green(0), "x=0 should have no green pixels with dx=1");
        assert!(has_green(1), "x=1 should have green pixels with dx=1");
    }

    #[test]
    fn output_draw_respects_sub_cell_offset_via_draw_layers() {
        // `Output::draw` has no body of its own (retroglyph#561): it forwards to `draw_layers`,
        // tagged onto layer 0. This exercises that forwarding end to end rather than a
        // separately-maintained single-layer blit path, which is what used to make it possible
        // for the two to silently disagree.
        let mut renderer = test_renderer();

        let fg = Tile::new('@', Style::new().fg(Color::rgb(0, 255, 0))).with_offset(1, 0);
        Output::draw(
            &mut renderer,
            [DrawCell::new(Pos::new(0, 0), &fg)].into_iter(),
        )
        .unwrap();

        let buf = renderer.pixels();
        let has_green = |col: usize| {
            buf.iter()
                .enumerate()
                .any(|(i, &p)| i % 8 == col && p == 0x0000_FF00)
        };
        assert!(!has_green(0), "x=0 should have no green pixels with dx=1");
        assert!(has_green(1), "x=1 should have green pixels with dx=1");
    }

    #[test]
    fn draw_layers_ignores_a_cell_positioned_outside_the_grid() {
        // retroglyph#729: a `DrawCell` positioned outside `size()` used to index the shadow
        // buffer out of bounds and panic; `Headless` already silently drops such cells, so this
        // backend should too instead of disagreeing on the input.
        let mut renderer = SoftwareBackendBuilder::new()
            .font(retroglyph_window::font::unscii16::FONT)
            .grid_size(2, 2)
            .scale(1)
            .build()
            .unwrap()
            .into_renderer()
            .unwrap();

        let tile = Tile::new('X', Style::new());
        renderer
            .draw_layers(core::iter::once(DrawCell::on_layer(
                0,
                Pos::new(5, 5),
                &tile,
            )))
            .expect("out-of-range cells are silently dropped, not a panic");
    }

    #[test]
    fn draw_layers_spills_glyph_right_into_the_neighbor_cell() {
        // Regression guard for uniform spill: a glyph offset past its right edge must land on the
        // neighbor's already-painted background, not be clobbered by the neighbor's background
        // fill. Before the two-pass repaint, only left/up spill survived; right/down was lost.
        let mut renderer = SoftwareBackendBuilder::new()
            .font(retroglyph_window::font::unscii16::FONT)
            .grid_size(2, 1)
            .scale(1)
            .build()
            .unwrap()
            .into_renderer()
            .unwrap();

        // Full block (all 8x16 pixels set), green, shifted right by 4px (half a cell): its left
        // half stays in cell 0, its right half spills into cell 1.
        let block = Tile::new('\u{2588}', Style::new().fg(Color::rgb(0, 255, 0))).with_offset(4, 0);
        // Neighbor cell (1, 0): blank with an opaque blue background, the fill that used to erase
        // the spill.
        let neighbor = Tile::new(' ', Style::new().bg(Color::rgb(0, 0, 255)));

        renderer.draw_layers(
            [
                DrawCell::on_layer(0, Pos::new(0, 0), &block),
                DrawCell::on_layer(0, Pos::new(1, 0), &neighbor),
            ]
            .into_iter(),
        );

        // Buffer is 16x16 (2 cols * 8px, 1 row * 16px), so `i % 16` is the pixel's x coordinate.
        let buf = renderer.pixels();
        let green = 0x0000_FF00_u32;
        let has_green_at_x = |x: usize| {
            buf.iter()
                .enumerate()
                .any(|(i, &p)| i % 16 == x && p == green)
        };

        // Cell 1 (x >= 8): left half is the spilled block, right half is blue background.
        assert!(
            has_green_at_x(8),
            "block must spill into the neighbor cell at x=8"
        );
        assert!(has_green_at_x(11), "spill must reach x=11");
        assert!(
            !has_green_at_x(12),
            "spill stops at half the neighbor cell (x=12 is bg)"
        );
        // Cell 0 (x < 8): left half is now empty (block shifted right), right half is green.
        assert!(
            !has_green_at_x(0),
            "block shifted right, so x=0 is background"
        );
        assert!(
            has_green_at_x(4),
            "block still occupies x=4 in its own cell"
        );
    }

    #[test]
    fn removing_an_offset_leaves_stale_spill_in_the_neighbor_cell() {
        // retroglyph#717: `full_repaint` used to OR in only the *current* frame's offsets, so the
        // frame that removes the last offset took the incremental repaint path. That path only
        // repaints cells whose own tile changed, and the neighbor cell's tile here is untouched
        // (still blue background, byte-identical to the previous frame), so it was never
        // revisited to clear the spill the previous frame's offset glyph had painted into it.
        let mut renderer = SoftwareBackendBuilder::new()
            .font(retroglyph_window::font::unscii16::FONT)
            .grid_size(2, 1)
            .scale(1)
            .build()
            .unwrap()
            .into_renderer()
            .unwrap();

        let green = 0x0000_FF00_u32;
        let blue = 0x0000_00FF_u32;
        let block_at = |dx: i16| {
            Tile::new('\u{2588}', Style::new().fg(Color::rgb(0, 255, 0))).with_offset(dx, 0)
        };
        let neighbor = Tile::new(' ', Style::new().bg(Color::rgb(0, 0, 255)));

        // Frame 1: block offset right by half a cell, spills green into cell 1's left half.
        renderer.draw_layers(
            [
                DrawCell::on_layer(0, Pos::new(0, 0), &block_at(4)),
                DrawCell::on_layer(0, Pos::new(1, 0), &neighbor),
            ]
            .into_iter(),
        );
        let buf = renderer.pixels();
        assert_eq!(buf[8], green, "spill into cell 1 should be confirmed first");

        // Frame 2: offset removed entirely; cell 1's tile is byte-identical to frame 1.
        renderer.draw_layers(
            [
                DrawCell::on_layer(0, Pos::new(0, 0), &block_at(0)),
                DrawCell::on_layer(0, Pos::new(1, 0), &neighbor),
            ]
            .into_iter(),
        );
        let buf = renderer.pixels();
        assert!(
            buf[8..16].iter().all(|&p| p == blue),
            "cell 1 must be fully blue again once the offset spilling into it is gone, but got: \
             {:?}",
            &buf[8..16]
        );
    }

    #[test]
    fn pixel_snapshot_render_scene() {
        // Render a small multi-layer scene and snapshot the pixel output.
        let opts = SoftwareBackendBuilder::new()
            .grid_size(2, 2)
            .scale(1)
            .build()
            .unwrap();
        let mut renderer = opts.into_renderer().unwrap();

        // Layer 0: dark background, ':' at (0,0) in dim blue, '.' at (1,0) in dim gray.
        let bg = Tile::new(
            ':',
            Style::new()
                .fg(Color::rgb(60, 60, 80))
                .bg(Color::rgb(20, 20, 30)),
        );
        let dot = Tile::new(
            '.',
            Style::new()
                .fg(Color::rgb(40, 40, 50))
                .bg(Color::rgb(20, 20, 30)),
        );
        let entity = Tile::new(
            '@',
            Style::new()
                .fg(Color::rgb(0, 255, 0))
                .bg(Color::rgb(10, 10, 10)),
        )
        .with_offset(1, 0);
        // Single draw_layers call (clears buffer first).
        renderer.draw_layers(
            [
                DrawCell::on_layer(0, Pos::new(0, 0), &bg),
                DrawCell::on_layer(0, Pos::new(1, 0), &dot),
                DrawCell::on_layer(1, Pos::new(0, 0), &entity),
            ]
            .into_iter(),
        );

        // Snapshot the pixel buffer.
        // The buffer is 2 cells wide (16px) x 2 cells tall (32px) = 512 u32s.
        let pixels = renderer.pixels();
        assert_eq!(pixels.len(), 2 * 8 * 2 * 16); // cols * glyph_w * rows * glyph_h

        // Snapshot a debug representation: groups of 16 pixels per row (one pixel row across 2 cells).
        let row_strs: Vec<String> = pixels
            .chunks(16)
            .take(32)
            .map(|row| {
                row.iter()
                    .map(|p| format!("{p:08x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        let snapshot = row_strs.join("\n");

        insta::assert_snapshot!("pixel_snapshot_render_scene", snapshot);
    }

    #[test]
    fn higher_layer_opaque_background_paints_but_empty_cell_does_not() {
        // 2x1 grid. Layer 0 is a plain dark background across both cells.
        // Layer 1 puts a colored background only at cell (0, 0); cell (1, 0)
        // on layer 1 is left empty and must not disturb layer 0.
        let opts = SoftwareBackendBuilder::new()
            .grid_size(2, 1)
            .scale(1)
            .build()
            .unwrap();
        let mut renderer = opts.into_renderer().unwrap();

        let base = Tile::new(' ', Style::new().bg(Color::rgb(20, 20, 20)));
        // Layer 1 overlay: an opaque space (non-empty) with a red background.
        let overlay = Tile::new(' ', Style::new().bg(Color::rgb(200, 0, 0)));
        // Layer 1 empty cell (default tile) must be skipped.
        let empty = Tile::default();

        renderer.draw_layers(
            [
                DrawCell::on_layer(0, Pos::new(0, 0), &base),
                DrawCell::on_layer(0, Pos::new(1, 0), &base),
                DrawCell::on_layer(1, Pos::new(0, 0), &overlay),
                DrawCell::on_layer(1, Pos::new(1, 0), &empty),
            ]
            .into_iter(),
        );

        let pixels = renderer.pixels();
        let cell_w = 8usize; // glyph width at scale 1
        // Top-left pixel of cell (0, 0): the layer-1 red overlay wins.
        assert_eq!(pixels[0] & 0x00ff_ffff, 0x00c8_0000);
        // Top-left pixel of cell (1, 0): layer-1 cell was empty, so layer 0 shows.
        assert_eq!(pixels[cell_w] & 0x00ff_ffff, 0x0014_1414);
    }

    // ── Damage tracking (the row band fed to present_with_damage) ─────────
    //
    // The windowed present() upload can't run in a headless test (no surface),
    // but the damage computation runs in draw_layers regardless, so the band
    // is exactly what these assert. At scale 1 with the unscii16 font each cell
    // is 8x16 px, so the pixel buffer is (cols*8) x (rows*16) and cell-row r
    // occupies pixel rows [r*16, r*16+16). Damage is reported in pixel rows.

    /// Cell height in pixels at scale 1 (unscii16).
    const CELL_H_PX: u32 = 16;

    fn damage_renderer(cols: u16, rows: u16) -> SoftwareRenderer {
        SoftwareBackendBuilder::new()
            .grid_size(cols, rows)
            .scale(1)
            .build()
            .unwrap()
            .into_renderer()
            .unwrap()
    }

    /// Fill every layer-0 cell with `tile`, overriding cell `(ox, oy)` with
    /// `over` when given, then run `draw_layers` (which computes damage).
    fn draw_fill(
        r: &mut SoftwareRenderer,
        cols: u16,
        rows: u16,
        tile: &Tile,
        over: Option<(u16, u16, &Tile)>,
    ) {
        let mut items: Vec<DrawCell<'_>> = Vec::new();
        for y in 0..rows {
            for x in 0..cols {
                let t = match over {
                    Some((ox, oy, ot)) if ox == x && oy == y => ot,
                    _ => tile,
                };
                items.push(DrawCell::on_layer(0, Pos::new(x, y), t));
            }
        }
        r.draw_layers(items.into_iter()).unwrap();
    }

    fn bg_tile(r: u8, g: u8, b: u8) -> Tile {
        Tile::new(' ', Style::new().bg(Color::rgb(r, g, b)))
    }

    #[test]
    fn damage_first_frame_covers_whole_buffer() {
        // prev_pixels starts zeroed, so a non-black first frame differs on
        // every row: the whole buffer is damaged.
        let mut r = damage_renderer(2, 3);
        draw_fill(&mut r, 2, 3, &bg_tile(200, 0, 0), None);
        assert_eq!(r.ctx.damage_rows, Some((0, 3 * CELL_H_PX)));
    }

    #[test]
    fn damage_is_none_when_a_frame_is_unchanged() {
        let mut r = damage_renderer(2, 3);
        let red = bg_tile(200, 0, 0);
        draw_fill(&mut r, 2, 3, &red, None); // first frame: full damage
        // Headless present() is a no-op (nothing to upload without a window surface), so it
        // never touches damage_rows; a real present() is what would clear the pending band
        // here (retroglyph#724), so clear it directly to simulate that for this test.
        assert!(r.present().is_ok());
        r.ctx.damage_rows = None;
        draw_fill(&mut r, 2, 3, &red, None); // identical redraw: nothing changed
        assert_eq!(r.ctx.damage_rows, None);
    }

    #[test]
    fn damage_band_is_tight_for_a_localized_change() {
        let mut r = damage_renderer(2, 3);
        let red = bg_tile(200, 0, 0);
        draw_fill(&mut r, 2, 3, &red, None); // baseline
        // Simulate the present() that would normally clear the baseline's damage before the
        // next frame (headless present() is a no-op with no surface to clear it through).
        r.ctx.damage_rows = None;
        // Change only cell (0, 1); its pixels live in rows [16, 32).
        draw_fill(&mut r, 2, 3, &red, Some((0, 1, &bg_tile(0, 0, 200))));
        assert_eq!(r.ctx.damage_rows, Some((CELL_H_PX, 2 * CELL_H_PX)));
    }

    #[test]
    fn damage_band_spans_from_first_to_last_changed_row() {
        // Changes in cell-row 0 and cell-row 2 inflate the band to cover the
        // clean row 1 between them (single row band, documented limitation).
        let mut r = damage_renderer(2, 3);
        let red = bg_tile(200, 0, 0);
        draw_fill(&mut r, 2, 3, &red, None);
        let blue = bg_tile(0, 0, 200);
        // Two separate frames would each report one band; do it in one frame
        // by changing both corners relative to the baseline.
        let mut items: Vec<DrawCell<'_>> = Vec::new();
        for y in 0..3u16 {
            for x in 0..2u16 {
                let t = if (x, y) == (0, 0) || (x, y) == (1, 2) {
                    &blue
                } else {
                    &red
                };
                items.push(DrawCell::on_layer(0, Pos::new(x, y), t));
            }
        }
        r.draw_layers(items.into_iter()).unwrap();
        assert_eq!(r.ctx.damage_rows, Some((0, 3 * CELL_H_PX)));
    }

    #[test]
    fn damage_bands_union_across_draw_layers_calls_before_present() {
        // Regression test for retroglyph#724: `present()` is the only thing that clears
        // `damage_rows`, so two `draw_layers` calls touching disjoint rows before a single
        // `present()` must union their bands rather than the second overwriting the first.
        let mut r = damage_renderer(2, 3);
        let red = bg_tile(200, 0, 0);
        draw_fill(&mut r, 2, 3, &red, None); // baseline frame
        // Simulate the present() that would normally clear this baseline's damage (headless
        // present() is a no-op with no surface to clear it through).
        assert!(r.present().is_ok());
        r.ctx.damage_rows = None;

        let blue = bg_tile(0, 0, 200);
        // First draw_layers call: change only cell-row 0, pixel rows [0, 16).
        draw_fill(&mut r, 2, 3, &red, Some((0, 0, &blue)));
        assert_eq!(r.ctx.damage_rows, Some((0, CELL_H_PX)));

        // Second draw_layers call, with no present() in between: keep (0, 0) as it already
        // is (still blue, so it does not itself register as changed) and change only cell
        // (1, 2), pixel rows [32, 48), disjoint from the first change.
        let mut items: Vec<DrawCell<'_>> = Vec::new();
        for y in 0..3u16 {
            for x in 0..2u16 {
                let t = match (x, y) {
                    (0, 0) | (1, 2) => &blue,
                    _ => &red,
                };
                items.push(DrawCell::on_layer(0, Pos::new(x, y), t));
            }
        }
        r.draw_layers(items.into_iter()).unwrap();

        // Both bands must still be reflected: the first call's rows [0, 16) must not have
        // been dropped in favor of the second call's [32, 48).
        assert_eq!(r.ctx.damage_rows, Some((0, 3 * CELL_H_PX)));
    }

    #[test]
    fn resize_marks_full_frame_damage() {
        let mut r = damage_renderer(2, 3);
        let red = bg_tile(200, 0, 0);
        draw_fill(&mut r, 2, 3, &red, None);
        // Simulate the present() that would normally clear this baseline's damage (headless
        // present() is a no-op with no surface to clear it through).
        r.ctx.damage_rows = None;
        draw_fill(&mut r, 2, 3, &red, None);
        assert_eq!(r.ctx.damage_rows, None);
        // A resize invalidates the shadow buffer and forces a full repaint so
        // no stale pixels survive at the new size.
        r.resize(Size::new(4, 5));
        assert_eq!(r.ctx.damage_rows, Some((0, 5 * CELL_H_PX)));
    }

    #[test]
    fn resize_to_a_larger_grid_does_not_panic_on_the_next_draw() {
        // retroglyph#567: `resize` cleared `prev_tiles` but left the parallel `prev_tints`
        // shadow at its old (smaller) length, so the next `draw_layers` indexed it out of
        // bounds. Draw at the original size, resize larger, and draw again; this must not panic.
        let mut r = damage_renderer(2, 3);
        let red = bg_tile(200, 0, 0);
        draw_fill(&mut r, 2, 3, &red, None);
        r.resize(Size::new(4, 5));
        draw_fill(&mut r, 4, 5, &red, None);
        assert_eq!(r.ctx.damage_rows, Some((0, 5 * CELL_H_PX)));
    }

    #[test]
    fn clear_then_redrawing_the_same_frame_leaves_the_buffer_blank() {
        // retroglyph#694: `clear` zeroed `pixel_buf` but left the per-cell shadow (`prev_tiles`,
        // `prev_tints`, `prev_layer_count`) untouched, so redrawing the exact same frame after a
        // `clear` diffed against stale-but-identical shadow state, found nothing changed, and
        // painted nothing: the buffer stayed all zero instead of showing the redrawn content.
        let mut r = damage_renderer(2, 1);
        let red = bg_tile(200, 0, 0);
        draw_fill(&mut r, 2, 1, &red, None);
        assert!(r.pixels().iter().all(|&p| p == 0x00C8_0000));

        r.clear().unwrap();
        assert!(r.pixels().iter().all(|&p| p == 0));

        draw_fill(&mut r, 2, 1, &red, None);
        assert!(
            r.pixels().iter().all(|&p| p == 0x00C8_0000),
            "redrawing the same frame after clear() must repaint, not stay blank"
        );
    }

    // ── Dirty-cell repaint (retroglyph#302) ──────────────────────────────
    //
    // `draw_layers` always receives every cell (see `Compositing::PixelLayered`), but internally
    // it should only actually repaint pixels for cells that changed since the last call, falling
    // back to a full clear + repaint when a sub-cell offset or a layer-count change is in play.
    // These assert on the rendered pixels (not on any private dirty-tracking state), so they
    // hold regardless of how the internal shadow copy is implemented.

    /// Fills every layer-0 cell with `bg`, sets one cell's foreground glyph, and returns the tile
    /// list (index-stable across calls so a second call can flip one cell without rebuilding the
    /// rest).
    fn glyph_scene(
        cols: u16,
        rows: u16,
        bg: Color,
        glyph_pos: (u16, u16),
        glyph: char,
    ) -> Vec<Tile> {
        let mut out = Vec::with_capacity(usize::from(cols) * usize::from(rows));
        for y in 0..rows {
            for x in 0..cols {
                let style = if (x, y) == glyph_pos {
                    Style::new().fg(Color::rgb(0, 255, 0)).bg(bg)
                } else {
                    Style::new().bg(bg)
                };
                out.push(Tile::new(
                    if (x, y) == glyph_pos { glyph } else { ' ' },
                    style,
                ));
            }
        }
        out
    }

    fn draw_scene(r: &mut SoftwareRenderer, cols: u16, tiles: &[Tile]) {
        let content = tiles.iter().enumerate().map(|(i, t)| {
            #[allow(clippy::cast_possible_truncation)]
            let pos = Pos::new(
                (i % usize::from(cols)) as u16,
                (i / usize::from(cols)) as u16,
            );
            DrawCell::on_layer(0, pos, t)
        });
        r.draw_layers(content).unwrap();
    }

    #[test]
    fn dirty_cell_repaint_leaves_unchanged_cell_pixels_untouched() {
        // 3x1 grid, all cells the same dark background, distinct glyphs so a stray repaint of an
        // untouched cell would be visible. Change only the middle cell's glyph and re-draw; the
        // other two cells' pixels must be byte-for-byte identical to the first frame.
        let mut r = damage_renderer(3, 1);
        let base = glyph_scene(3, 1, Color::rgb(10, 10, 10), (1, 0), '@');
        draw_scene(&mut r, 3, &base);
        let before = r.pixels().to_vec();

        let mut changed = base.clone();
        changed[1] = Tile::new(
            '#',
            Style::new()
                .fg(Color::rgb(0, 0, 255))
                .bg(Color::rgb(10, 10, 10)),
        );
        draw_scene(&mut r, 3, &changed);
        let after = r.pixels().to_vec();

        let cell_w = 8usize; // unscii16 glyph width at scale 1.
        let cell_h = 16usize;
        let buf_w = 3 * cell_w;
        // Extracts cell `col`'s full pixel rect (all `cell_h` rows) out of a `buf_w`-wide buffer.
        let cell_pixels = |buf: &[u32], col: usize| -> Vec<u32> {
            (0..cell_h)
                .flat_map(|row| {
                    let start = row * buf_w + col * cell_w;
                    buf[start..start + cell_w].to_vec()
                })
                .collect()
        };

        // Cell 0 and cell 2 (unchanged) must be pixel-identical across the two frames.
        assert_eq!(
            cell_pixels(&before, 0),
            cell_pixels(&after, 0),
            "cell 0 pixels changed"
        );
        assert_eq!(
            cell_pixels(&before, 2),
            cell_pixels(&after, 2),
            "cell 2 pixels changed"
        );
        // Cell 1 (changed) must actually differ: '@' vs '#' in different colors.
        assert_ne!(
            cell_pixels(&before, 1),
            cell_pixels(&after, 1),
            "cell 1 pixels should have changed"
        );
    }

    #[test]
    fn dirty_cell_repaint_updates_changed_cell() {
        let mut r = damage_renderer(2, 1);
        let base = glyph_scene(2, 1, Color::rgb(0, 0, 0), (0, 0), ' ');
        draw_scene(&mut r, 2, &base);

        let mut changed = base;
        changed[0] = Tile::new(' ', Style::new().bg(Color::rgb(200, 0, 0)));
        draw_scene(&mut r, 2, &changed);

        let cell_w = 8usize;
        assert!(
            r.pixels()[0..cell_w].iter().all(|&p| p == 0x00C8_0000),
            "changed cell should show its new red background"
        );
    }

    #[test]
    fn sub_cell_offset_forces_full_frame_fallback_even_for_unchanged_cells() {
        // 2x1 grid. Frame 1: cell 0 has an offset glyph, cell 1 is plain. Frame 2: identical
        // content (nothing actually changed), but since a sub-cell offset is in play this frame,
        // the fallback repaint path runs regardless of the dirty set; assert the buffer is
        // still correct (not that any particular code path ran).
        let mut r = damage_renderer(2, 1);
        let bg = Tile::new(' ', Style::new().bg(Color::rgb(5, 5, 5)));
        let offset_fg = Tile::new('@', Style::new().fg(Color::rgb(0, 255, 0))).with_offset(1, 0);

        let draw = |r: &mut SoftwareRenderer| {
            r.draw_layers(
                [
                    DrawCell::on_layer(0, Pos::new(0, 0), &bg),
                    DrawCell::on_layer(1, Pos::new(0, 0), &offset_fg),
                    DrawCell::on_layer(0, Pos::new(1, 0), &bg),
                ]
                .into_iter(),
            )
            .unwrap();
        };

        draw(&mut r);
        let before = r.pixels().to_vec();
        draw(&mut r); // identical content; offsets are in play, so this takes the fallback path.
        let after = r.pixels().to_vec();

        assert_eq!(
            before, after,
            "identical frames with an active offset must render identically"
        );
        let has_green = |col: usize| {
            after
                .iter()
                .enumerate()
                .any(|(i, &p)| i % 16 == col && p == 0x0000_FF00)
        };
        assert!(!has_green(0), "x=0 should have no green pixels with dx=1");
        assert!(has_green(1), "x=1 should have green pixels with dx=1");
    }

    #[test]
    fn layer_count_change_forces_full_frame_repaint() {
        // 1x1 grid. Frame 1: only layer 0. Frame 2: layer 0 unchanged, but layer 1 newly
        // allocated with an opaque background: the layer-set change must not be missed by the
        // dirty-cell path (layer 0's own cell never changed, so a naive per-cell diff limited to
        // previously-seen layers would skip it).
        let mut r = damage_renderer(1, 1);
        let base = Tile::new(' ', Style::new().bg(Color::rgb(10, 10, 10)));
        r.draw_layers(core::iter::once(DrawCell::on_layer(
            0,
            Pos::new(0, 0),
            &base,
        )))
        .unwrap();

        let overlay = Tile::new(' ', Style::new().bg(Color::rgb(200, 0, 0)));
        r.draw_layers(
            [
                DrawCell::on_layer(0, Pos::new(0, 0), &base),
                DrawCell::on_layer(1, Pos::new(0, 0), &overlay),
            ]
            .into_iter(),
        )
        .unwrap();

        assert!(
            r.pixels().iter().all(|&p| p & 0x00ff_ffff == 0x00c8_0000),
            "newly-allocated layer 1's opaque background must be visible everywhere"
        );
    }
    //
    // `resolve_color` now delegates entirely to core's `Color::resolve_rgb`; this asserts the
    // packing is correct and that ANSI resolution agrees with core across the full 16-color
    // palette, so a regression in either the delegation or the packing is caught.

    #[test]
    fn resolve_color_matches_core_for_all_ansi_variants() {
        use retroglyph_core::color::AnsiColor;
        for index in 0..16u8 {
            let ansi = AnsiColor::try_from(index).expect("0..16 are valid ANSI indices");
            let (r, g, b) = ansi.to_rgb();
            let core_rgb = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
            assert_eq!(
                resolve_color(Color::Ansi(ansi), DEFAULT_BG),
                core_rgb,
                "{ansi:?}: resolve_color no longer matches retroglyph-core's Color::resolve_rgb"
            );
        }
    }

    // ── Output/Cursor conformance (retroglyph#763) ──────────────────────────────────────────

    fn conformance_renderer(size: Size) -> SoftwareRenderer {
        SoftwareBackendBuilder::new()
            .font(retroglyph_window::font::unscii16::FONT)
            .grid_size(size.width(), size.height())
            .scale(1)
            .build()
            .unwrap()
            .into_renderer()
            .unwrap()
    }

    /// Wraps [`SoftwareRenderer`] so [`Observable::snapshot`] hashes only the pixels that changed
    /// since the previous call, per that trait's docs: this backend's observable state is its
    /// pixel buffer (framebuffer-shaped, not a log), so this remembers the previous call's pixels
    /// and hashes only the `(index, pixel)` pairs that differ from it.
    struct SoftwareObserver {
        renderer: SoftwareRenderer,
        previous: Vec<u32>,
    }

    impl SoftwareObserver {
        fn new(size: Size) -> Self {
            let renderer = conformance_renderer(size);
            let previous = renderer.pixels().to_vec();
            Self { renderer, previous }
        }
    }

    impl Output for SoftwareObserver {
        type Error = core::convert::Infallible;

        fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
        where
            I: Iterator<Item = DrawCell<'a>>,
        {
            self.renderer.draw_layers(content)
        }

        fn flush(&mut self) -> Result<(), Self::Error> {
            self.renderer.flush()
        }

        fn size(&self) -> Size {
            Output::size(&self.renderer)
        }

        fn clear(&mut self) -> Result<(), Self::Error> {
            Output::clear(&mut self.renderer)?;
            self.settle()
        }

        fn resize(&mut self, size: Size) {
            Output::resize(&mut self.renderer, size);
            // This backend always reports `needs_full_frame: true` (see its `Output` impl):
            // its pixel buffer is only actually repainted (and so only meaningfully
            // observable) on the next `draw_layers` call, which `Terminal::present` always
            // supplies in real use. `snapshot()` reads pixels directly instead, so settle it
            // here with an empty full-repaint pass rather than let a caller observe whatever
            // `resize`'s own buffer growth left behind.
            let _ = self.settle();
        }
    }

    impl SoftwareObserver {
        fn settle(&mut self) -> Result<(), core::convert::Infallible> {
            self.renderer.draw_layers(core::iter::empty())?;
            self.renderer.flush()
        }
    }

    impl Cursor for SoftwareObserver {}

    impl retroglyph_core::testing::conformance::Observable for SoftwareObserver {
        fn snapshot(&mut self) -> u64 {
            let current = self.renderer.pixels();
            let mut hash = retroglyph_core::testing::conformance::fnv1a(b"software-diff");
            for (index, (&was, &now)) in self.previous.iter().zip(current.iter()).enumerate() {
                if was != now {
                    hash ^=
                        retroglyph_core::testing::conformance::fnv1a(&(index as u64).to_ne_bytes());
                    hash ^= retroglyph_core::testing::conformance::fnv1a(&now.to_ne_bytes());
                }
            }
            self.previous = current.to_vec();
            hash
        }
    }

    #[test]
    fn satisfies_the_output_contract() {
        retroglyph_core::testing::conformance::assert_output_contract(SoftwareObserver::new);
    }

    #[test]
    fn satisfies_the_cursor_contract() {
        // `SoftwareRenderer`'s `Cursor` impl is a no-op (no hardware cursor in software mode),
        // so this is expected to pass trivially: included anyway so a future change that gives
        // this backend real cursor tracking is checked against the same contract as the terminal
        // backends are.
        retroglyph_core::testing::conformance::assert_cursor_contract(SoftwareObserver::new);
    }

    #[test]
    fn satisfies_the_input_contract() {
        retroglyph_core::testing::conformance::assert_input_contract(|| {
            conformance_renderer(Size::new(10, 10))
        });
    }

    /// `expand_dirty_spans` reads a shadow buffer that can lag a resize by a frame (see its doc
    /// comment), so a covered cell's stored `(dx, dy)` offset can point before the start of the
    /// buffer once reinterpreted against the new `cols` stride. This must be a no-op for that
    /// cell rather than panic on the underflowing subtraction.
    #[test]
    fn expand_dirty_spans_skips_a_covered_cell_whose_anchor_offset_underflows() {
        use retroglyph_core::grid::Grid;
        use retroglyph_core::grid::Pos;

        let cols = 2;
        let rows = 3;
        let mut grid = Grid::new(cols, rows);
        grid.write_span_uniform(0, (0, 0), (1, 2), 'A', ' ', Style::default());
        let layer: Vec<Tile> = (0..rows)
            .flat_map(|y| (0..cols).map(move |x| (x, y)))
            .map(|(x, y)| grid.tile(0, Pos::new(x, y)).copied().unwrap_or_default())
            .collect();

        // idx 2 is (0, 1), the covered cell with offset (dx, dy) = (0, 1). Calling with a much
        // wider `cols` than the layer was built with reproduces the stale-buffer case: dy * cols
        // now exceeds idx.
        let mut dirty = vec![false; layer.len()];
        dirty[2] = true;
        expand_dirty_spans(&mut dirty, &layer, 10, rows as usize);

        assert_eq!(dirty, vec![false, false, true, false, false, false]);
    }
}

// ── Multi-cell sprite tests (retroglyph#412) ────────────────────────────────

#[cfg(all(test, feature = "tilesets"))]
mod span_tests {
    use super::*;
    use config::SoftwareBackendBuilder;
    use retroglyph_core::color::Color;
    use retroglyph_core::color::Style;
    use retroglyph_core::grid::Grid;
    use retroglyph_core::grid::Pos;
    use retroglyph_window::tileset::{Codepage, SheetColor, SpriteAlign, TilesetOptions};

    const RED: u32 = 0x00FF_0000;
    const BLUE: u32 = 0x0000_00FF;
    const GREEN: u32 = 0x0000_FF00;

    /// A one-tile PNG of `w` x `h` pixels, solid opaque red except for a fully transparent
    /// right-hand column band `transparent_from` pixels in, so a test can see the background
    /// through part of the sprite.
    fn sprite_png(w: u32, h: u32, transparent_from: u32) -> Vec<u8> {
        use image::ImageEncoder as _;
        let mut img = image::RgbaImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let px = if x >= transparent_from {
                    [0, 0, 0, 0]
                } else {
                    [0xFF, 0, 0, 0xFF]
                };
                img.put_pixel(x, y, image::Rgba(px));
            }
        }
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)
            .expect("encode test tileset PNG");
        png
    }

    /// A `cols` x `rows` renderer at scale 1 with `'S'` bound to a `w` x `h` sprite.
    fn renderer_with_sprite(
        cols: u16,
        rows: u16,
        w: u32,
        h: u32,
        transparent_from: u32,
        align: SpriteAlign,
    ) -> SoftwareRenderer {
        #[allow(clippy::cast_possible_truncation)]
        let opts = TilesetOptions::builder(sprite_png(w, h, transparent_from))
            .tile_size(w as u16, h as u16)
            .columns(1)
            .codepage(Codepage::Custom(vec!['S']))
            .align(align)
            .build()
            .expect("valid single-tile tileset");
        SoftwareBackendBuilder::new()
            .font(retroglyph_window::font::unscii16::FONT)
            .grid_size(cols, rows)
            .scale(1)
            .tileset(opts)
            .build()
            .unwrap()
            .into_renderer()
            .unwrap()
    }

    /// A 2-tile 16x16 PNG: tile 0 (`'S'`) fully opaque red, tile 1 (`'T'`) red only in its left
    /// half and transparent in its right. Swapping between the two changes what the sprite paints
    /// in the *second* cell of a 2x1 span without changing that cell's tile at all.
    fn wide_and_narrow_png() -> Vec<u8> {
        use image::ImageEncoder as _;
        let (w, h) = (16u32, 16u32);
        let mut img = image::RgbaImage::new(w * 2, h);
        for y in 0..h {
            for x in 0..w * 2 {
                let opaque = x < w || x - w < 8;
                let px = if opaque {
                    [0xFF, 0, 0, 0xFF]
                } else {
                    [0, 0, 0, 0]
                };
                img.put_pixel(x, y, image::Rgba(px));
            }
        }
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(img.as_raw(), w * 2, h, image::ExtendedColorType::Rgba8)
            .expect("encode test tileset PNG");
        png
    }

    /// Streams every cell of `grid`'s layer 0 through `draw_layers`, the path the compositing
    /// backend actually uses.
    fn paint(renderer: &mut SoftwareRenderer, grid: &Grid) {
        let tiles: Vec<(u8, Pos, Tile)> = (0..grid.height())
            .flat_map(|y| (0..grid.width()).map(move |x| (x, y)))
            .map(|(x, y)| (0u8, Pos::new(x, y), *grid.tile(0, (x, y)).unwrap()))
            .collect();
        renderer
            .draw_layers(
                tiles
                    .iter()
                    .map(|(l, pos, tile)| DrawCell::on_layer(*l, *pos, tile)),
            )
            .unwrap();
    }

    /// The buffer's pixel at `(x, y)`, given a `cols`-wide grid of 8x16 cells at scale 1.
    fn px(renderer: &SoftwareRenderer, cols: usize, x: usize, y: usize) -> u32 {
        renderer.pixels()[y * cols * 8 + x]
    }

    /// The literal subject of retroglyph#412: a span reserves cells the artwork does not fill,
    /// and the reserved cells stop drawing their own glyph and take the anchor's background.
    #[test]
    fn span_reserves_cells_beyond_the_artwork() {
        // An 8x16 sprite (exactly one cell) declared as a 2x1 span, so cell 1 is reserved but no
        // sprite pixel ever reaches it.
        let mut r = renderer_with_sprite(2, 1, 8, 16, 8, SpriteAlign::TopLeft);
        let mut grid = Grid::new(2, 1);
        grid.write_span(0, 0, 0, &["S#"], Style::new().bg(Color::rgb(0, 0, 255)))
            .unwrap();
        paint(&mut r, &grid);

        // Cell 0 is the sprite.
        assert_eq!(px(&r, 2, 0, 0), RED);
        // Cell 1 is the anchor's background, with no trace of the '#' fallback glyph.
        for y in 0..16 {
            for x in 8..16 {
                assert_eq!(px(&r, 2, x, y), BLUE, "covered cell pixel ({x}, {y})");
            }
        }
    }

    #[test]
    fn covered_cell_glyph_is_not_drawn() {
        // A fully transparent sprite: anything visible in cell 1 can only be its fallback glyph.
        let mut r = renderer_with_sprite(2, 1, 8, 16, 0, SpriteAlign::TopLeft);
        let mut grid = Grid::new(2, 1);
        grid.write_span(
            0,
            0,
            0,
            &["S#"],
            Style::new()
                .fg(Color::rgb(0, 255, 0))
                .bg(Color::rgb(0, 0, 255)),
        )
        .unwrap();
        paint(&mut r, &grid);

        assert!(
            !r.pixels().contains(&GREEN),
            "the covered cell's fallback glyph must not be drawn on a pixel backend"
        );
    }

    #[test]
    fn span_background_fills_the_whole_footprint() {
        // A 16x16 sprite spanning 2x1 cells, transparent past x=8: the background must show
        // through the transparent half, which lives in the *covered* cell.
        let mut r = renderer_with_sprite(2, 1, 16, 16, 8, SpriteAlign::TopLeft);
        let mut grid = Grid::new(2, 1);
        grid.write_span(0, 0, 0, &["S#"], Style::new().bg(Color::rgb(0, 0, 255)))
            .unwrap();
        paint(&mut r, &grid);

        assert_eq!(px(&r, 2, 0, 0), RED, "sprite's opaque half");
        // The transparent half lands in the covered cell, so what shows through there is the
        // background the anchor established, not the renderer's default, and not a fill of the
        // covered cell's own.
        for y in 0..16 {
            assert_eq!(px(&r, 2, 8, y), BLUE, "covered cell pixel (8, {y})");
        }
    }

    /// `paint`, with `tint` applied to every cell's anchor.
    fn paint_tinted(renderer: &mut SoftwareRenderer, grid: &Grid, tint: Tint) {
        let tiles: Vec<(u8, Pos, Tile)> = (0..grid.height())
            .flat_map(|y| (0..grid.width()).map(move |x| (x, y)))
            .map(|(x, y)| (0u8, Pos::new(x, y), *grid.tile(0, (x, y)).unwrap()))
            .collect();
        renderer
            .draw_layers(
                tiles
                    .iter()
                    .map(|(l, pos, tile)| DrawCell::on_layer(*l, *pos, tile).with_tint(tint)),
            )
            .unwrap();
    }

    /// A 1x1 grid holding the fully opaque red sprite `'S'`, drawn with `fg` and `tint`.
    ///
    /// `transparent_from` is the x the sprite goes transparent at, so passing the sprite's own
    /// width keeps every pixel opaque.
    fn sprite_pixel(fg: Color, tint: Tint) -> u32 {
        let mut r = renderer_with_sprite(1, 1, 8, 16, 8, SpriteAlign::TopLeft);
        let mut grid = Grid::new(1, 1);
        grid.write_span(0, 0, 0, &["S"], Style::new().fg(fg))
            .unwrap();
        paint_tinted(&mut r, &grid, tint);
        px(&r, 1, 0, 0)
    }

    #[test]
    fn an_art_sheet_ignores_fg_however_it_is_set() {
        // The #537 regression guard: a full-color sheet renders as authored, and a caller who
        // sets `fg` hoping to tint it gets no silent change.
        for fg in [Color::Default, Color::rgb(0, 255, 0), Color::rgb(0, 0, 255)] {
            assert_eq!(
                sprite_pixel(fg, Tint::None),
                RED,
                "fg {fg:?} tinted an art sprite"
            );
        }
    }

    #[test]
    fn multiply_darkens_the_sprite() {
        let half = sprite_pixel(Color::Default, Tint::multiply(128, 128, 128));
        assert_eq!(half, 0x0080_0000, "red scaled by 128/255 with rounding");
    }

    #[test]
    fn mix_brightens_the_sprite_which_multiply_cannot() {
        // Toward white: the red channel stays saturated and the other two come up off zero,
        // which no multiply of an opaque red pixel can produce.
        let flashed = sprite_pixel(Color::Default, Tint::mix(255, 255, 255, 128));
        assert_eq!(flashed & 0x00FF_0000, 0x00FF_0000, "red stays saturated");
        assert!(flashed & 0x0000_FF00 > 0, "green lifted off zero");
        assert!(flashed & 0x0000_00FF > 0, "blue lifted off zero");
    }

    #[test]
    fn the_software_blit_agrees_with_sprite_tint_apply() {
        // The anchor for cross-backend agreement: the CPU rasteriser must produce exactly what
        // `SpriteTint::apply` says, because the GL fragment shader mirrors that same function.
        // If this drifts, the two backends have diverged (which is how #537 happened).
        for tint in [
            Tint::None,
            Tint::multiply(128, 64, 32),
            Tint::multiply(255, 255, 255),
            Tint::mix(0, 255, 0, 200),
            Tint::mix(255, 255, 255, 255),
        ] {
            let expected = SpriteTint::resolve(SheetColor::Art, Color::Default, tint, DEFAULT_FG)
                .apply((255, 0, 0));
            let want =
                u32::from(expected.0) << 16 | u32::from(expected.1) << 8 | u32::from(expected.2);
            assert_eq!(sprite_pixel(Color::Default, tint), want, "tint {tint:?}");
        }
    }

    #[test]
    fn a_tint_only_change_repaints_the_cell() {
        // The tile is byte-identical across both frames; only the tint moves. Damage tracking
        // compares `Tile`s, which cannot see a tint, so this would silently not repaint without
        // the parallel `prev_tints` shadow copy.
        let mut r = renderer_with_sprite(1, 1, 8, 16, 8, SpriteAlign::TopLeft);
        let mut grid = Grid::new(1, 1);
        grid.write_span(0, 0, 0, &["S"], Style::new()).unwrap();

        paint_tinted(&mut r, &grid, Tint::None);
        assert_eq!(px(&r, 1, 0, 0), RED);

        paint_tinted(&mut r, &grid, Tint::multiply(128, 128, 128));
        assert_eq!(
            px(&r, 1, 0, 0),
            0x0080_0000,
            "a tint-only change must mark the cell dirty"
        );
    }

    #[test]
    fn sprite_align_center_shifts_the_blit_within_the_span_box() {
        // An 8x16 sprite centered in a 2x1 span of 8x16 cells: 8px of slack, so it starts at x=4.
        let mut r = renderer_with_sprite(2, 1, 8, 16, 8, SpriteAlign::Center);
        let mut grid = Grid::new(2, 1);
        grid.write_span(0, 0, 0, &["S#"], Style::new().bg(Color::rgb(0, 0, 255)))
            .unwrap();
        paint(&mut r, &grid);

        assert_eq!(px(&r, 2, 3, 0), BLUE, "left of the centered sprite");
        assert_eq!(px(&r, 2, 4, 0), RED, "centered sprite starts at x=4");
        assert_eq!(px(&r, 2, 11, 0), RED, "centered sprite ends at x=11");
        assert_eq!(px(&r, 2, 12, 0), BLUE, "right of the centered sprite");
    }

    /// Regression guard for the stale-pixel half of retroglyph#412.
    ///
    /// The incremental repaint path only touches cells whose own tile changed, but a span's
    /// artwork is drawn from its anchor and paints across every cell the span covers. A covered
    /// cell's tile holds only the fallback glyph and the offset back to the anchor, so it stays
    /// byte-identical while the anchor's artwork changes underneath it, and its pixels go stale.
    ///
    /// Each direction of the swap below catches one half of the fix:
    ///
    /// - Opaque to transparent needs the covered cell to be marked dirty *from the anchor*, or
    ///   its background is never repainted and the old opaque pixels survive.
    /// - Transparent to opaque needs every dirty cell's background laid down *before* any glyph,
    ///   or the covered cell's own background fill erases the sprite the anchor spilled into it.
    #[test]
    fn changing_only_a_span_anchor_repaints_its_covered_cells() {
        let opts = TilesetOptions::builder(wide_and_narrow_png())
            .tile_size(16, 16)
            .columns(2)
            .codepage(Codepage::Custom(vec!['S', 'T']))
            .build()
            .expect("valid 2-tile tileset");
        let mut r = SoftwareBackendBuilder::new()
            .font(retroglyph_window::font::unscii16::FONT)
            .grid_size(2, 1)
            .scale(1)
            .tileset(opts)
            .build()
            .unwrap()
            .into_renderer()
            .unwrap();

        let bg = Style::new().bg(Color::rgb(0, 0, 255));
        let frame = |anchor: &str| {
            let mut grid = Grid::new(2, 1);
            grid.write_span(0, 0, 0, &[anchor], bg).unwrap();
            grid
        };
        let wide = frame("S#");
        let narrow = frame("T#");
        assert_eq!(
            wide.tile(0, (1, 0)),
            narrow.tile(0, (1, 0)),
            "this only bites while the covered cell's own tile is unchanged"
        );

        // The right half of the span is the covered cell, at x >= 8.
        let covered_is = |r: &SoftwareRenderer, want: u32, what: &str| {
            for y in 0..16 {
                for x in 8..16 {
                    assert_eq!(px(r, 2, x, y), want, "{what}: covered pixel ({x}, {y})");
                }
            }
        };

        paint(&mut r, &wide);
        covered_is(&r, RED, "sanity: the wide sprite reaches the covered cell");

        paint(&mut r, &narrow);
        covered_is(&r, BLUE, "opaque to transparent left stale sprite pixels");

        paint(&mut r, &wide);
        covered_is(&r, RED, "transparent to opaque had its spill erased");
    }

    #[test]
    fn a_span_on_layer_0_does_not_suppress_layer_1() {
        // Coverage is per layer: a sprite on layer 0 must not blank a glyph drawn above it.
        let mut r = renderer_with_sprite(2, 1, 16, 16, 16, SpriteAlign::TopLeft);
        let mut grid = Grid::new(2, 1);
        grid.write_span(0, 0, 0, &["S#"], Style::default()).unwrap();
        grid.put_tile(
            1,
            (1, 0),
            Tile::new('\u{2588}', Style::new().fg(Color::rgb(0, 255, 0))),
        );

        let tiles: Vec<(u8, Pos, Tile)> = (0..=1u8)
            .flat_map(|layer| (0..2u16).map(move |x| (layer, x)))
            .map(|(layer, x)| (layer, Pos::new(x, 0), *grid.tile(layer, (x, 0)).unwrap()))
            .collect();
        r.draw_layers(
            tiles
                .iter()
                .map(|(l, pos, tile)| DrawCell::on_layer(*l, *pos, tile)),
        )
        .unwrap();

        assert!(
            r.pixels().contains(&GREEN),
            "layer 1's glyph must still render over a layer 0 span"
        );
    }

    #[test]
    fn a_span_with_no_sprite_draws_only_its_anchor_glyph() {
        // No tileset entry for 'X', so the anchor falls back to the bitmap font, and the
        // covered cells stay blank rather than each drawing their own fallback glyph.
        let mut r = renderer_with_sprite(2, 1, 8, 16, 8, SpriteAlign::TopLeft);
        let mut grid = Grid::new(2, 1);
        grid.write_span(
            0,
            0,
            0,
            &["\u{2588}\u{2588}"],
            Style::new().fg(Color::rgb(0, 255, 0)),
        )
        .unwrap();
        paint(&mut r, &grid);

        assert_eq!(px(&r, 2, 0, 0), GREEN, "the anchor's own glyph still draws");
        assert_ne!(px(&r, 2, 8, 0), GREEN, "the covered cell's glyph does not");
    }

    /// A 2-tile `8x16` PNG in two columns: tile 0 solid red, tile 1 solid green. Both tiles are
    /// exactly one cell, so a sprite drawn from either fills its cell with no alignment ambiguity.
    fn two_solid_tiles_png() -> Vec<u8> {
        use image::ImageEncoder as _;
        let (tile_w, tile_h) = (8u32, 16u32);
        let img_w = tile_w * 2;
        let mut img = image::RgbaImage::new(img_w, tile_h);
        for y in 0..tile_h {
            for x in 0..img_w {
                let px = if x < tile_w {
                    [0xFF, 0x00, 0x00, 0xFF]
                } else {
                    [0x00, 0xFF, 0x00, 0xFF]
                };
                img.put_pixel(x, y, image::Rgba(px));
            }
        }
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(img.as_raw(), img_w, tile_h, image::ExtendedColorType::Rgba8)
            .expect("encode test tileset PNG");
        png
    }

    /// Regression guard for retroglyph#762: a span's text fallback glyph can have its own
    /// registered sprite (the default `Codepage::Cp437` registers one for every codepoint, so
    /// this is the common case, not an edge case), and the covered cell must still draw nothing
    /// of its own, not that sprite.
    #[test]
    fn a_span_covered_cells_fallback_sprite_does_not_paint_over_the_anchor() {
        // 'S' -> tile 0 (red), the anchor's own sprite, exactly one cell (so it reserves but
        // does not fill cell 1, per `span_reserves_cells_beyond_the_artwork`). '#' -> tile 1
        // (green) is the covered cell's own text-fallback glyph, also registered as a sprite:
        // before the fix, the covered cell's sprite lookup ran unconditionally and painted this
        // sprite into cell 1 instead of leaving it reserved.
        let opts = TilesetOptions::builder(two_solid_tiles_png())
            .tile_size(8, 16)
            .columns(2)
            .codepage(Codepage::Custom(vec!['S', '#']))
            .build()
            .expect("valid 2-tile tileset");
        let mut r = SoftwareBackendBuilder::new()
            .font(retroglyph_window::font::unscii16::FONT)
            .grid_size(2, 1)
            .scale(1)
            .tileset(opts)
            .build()
            .unwrap()
            .into_renderer()
            .unwrap();

        let mut grid = Grid::new(2, 1);
        grid.write_span(0, 0, 0, &["S#"], Style::default()).unwrap();
        paint(&mut r, &grid);

        assert_eq!(px(&r, 2, 0, 0), RED, "the anchor's own sprite still draws");
        assert_ne!(
            px(&r, 2, 8, 0),
            GREEN,
            "the covered cell must not draw '#''s own sprite"
        );
    }

    // ── Dropped-tint diagnostic (retroglyph#564) ───────────────────────────

    #[test]
    fn draw_layers_reports_a_tint_on_a_glyph_without_a_sprite() {
        // 'X' has no tileset entry, so it falls back to the bitmap font and any tint on it is
        // silently dropped: exactly the trap retroglyph#537 fell into.
        let mut r = renderer_with_sprite(1, 1, 8, 16, 8, SpriteAlign::TopLeft);
        let mut grid = Grid::new(1, 1);
        grid.write_span(0, 0, 0, &["X"], Style::new()).unwrap();
        paint_tinted(&mut r, &grid, Tint::multiply(128, 128, 128));

        assert_eq!(
            r.ctx.warned_dropped_tint.contains(&'X'),
            retroglyph_core::dev::DEV
        );
    }

    #[test]
    fn draw_layers_does_not_report_a_tint_on_a_glyph_that_has_a_sprite() {
        // 'S' does resolve to a sprite, so its tint is applied, not dropped, and must not be
        // reported.
        let mut r = renderer_with_sprite(1, 1, 8, 16, 8, SpriteAlign::TopLeft);
        let mut grid = Grid::new(1, 1);
        grid.write_span(0, 0, 0, &["S"], Style::new()).unwrap();
        paint_tinted(&mut r, &grid, Tint::multiply(128, 128, 128));

        assert!(!r.ctx.warned_dropped_tint.contains(&'S'));
    }

    #[test]
    fn draw_layers_does_not_report_tint_none() {
        let mut r = renderer_with_sprite(1, 1, 8, 16, 8, SpriteAlign::TopLeft);
        let mut grid = Grid::new(1, 1);
        grid.write_span(0, 0, 0, &["X"], Style::new()).unwrap();
        paint(&mut r, &grid);

        assert!(r.ctx.warned_dropped_tint.is_empty());
    }

    #[test]
    fn draw_reports_a_tint_on_a_glyph_without_a_sprite() {
        // `Output::draw` forwards to `draw_layers` (retroglyph#561), so this is really exercising
        // that the forwarding preserves per-cell tint data rather than dropping it along the way.
        let mut r = renderer_with_sprite(1, 1, 8, 16, 8, SpriteAlign::TopLeft);
        let tile = Tile::new('X', Style::new());
        r.draw(core::iter::once(
            DrawCell::new(Pos::new(0, 0), &tile).with_tint(Tint::multiply(128, 128, 128)),
        ))
        .unwrap();

        assert_eq!(
            r.ctx.warned_dropped_tint.contains(&'X'),
            retroglyph_core::dev::DEV
        );
    }
}

// ── Font chain tests (retroglyph#539) ───────────────────────────────────────

#[cfg(all(test, feature = "default-font"))]
mod font_chain_tests {
    use super::*;
    use config::SoftwareBackendBuilder;
    use retroglyph_core::color::Color;
    use retroglyph_core::color::Style;
    use retroglyph_core::grid::Pos;
    use retroglyph_window::font::{BitmapFont, FontChain, unscii16};

    const RED: Color = Color::rgb(255, 0, 0);
    const BLUE: Color = Color::rgb(0, 0, 255);
    const BLACK: Color = Color::rgb(0, 0, 0);

    const RED_PX: u32 = 0x00FF_0000;
    const BLUE_PX: u32 = 0x0000_00FF;
    const BLACK_PX: u32 = 0x0000_0000;

    /// One 8x16 glyph with the upper-left quadrant filled: U+2598, which CP437 has no mapping for
    /// at all, so it is only reachable through the charset table below.
    static QUADRANT_DATA: [u8; 16] = [
        0xF0, 0xF0, 0xF0, 0xF0, 0xF0, 0xF0, 0xF0, 0xF0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    const QUADRANT_CHARSET: [(char, u8); 1] = [('▘', 0)];
    const QUADRANT_FONT: BitmapFont =
        BitmapFont::with_charset(&QUADRANT_DATA, 8, 16, 1, &QUADRANT_CHARSET);
    static FALLBACKS: [BitmapFont; 1] = [QUADRANT_FONT];

    /// Renders `ch` in `fg` on black, in a 1x1 grid drawn through `fonts`.
    fn render(fonts: FontChain<'static>, ch: char, fg: Color) -> Vec<u32> {
        let mut renderer = SoftwareBackendBuilder::new()
            .font(fonts)
            .grid_size(1, 1)
            .scale(1)
            .build()
            .expect("chain builds")
            .into_renderer()
            .expect("renderer builds");
        let tile = Tile::new(ch, Style::new().fg(fg).bg(BLACK));
        renderer
            .draw_layers(core::iter::once(DrawCell::on_layer(
                0,
                Pos::new(0, 0),
                &tile,
            )))
            .unwrap();
        renderer.pixels().to_vec()
    }

    /// The bug: both pixel backends resolved every glyph through a CP437-only mapping, so a
    /// `with_charset` fallback font's glyphs were unreachable and rendered as the solid block.
    #[test]
    fn charset_fallback_glyph_renders_its_own_shape() {
        let pixels = render(FontChain::new(unscii16::FONT, &FALLBACKS), '▘', RED);

        for y in 0..16 {
            for x in 0..8 {
                let expected = if x < 4 && y < 8 { RED_PX } else { BLACK_PX };
                assert_eq!(pixels[y * 8 + x], expected, "pixel ({x},{y})");
            }
        }
    }

    /// The point of routing sub-cell glyphs through a font rather than a tileset: a font glyph is
    /// a 1-bit mask painted in the cell's own foreground, so the same glyph serves every color,
    /// while a tileset sprite carries the colors it was authored in (#537).
    #[test]
    fn charset_fallback_glyph_takes_the_cells_foreground_color() {
        let chain = FontChain::new(unscii16::FONT, &FALLBACKS);
        let red = render(chain, '▘', RED);
        let blue = render(chain, '▘', BLUE);

        assert_eq!(red[0], RED_PX);
        assert_eq!(blue[0], BLUE_PX);
    }

    /// A character no font in the chain covers still gets the solid-block substitute, so the
    /// chain is not a way to silently lose glyphs.
    #[test]
    fn uncovered_char_still_renders_the_solid_block() {
        let pixels = render(FontChain::new(unscii16::FONT, &FALLBACKS), 'あ', RED);
        assert!(
            pixels.iter().all(|&p| p == RED_PX),
            "every pixel of the solid block is foreground"
        );
    }

    /// retroglyph#1292: the solid-block substitute above is a legitimate cell on its own, so
    /// nothing about the rendered pixels distinguishes it from a real solid-block glyph. This is
    /// the diagnostic that closes that gap: a dev build reports the character the chain couldn't
    /// actually draw.
    #[test]
    fn uncovered_char_reports_the_notdef_diagnostic() {
        let mut renderer = SoftwareBackendBuilder::new()
            .font(FontChain::new(unscii16::FONT, &FALLBACKS))
            .grid_size(1, 1)
            .scale(1)
            .build()
            .expect("chain builds")
            .into_renderer()
            .expect("renderer builds");
        let tile = Tile::new('あ', Style::new().fg(RED).bg(BLACK));
        renderer
            .draw_layers(core::iter::once(DrawCell::on_layer(
                0,
                Pos::new(0, 0),
                &tile,
            )))
            .unwrap();

        assert_eq!(
            renderer.ctx.warned_notdef.contains(&'あ'),
            retroglyph_core::dev::DEV
        );
    }

    /// A character some font in the chain does cover must never be reported, dev build or not.
    #[test]
    fn covered_char_does_not_report_the_notdef_diagnostic() {
        let mut renderer = SoftwareBackendBuilder::new()
            .font(FontChain::new(unscii16::FONT, &FALLBACKS))
            .grid_size(1, 1)
            .scale(1)
            .build()
            .expect("chain builds")
            .into_renderer()
            .expect("renderer builds");
        let tile = Tile::new('A', Style::new().fg(RED).bg(BLACK));
        renderer
            .draw_layers(core::iter::once(DrawCell::on_layer(
                0,
                Pos::new(0, 0),
                &tile,
            )))
            .unwrap();

        assert!(!renderer.ctx.warned_notdef.contains(&'A'));
    }

    /// The panic reported in #539 (`glyph index 219 out of range (2)`): the CP437 solid block is
    /// glyph 0xDB, which a small `with_charset` font doesn't have, so substituting it by index
    /// indexed past the end of that font's data. A chain with nothing drawable for a character
    /// now leaves the cell at its background instead.
    #[test]
    fn char_no_font_can_draw_leaves_the_cell_blank() {
        let pixels = render(FontChain::from(QUADRANT_FONT), 'A', RED);
        assert!(
            pixels.iter().all(|&p| p == BLACK_PX),
            "an undrawable character paints no foreground at all"
        );
    }
}
