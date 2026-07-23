//! CPU rasterization backend: renders grid cells into a pixel buffer and
//! blits it to a window surface via `softbuffer`.
//!
//! # Architecture
//!
//! [`SoftwareBackend`] holds configuration only (font, grid size, scale); it
//! does not implement [`Backend`](retroglyph_core::Backend). Call
//! [`run_headless`](SoftwareBackend::run_headless) to build a
//! [`SoftwareRenderer`], which does the actual rendering work:
//!
//! ```text
//! SoftwareBackend (config: font, grid size, scale)
//!   |  .run_headless()
//!   v
//! SoftwareRenderer
//!   implements retroglyph_core::{Output, Input, Cursor} (= Backend)
//!   implements retroglyph_window::Presenter (an Output supertrait)
//!   |                                |
//!   |                                v
//!   |                     wrapped in retroglyph_window::WindowBackend,
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
//! [`Presenter`](retroglyph_window::Presenter) against raw window handles
//! ([`WindowHandle`]), so anything that produces those (winit via
//! `retroglyph-window`, or another windowing library) can drive it. Because
//! `Presenter` is an [`Output`] supertrait, `SoftwareRenderer`'s single
//! `Output` implementation satisfies both `Backend`'s output half and `Presenter` directly, with
//! no duplicated method bodies. `retroglyph-window`'s
//! [`WindowBackend`](retroglyph_window::WindowBackend) wraps a `Presenter` to provide the full
//! [`Backend`](retroglyph_core::Backend) for windowed use, owning the input event queue that this
//! crate does not.
//!
//! For headless use (in-memory rendering, pixel-level tests) skip windowing
//! entirely: [`SoftwareRenderer`] implements [`Output`],
//! [`Input`], and [`Cursor`] directly (bundled
//! as [`Backend`](retroglyph_core::Backend)), so `Terminal<SoftwareRenderer>` works without a
//! window, and [`pixels`](SoftwareRenderer::pixels) gives direct access to the rendered
//! buffer.

pub mod config;

#[cfg(feature = "tilesets")]
pub mod sprite_cache;
#[cfg(feature = "tilesets")]
pub mod tileset;

// Platform-specific window surface. Both modules expose a `WindowSurface` with
// the same `new`/`resize`/`present` API and their own `SurfaceError`, so the
// renderer below drives either without `cfg` in its body. This is the same
// module-swap pattern std uses for `std::sys`.
#[cfg(not(target_arch = "wasm32"))]
#[path = "surface_native.rs"]
mod surface;
#[cfg(target_arch = "wasm32")]
#[path = "surface_wasm.rs"]
mod surface;

pub use surface::SurfaceError;
use surface::WindowSurface;

// Compile the code blocks in this crate's own README as doctests so its quick start is
// type-checked on every test run and cannot silently rot. The `cfg(doctest)` gate keeps this out
// of the rendered crate documentation -- see `retroglyph-crossterm`'s matching include for the
// same pattern applied to the workspace root README.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

use retroglyph_core::backend::{Cursor, Input, Output};
use retroglyph_core::color::Color;

// The bitmap font lives in its own crate (`retroglyph-font`) so `retroglyph-gl` can share the
// exact same glyph source. Re-exported here for ergonomics (the builder's `font()` takes one);
// `FallbackFontChain`, `unscii16`, etc. are reached through `retroglyph_font` directly.
pub use config::{SoftwareBackend, SoftwareBackendBuilder, SoftwareBackendError};
pub use retroglyph_font::BitmapFont;

#[cfg(feature = "tilesets")]
use alpha_blend::rgba::U8x4Rgba;
use grixy::buf::GridBuf;
use grixy::ops::GridWrite;
use grixy::ops::layout::RowMajor;
use retroglyph_core::event::Event;
use retroglyph_core::grid::{Pos, Size};
use retroglyph_core::tile::Tile;
use retroglyph_font::BitmapFont as Font;
use retroglyph_window::WindowHandle;
#[cfg(feature = "tilesets")]
use sprite_cache::{Sprite, SpriteCache};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

// ── Public types ──────────────────────────────────────────────────────────────

/// A running software renderer, produced by [`SoftwareBackend::run_headless`].
///
/// Unlike [`SoftwareBackend`] (which is just configuration), this type
/// always has an active rendering context: its pixel buffer is always
/// available, and the `ctx` field is never `None`, so [`Output`] methods
/// never panic for missing initialisation.
///
/// Call [`pixels`](Self::pixels) to inspect the rendered output, or use
/// [`Output::draw`] and [`Output::draw_layers`] to render into it.
///
/// If the `tilesets` feature is enabled, the sprite tileset is loaded once, at
/// [`run_headless`](SoftwareBackend::run_headless) time, into an internal
/// [`SpriteCache`]. That cache has no reload/hot-swap support (see its
/// docs); to pick up a changed tileset, rebuild the renderer via a fresh [`SoftwareBackend`]
/// configuration rather than mutating this one.
pub struct SoftwareRenderer {
    options: SoftwareBackend,
    /// The bitmap font, extracted from `options.font` at construction time.
    /// Always present; the `Option` wrapper in `SoftwareBackend` is only for
    /// the builder validation step.
    font: BitmapFont,
    ctx: RenderContext,
    #[cfg(feature = "tilesets")]
    sprite_cache: Arc<SpriteCache>,
}

struct RenderContext {
    event_buffer: VecDeque<Event>,
    pixel_buf: GridBuf<u32, Vec<u32>, RowMajor>,
    window_surface: Option<WindowSurface>,
    cell_w: u32,
    cell_h: u32,
    /// Shadow copy of `pixel_buf` from the previous frame, used to compute
    /// the damaged row range in [`present`](SoftwareRenderer::present).
    /// Kept the same length as `pixel_buf`; resized (and the whole frame
    /// marked damaged) whenever the buffer is resized.
    prev_pixels: Vec<u32>,
    /// Row range `[y0, y1)` changed since the last present, computed in
    /// `draw_layers` by diffing against `prev_pixels`. `None` means no rows
    /// changed (nothing to present).
    damage_rows: Option<(u32, u32)>,
    /// Shadow copy of every allocated layer's tiles from the last `draw_layers` call, indexed by
    /// `[layer_id][y * cols + x]`. Used to find dirty cells without touching core's diff model:
    /// `draw_layers` already receives every cell on every allocated layer every frame (see
    /// [`Output::needs_full_frame`]), so comparing against this shadow copy in place is enough to
    /// tell which cells actually changed, with no new core API needed. Resized (and cleared,
    /// forcing a full repaint) whenever the grid is resized; grown (never shrunk) as new layer ids
    /// are seen.
    prev_tiles: Vec<Vec<Tile>>,
    /// Reusable per-cell dirty scratch buffer, `true` at index `y * cols + x` when any layer's
    /// tile at that position changed this frame. Indexed the same way as each `prev_tiles` layer;
    /// resized alongside it.
    dirty_mask: Vec<bool>,
    /// Number of layers (`max layer id + 1`) present in the last `draw_layers` call. A change in
    /// this count between frames (a layer being newly allocated or fully deallocated) forces a
    /// full repaint next frame, since the dirty-cell path can only compare cells within layers
    /// present in both frames.
    prev_layer_count: usize,
}

impl SoftwareRenderer {
    /// Creates a new renderer with the given buffer and cell dimensions.
    pub(crate) fn create(
        options: SoftwareBackend,
        font: BitmapFont,
        buf_w: usize,
        buf_h: usize,
        cell_w: u32,
        cell_h: u32,
        #[cfg(feature = "tilesets")] sprite_cache: Arc<SpriteCache>,
    ) -> Self {
        Self {
            options,
            font,
            ctx: RenderContext {
                event_buffer: VecDeque::new(),
                pixel_buf: GridBuf::from_buffer(vec![0u32; buf_w * buf_h], buf_w),
                window_surface: None,
                cell_w,
                cell_h,
                prev_pixels: vec![0u32; buf_w * buf_h],
                damage_rows: None,
                prev_tiles: Vec::new(),
                dirty_mask: Vec::new(),
                // Sentinel distinct from any real layer count (always < 256), so the very first
                // `draw_layers` call is unconditionally treated as a layer-set change and takes
                // the full-repaint path once, seeding `prev_tiles` for every subsequent frame.
                prev_layer_count: usize::MAX,
            },
            #[cfg(feature = "tilesets")]
            sprite_cache,
        }
    }

    /// Returns a slice of the rendered pixel buffer (`0x00RRGGBB` format).
    ///
    /// The buffer length is `cols * (glyph_width * scale) * rows * (glyph_height * scale)`.
    /// Each `u32` is a packed RGB pixel with the top byte unused.
    ///
    /// This is always available: there is no `Option` wrapper because
    /// `SoftwareRenderer` is guaranteed to have an active rendering context.
    #[must_use]
    pub fn pixels(&self) -> &[u32] {
        self.ctx.pixel_buf.as_ref()
    }

    /// Pushes an event into the internal buffer, to be drained by
    /// [`Input::poll_event`].
    pub fn push_event(&mut self, event: Event) {
        self.ctx.event_buffer.push_back(event);
    }

    /// Initializes the window surface from a raw window/display handle.
    ///
    /// The concrete surface is platform-specific (softbuffer on native, a
    /// `Canvas2D` context on wasm32); see the `surface` module.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError`] if the platform surface cannot be created.
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
    /// Returns [`SurfaceError`] if the platform surface fails to present.
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
        // Damage has been presented (or the attempt is done); drop it so a
        // later present() with no new draw_layers() call is a no-op instead of
        // re-presenting stale damage.
        self.ctx.damage_rows = None;
        result
    }

    /// Diffs `pixel_buf` against `prev_pixels` row by row to find the
    /// smallest contiguous `[y0, y1)` band that changed and stores it as
    /// `damage_rows`.
    ///
    /// Only the `[y0, y1)` band (not the whole buffer) is copied from
    /// `pixel_buf` into `prev_pixels` afterwards, since every other row is
    /// already known to match; when nothing changed, the copy is skipped
    /// entirely. `draw_layers` always repaints every cell (see
    /// [`Output::needs_full_frame`]), so `prev_pixels` still has to hold a
    /// full previous-frame pixel buffer to diff against -- this only removes
    /// the copy's cost from being proportional to the whole buffer instead of
    /// the changed region, which is what actually dominates this function's
    /// cost on an unchanged or near-unchanged frame.
    ///
    /// If the buffers differ in length (a resize raced with this call)
    /// the whole frame is marked damaged and `prev_pixels` is resized to
    /// match.
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

        self.ctx.damage_rows = y0.map(|y0| {
            #[allow(clippy::cast_possible_truncation)]
            (y0 as u32, y1 as u32)
        });

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
    /// [`resolve_bg_fill`]'s "an occupied tile is opaque" rule -- see its doc comment. Without the
    /// `tilesets` feature there is no sprite cache at all, so this always returns `false`.
    #[cfg(feature = "tilesets")]
    fn has_sprite(&self, glyph: char) -> bool {
        self.sprite_cache.get(glyph).is_some()
    }

    #[cfg(not(feature = "tilesets"))]
    fn has_sprite(&self, _glyph: char) -> bool {
        false
    }

    /// Paints one layer's tile at `pos` into the pixel buffer: the cell-rect background fill
    /// (when `bg_fill` is `Some`, see [`resolve_bg_fill`] and [`Output::draw_layers`]'s doc for
    /// the exact rule) followed by the sprite or bitmap-glyph foreground.
    ///
    /// `tile` is taken by value (it's `Copy`) rather than by reference so callers can read it out
    /// of `self.ctx.prev_tiles` before calling this, instead of holding a borrow of `self.ctx`
    /// across a call that needs `&mut self.ctx.pixel_buf`.
    #[allow(clippy::too_many_arguments)]
    fn composite_cell(
        &mut self,
        buf_w: usize,
        cell_w: usize,
        cell_h: usize,
        scale: usize,
        pos: Pos,
        tile: Tile,
        bg_fill: Option<u32>,
    ) {
        let px_x = usize::from(pos.x) * cell_w;
        let px_y = usize::from(pos.y) * cell_h;

        if let Some(bg) = bg_fill {
            let rect = ixy::Rect::new(px_x, px_y, cell_w, cell_h);
            self.ctx.pixel_buf.fill_rect_solid(rect, bg);
        }

        // Sprite cache dispatch: sprite wins over bitmap font.
        #[cfg(feature = "tilesets")]
        {
            let buf_h = self.ctx.pixel_buf.as_ref().len() / buf_w;
            if let Some(sprite) = self.sprite_cache.get(tile.glyph()) {
                blit_sprite(
                    self.ctx.pixel_buf.as_mut(),
                    buf_w,
                    buf_h,
                    px_x,
                    px_y,
                    &tile,
                    sprite,
                    scale,
                );
                return;
            }
        }

        blit_glyph(
            self.ctx.pixel_buf.as_mut(),
            buf_w,
            px_x,
            px_y,
            &tile,
            &self.font,
            cell_w,
            cell_h,
            scale,
        );
    }
}

// ── Renderer construction ────────────────────────────────────────────────────────────

impl SoftwareBackend {
    /// Creates a headless renderer that renders into an internal buffer
    /// without opening a window.
    ///
    /// This does not block: it returns a [`SoftwareRenderer`] immediately.
    /// The renderer's pixel buffer can be inspected via
    /// [`SoftwareRenderer::pixels`], or the renderer can be handed to
    /// `retroglyph_window::winit::run_windowed` to drive a window.  Flushing
    /// is a no-op (the buffer stays in memory).
    ///
    /// This is primarily useful for testing pixel-level output without
    /// needing a window or event loop.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::Output;
    /// use retroglyph_core::tile::Tile;
    /// use retroglyph_core::style::Style;
    /// use retroglyph_core::grid::Pos;
    /// use retroglyph_core::Color;
    /// use retroglyph_software::SoftwareBackendBuilder;
    ///
    /// let mut renderer = SoftwareBackendBuilder::new()
    ///     .grid_size(1, 1)
    ///     .scale(1)
    ///     .build()
    ///     .unwrap()
    ///     .run_headless()
    ///     .unwrap();
    ///
    /// // Render a red cell on layer 0.
    /// let tile = Tile::new(' ', Style::new().bg(Color::Rgb { r: 255, g: 0, b: 0 }));
    /// renderer
    ///     .draw_layers([(0, Pos::new(0, 0), &tile, None)].into_iter())
    ///     .unwrap();
    ///
    /// assert!(renderer.pixels().iter().all(|&p| p == 0x00FF_0000));
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`SoftwareBackendError::NoFont`] if no font is set (only reachable if
    /// [`SoftwareBackendBuilder::build`] was bypassed) and
    /// [`SoftwareBackendError::Tileset`] if a registered tileset fails to load.
    ///
    /// # Panics
    ///
    /// Panics only on a `u32`-to-`usize` conversion that cannot fail on any target
    /// this crate supports (`usize` is at least 32 bits on every 32- and 64-bit
    /// platform), so this is not reachable in practice.
    pub fn run_headless(self) -> Result<SoftwareRenderer, SoftwareBackendError> {
        let Some(font) = self.font else {
            return Err(SoftwareBackendError::NoFont);
        };

        let cell_w = u32::from(font.glyph_width) * u32::from(self.scale);
        let cell_h = u32::from(font.glyph_height) * u32::from(self.scale);
        // u32 always fits in usize (all targets: 32- and 64-bit).
        let buf_w = usize::from(self.cols) * usize::try_from(cell_w).unwrap();
        let buf_h = usize::from(self.rows) * usize::try_from(cell_h).unwrap();

        #[cfg(feature = "tilesets")]
        let sprite_cache = if self.tilesets.is_empty() {
            Arc::new(SpriteCache::new())
        } else {
            let mut cache = SpriteCache::new();
            for opts in &self.tilesets {
                cache.load(opts).map_err(SoftwareBackendError::Tileset)?;
            }
            Arc::new(cache)
        };

        Ok(SoftwareRenderer::create(
            self,
            font,
            buf_w,
            buf_h,
            cell_w,
            cell_h,
            #[cfg(feature = "tilesets")]
            sprite_cache,
        ))
    }
}

// ── Output impl ─────────────────────────────────────────────────────────────────

impl Output for SoftwareRenderer {
    type Error = core::convert::Infallible;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (Pos, &'a Tile, Option<&'a str>)>,
    {
        let font = &self.font;
        let scale = usize::from(self.options.scale);
        let cols = self.options.cols;
        let glyph_w = usize::from(font.glyph_width) * scale;
        let glyph_h = usize::from(font.glyph_height) * scale;
        let buf_w = usize::from(cols) * glyph_w;

        #[cfg(feature = "tilesets")]
        let sprite_cache = Some(&*self.sprite_cache);

        // This bitmap/sprite renderer keys everything off `Tile::glyph`, not
        // the full grapheme cluster: sprite lookup, ligature-free bitmap
        // fonts, and sub-cell offsets are all single-codepoint concepts here.
        for (pos, cell, _extra) in content {
            blit_cell(
                self.ctx.pixel_buf.as_mut(),
                buf_w,
                pos,
                cell,
                font,
                glyph_w,
                glyph_h,
                scale,
                #[cfg(feature = "tilesets")]
                sprite_cache,
            );
        }
        Ok(())
    }

    /// Composite the raw layer stream into the pixel buffer.
    ///
    /// Layers arrive layer-major (0 first), so painting them in order gives the
    /// correct z-order. Layer 0 always fills its cell background; a higher layer's
    /// occupied (non-empty) tile always fills a background too, and an empty tile
    /// never does -- see the private `resolve_bg_fill` helper for the exact color each of those
    /// cases paints (it is not always the tile's own background, to mirror
    /// `Grid::flatten_into`'s background-inheritance rule exactly). The `is_empty`
    /// guard matters because this receives the full frame (see
    /// [`needs_full_frame`](Output::needs_full_frame)), including empty
    /// higher-layer cells that must not overwrite layer 0.
    ///
    /// This matches cell backends (retroglyph#304): an occupied space with a
    /// [`Color::Default`] background on a higher layer erases the glyph beneath it
    /// when flattened, and this backend now does too, by repainting that cell's
    /// background (see the private `resolve_bg_fill` helper) even though the occupied tile's own
    /// background is the default one.
    ///
    /// # Dirty-cell repaint (retroglyph#302)
    ///
    /// `needs_full_frame` always returns `true` for this backend (see its docs), so this
    /// receives every cell on every allocated layer on every call -- `Terminal::present`'s
    /// diff-only path (used when a backend's `needs_full_frame` is `false`) never applies here,
    /// and changing that would be a `retroglyph-core` API change. Instead, this method keeps its
    /// own per-cell shadow copy of the last frame's tiles (see `RenderContext::prev_tiles`) and
    /// diffs incoming cells against it here, entirely internally: cells whose tile is unchanged
    /// since the last call are skipped instead of being cleared and repainted.
    ///
    /// This falls back to the old clear-then-repaint-every-cell strategy when either:
    /// - any tile this frame has a nonzero sub-cell offset ([`Tile::dx`]/[`Tile::dy`]): offsets
    ///   can spill glyph pixels into neighboring cells by an amount `Tile` does not bound, so
    ///   containing the repaint to a neighborhood around the changed cells isn't possible without
    ///   a magnitude cap core doesn't provide; or
    /// - the number of allocated layers changed since the last call: a layer's cells falling out
    ///   of (or into) the frame can't be diffed against a shadow copy that no longer describes
    ///   this frame's layer set.
    ///
    /// When neither applies, a changed cell at a given position also forces every *other* layer
    /// at that same position to be repainted (even if unchanged there), because a lower layer's
    /// background fill covers the whole cell rect and would otherwise erase an unchanged higher
    /// layer's already-composited glyph pixels on top of it.
    fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u8, Pos, &'a Tile, Option<&'a str>)>,
    {
        let cols = usize::from(self.options.cols);
        let rows = usize::from(self.options.rows);
        let scale = usize::from(self.options.scale);
        let cell_w = usize::from(self.font.glyph_width) * scale;
        let cell_h = usize::from(self.font.glyph_height) * scale;
        let buf_w = cols * cell_w;
        let cell_count = cols * rows;

        if self.ctx.dirty_mask.len() == cell_count {
            self.ctx.dirty_mask.iter_mut().for_each(|d| *d = false);
        } else {
            self.ctx.dirty_mask.clear();
            self.ctx.dirty_mask.resize(cell_count, false);
        }

        let mut any_offset = false;
        let mut any_dirty = false;
        let mut max_layer_seen: i32 = -1;

        for (layer_id, pos, tile, _extra) in content {
            let layer_idx = usize::from(layer_id);
            max_layer_seen = max_layer_seen.max(i32::from(layer_id));

            if layer_idx >= self.ctx.prev_tiles.len() {
                self.ctx
                    .prev_tiles
                    .resize_with(layer_idx + 1, || vec![Tile::default(); cell_count]);
            } else if self.ctx.prev_tiles[layer_idx].len() != cell_count {
                self.ctx.prev_tiles[layer_idx] = vec![Tile::default(); cell_count];
            }

            let idx = usize::from(pos.y) * cols + usize::from(pos.x);
            let slot = &mut self.ctx.prev_tiles[layer_idx][idx];
            if *slot != *tile {
                self.ctx.dirty_mask[idx] = true;
                any_dirty = true;
                *slot = *tile;
            }
            if tile.dx() != 0 || tile.dy() != 0 {
                any_offset = true;
            }
        }

        #[allow(clippy::cast_sign_loss)]
        let layer_count_now = (max_layer_seen + 1) as usize;
        let layers_changed = layer_count_now != self.ctx.prev_layer_count;
        self.ctx.prev_layer_count = layer_count_now;

        let full_repaint = any_offset || layers_changed;

        if full_repaint {
            self.ctx.pixel_buf.clear();
            for layer_id in 0..layer_count_now {
                for idx in 0..cell_count {
                    #[allow(clippy::cast_possible_truncation)]
                    let layer_id = layer_id as u8;
                    let tile = self.ctx.prev_tiles[layer_id as usize][idx];
                    let has_sprite = self.has_sprite(tile.glyph());
                    let bg_fill = resolve_bg_fill(&self.ctx.prev_tiles, layer_id, idx, has_sprite);
                    #[allow(clippy::cast_possible_truncation)]
                    let pos = Pos::new((idx % cols) as u16, (idx / cols) as u16);
                    self.composite_cell(buf_w, cell_w, cell_h, scale, pos, tile, bg_fill);
                }
            }
        } else if any_dirty {
            for idx in 0..cell_count {
                if !self.ctx.dirty_mask[idx] {
                    continue;
                }
                #[allow(clippy::cast_possible_truncation)]
                let pos = Pos::new((idx % cols) as u16, (idx / cols) as u16);
                for layer_id in 0..layer_count_now {
                    #[allow(clippy::cast_possible_truncation)]
                    let layer_id = layer_id as u8;
                    let tile = self.ctx.prev_tiles[layer_id as usize][idx];
                    let has_sprite = self.has_sprite(tile.glyph());
                    let bg_fill = resolve_bg_fill(&self.ctx.prev_tiles, layer_id, idx, has_sprite);
                    self.composite_cell(buf_w, cell_w, cell_h, scale, pos, tile, bg_fill);
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
        Size {
            width: self.options.cols,
            height: self.options.rows,
        }
    }

    fn resize(&mut self, size: Size) {
        self.options.cols = size.width;
        self.options.rows = size.height;
        let font = &self.font;
        let cell_w = usize::from(font.glyph_width) * usize::from(self.options.scale);
        let cell_h = usize::from(font.glyph_height) * usize::from(self.options.scale);
        let new_w = usize::from(size.width) * cell_w;
        let new_h = usize::from(size.height) * cell_h;
        self.ctx.pixel_buf.resize(new_w, new_h);
        // Buffer dimensions changed: the shadow buffer no longer matches,
        // so drop it and force a full-frame damage rect on the next present.
        self.ctx.prev_pixels.clear();
        self.ctx.prev_pixels.resize(new_w * new_h, 0);
        // The per-cell tile shadow is keyed by the old grid dimensions; drop it too so the next
        // `draw_layers` call can't misread stale entries against the new layout, and force that
        // call onto the full-repaint path (`prev_layer_count` back to its initial value never
        // matches a real frame's layer count).
        self.ctx.prev_tiles.clear();
        self.ctx.dirty_mask.clear();
        self.ctx.prev_layer_count = usize::MAX;
        self.ctx.damage_rows = if new_h == 0 {
            None
        } else {
            #[allow(clippy::cast_possible_truncation)]
            Some((0, new_h as u32))
        };
        #[allow(clippy::cast_possible_truncation)]
        {
            self.ctx.cell_w = cell_w as u32;
            self.ctx.cell_h = cell_h as u32;
        }
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.ctx.pixel_buf.clear();
        Ok(())
    }

    fn needs_full_frame(&self) -> bool {
        true
    }

    fn composites_layers(&self) -> bool {
        true
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
impl retroglyph_window::Presenter for SoftwareRenderer {
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
        (self.ctx.cell_w, self.ctx.cell_h)
    }
}

// ── Grid compositing ──────────────────────────────────────────────────────────

/// Renders one grid cell into `buffer` using 1-bit bitmap glyph data.
///
/// Each set bit in the font row maps to `fg`; each clear bit maps to `bg`.
/// No alpha blending is needed: bitmap fonts are 1-bit. The glyph is shifted
/// by `cell.dx()/dy()` sub-cell pixel offset (scaled by `scale`), matching
/// [`blit_glyph`]; the background fill always covers the full, unshifted
/// cell rectangle. When `scale > 1` each source pixel becomes a `scale×scale`
/// block.
///
/// If `sprite_cache` contains a sprite for the cell's glyph, the bitmap font
/// path is skipped in favor of [`blit_sprite`].
#[allow(clippy::cast_possible_truncation, clippy::too_many_arguments)]
fn blit_cell(
    buffer: &mut [u32],
    buf_w: usize,
    pos: Pos,
    cell: &Tile,
    font: &Font,
    cell_w: usize,
    cell_h: usize,
    scale: usize,
    #[cfg(feature = "tilesets")] sprite_cache: Option<&SpriteCache>,
) {
    let px_x = pos.x as usize * cell_w;
    let px_y = pos.y as usize * cell_h;

    #[cfg(feature = "tilesets")]
    if let Some(sprite) = sprite_cache.and_then(|c| c.get(cell.glyph())) {
        let buf_h = buffer.len() / buf_w;
        blit_sprite(buffer, buf_w, buf_h, px_x, px_y, cell, sprite, scale);
        return;
    }

    let fg = resolve_color(cell.style().foreground(), DEFAULT_FG);
    let bg = resolve_color(cell.style().background(), DEFAULT_BG);

    let buf_h = buffer.len() / buf_w;

    // Fill the entire cell rectangle with background first: the glyph is
    // painted on top, offset by `dx`/`dy`, so it may not cover the whole
    // cell (or may spill into neighboring cells). The background fill is
    // never sub-cell-offset, so the only clipping needed is against the
    // buffer edges (for a partial cell at the grid boundary); precompute the
    // clamped range once and fill whole rows instead of checking every pixel.
    if px_y < buf_h && px_x < buf_w {
        let y_end = (px_y + cell_h).min(buf_h);
        let x_end = (px_x + cell_w).min(buf_w);
        for y in px_y..y_end {
            let row_start = y * buf_w + px_x;
            let row_end = y * buf_w + x_end;
            buffer[row_start..row_end].fill(bg);
        }
    }

    #[allow(clippy::cast_possible_wrap)]
    let origin_x = px_x as i64 + i64::from(cell.dx()) * scale as i64;
    #[allow(clippy::cast_possible_wrap)]
    let origin_y = px_y as i64 + i64::from(cell.dy()) * scale as i64;

    let glyph_index = font.char_to_index(cell.glyph());
    let rows = font.rows(glyph_index);
    let src_w = usize::from(font.glyph_width);

    blit_glyph_mask(
        buffer, buf_w, buf_h, origin_x, origin_y, rows, src_w, scale, fg,
    );
}

/// Paints the set bits of a 1-bit glyph bitmap (`rows`, each `src_w` bits
/// wide) into `buffer` as `color`, with each source pixel scaled to a
/// `scale x scale` destination block.
///
/// The glyph's top-left destination corner is `(origin_x, origin_y)`
/// (already including any sub-cell `dx`/`dy` offset, scaled). When the whole
/// glyph's destination bounding box fits inside `buffer` -- the overwhelmingly
/// common case, since it only fails for cells with a nonzero `dx`/`dy` that
/// pushes them past a buffer edge -- this takes a fast path with no per-pixel
/// bounds check: it fills each `scale`-wide destination run in one slice
/// `fill` call. Otherwise it falls back to a row-clamped path that clips
/// each destination run to the buffer bounds once per row, rather than
/// checking every pixel.
#[allow(clippy::too_many_arguments, clippy::cast_possible_truncation)]
fn blit_glyph_mask(
    buffer: &mut [u32],
    buf_w: usize,
    buf_h: usize,
    origin_x: i64,
    origin_y: i64,
    rows: &[u8],
    src_w: usize,
    scale: usize,
    color: u32,
) {
    let glyph_w = src_w * scale;
    let glyph_h = rows.len() * scale;

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
        for (src_y, &mask) in rows.iter().enumerate() {
            for src_x in 0..src_w {
                if (mask >> (src_w - 1 - src_x)) & 1 == 0 {
                    continue;
                }
                let x0 = ox + src_x * scale;
                let y0 = oy + src_y * scale;
                for sdy in 0..scale {
                    let row_start = (y0 + sdy) * buf_w + x0;
                    buffer[row_start..row_start + scale].fill(color);
                }
            }
        }
        return;
    }

    #[allow(
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        clippy::similar_names
    )]
    for (src_y, &mask) in rows.iter().enumerate() {
        for src_x in 0..src_w {
            if (mask >> (src_w - 1 - src_x)) & 1 == 0 {
                continue;
            }
            for sdy in 0..scale {
                let y = origin_y + (src_y * scale + sdy) as i64;
                if y < 0 || y as usize >= buf_h {
                    continue;
                }
                let y = y as usize;
                let x_start = origin_x + (src_x * scale) as i64;
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
}

/// Blits a glyph's set bits into `buffer` at `(px_x, px_y)` plus sub-cell
/// offset from `tile.dx`/`tile.dy`. Only the foreground (glyph) pixels are
/// painted; background is left untouched.
#[allow(clippy::cast_possible_truncation, clippy::too_many_arguments)]
fn blit_glyph(
    buffer: &mut [u32],
    buf_w: usize,
    px_x: usize,
    px_y: usize,
    tile: &Tile,
    font: &Font,
    _cell_w: usize,
    _cell_h: usize,
    scale: usize,
) {
    if tile.glyph() == ' ' {
        return;
    }

    let fg = resolve_color(tile.style().foreground(), DEFAULT_FG);

    #[allow(clippy::cast_possible_wrap)]
    let origin_x = px_x as i64 + i64::from(tile.dx()) * scale as i64;
    #[allow(clippy::cast_possible_wrap)]
    let origin_y = px_y as i64 + i64::from(tile.dy()) * scale as i64;

    let glyph_index = font.char_to_index(tile.glyph());
    let rows = font.rows(glyph_index);
    let src_w = usize::from(font.glyph_width);
    let buf_h = buffer.len() / buf_w;

    blit_glyph_mask(
        buffer, buf_w, buf_h, origin_x, origin_y, rows, src_w, scale, fg,
    );
}

/// Blit a decoded RGBA8 sprite into `buffer` with alpha blending.
///
/// The sprite's top-left corner is at pixel `(cell_px_x + tile.dx * scale,
/// cell_px_y + tile.dy * scale)`. If `spacing_cells > 1`, the sprite's pixels
/// extend beyond the anchor cell into adjacent cells.
///
/// Pixels outside `buffer` bounds are silently clipped.
///
/// Blending uses pure integer `U8x4Rgba::source_over`. Fully opaque pixels
/// (alpha == 255) skip blending entirely and write directly to the buffer.
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
    tile: &Tile,
    sprite: &Sprite,
    scale: usize,
) {
    let origin_x = cell_px_x as i64 + i64::from(tile.dx()) * scale as i64;
    let origin_y = cell_px_y as i64 + i64::from(tile.dy()) * scale as i64;

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

            // Fast path: fully opaque pixels write directly, no blending.
            // Most roguelike sprites are opaque, so this skips U8x4Rgba
            // construction + source_over for the common case.
            let rgb = u32::from(src.r) << 16 | u32::from(src.g) << 8 | u32::from(src.b);
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

/// Default foreground when [`Color::Default`] is used.
const DEFAULT_FG: u32 = 0x00d4_d4d4;
/// Default background when [`Color::Default`] is used.
const DEFAULT_BG: u32 = 0x0000_0000;

/// Determines the background this layer/tile should paint at `idx` in `composite_cell`, if any,
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
///   `has_sprite` is `true` -- this is the fix for retroglyph#304: an occupied space is opaque and
///   erases whatever glyph a lower layer drew there, even though its own background is the
///   default one. What it paints with is *not* [`DEFAULT_BG`] though: matching `flatten_into`'s
///   `if tile.style.bg != Color::Default` guard, a `Color::Default` background never overwrites
///   the destination background, so this walks back down through the layers below `layer_id`
///   (down to and including layer 0) to find whichever one last established a background, and
///   repaints with that instead. `has_sprite` opts a tile out of this rule entirely: sprites
///   carry genuine per-pixel alpha (see [`SoftwareRenderer::has_sprite`]), so forcing an opaque
///   fill underneath one before it's blended would erase transparency the sprite's own pixels are
///   supposed to let show through -- core's `Tile`/`Grid` model has no such per-pixel concept, so
///   the cell-backend-parity rule this function otherwise implements just doesn't apply to them.
fn resolve_bg_fill(
    prev_tiles: &[Vec<Tile>],
    layer_id: u8,
    idx: usize,
    has_sprite: bool,
) -> Option<u32> {
    let layer_idx = usize::from(layer_id);
    let tile = prev_tiles[layer_idx][idx];
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
        let bg = prev_tiles[below][idx].style().background();
        if below == 0 || bg != Color::Default {
            return Some(resolve_color(bg, DEFAULT_BG));
        }
    }
    unreachable!("the loop above always terminates at `below == 0`, which always returns")
}

/// Resolve a [`Color`] to a packed `0x00RRGGBB` value, substituting
/// `default_rgb` for [`Color::Default`].
///
/// Delegates the actual palette resolution to `retroglyph-core`'s [`Color::resolve_rgb`], the
/// single canonical color-to-RGB path every graphical backend shares (so the CPU rasterizer and
/// `retroglyph-gl`'s GPU atlas agree on every pixel color); this only unpacks/repacks between
/// core's `(r, g, b)` triples and this backend's `0x00RRGGBB` `u32` pixel format.
fn resolve_color(color: Color, default_rgb: u32) -> u32 {
    #[allow(clippy::cast_possible_truncation)]
    let default = (
        (default_rgb >> 16) as u8,
        (default_rgb >> 8) as u8,
        default_rgb as u8,
    );
    let (r, g, b) = color.resolve_rgb(default);
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use retroglyph_core::color::Color;
    use retroglyph_core::grid::{Pos, Size};
    use retroglyph_core::style::Style;

    fn test_renderer() -> SoftwareRenderer {
        SoftwareBackendBuilder::new()
            .font(retroglyph_font::unscii16::FONT)
            .grid_size(1, 1)
            .scale(1)
            .build()
            .unwrap()
            .run_headless()
            .unwrap()
    }

    #[test]
    fn layer0_paints_background() {
        let mut renderer = test_renderer();
        let tile = Tile::new(' ', Style::new().bg(Color::Rgb { r: 255, g: 0, b: 0 }));
        let diff: Vec<(u8, Pos, &Tile, Option<&str>)> = vec![(0, Pos::new(0, 0), &tile, None)];
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

        let bg_tile = Tile::new(' ', Style::new().bg(Color::Rgb { r: 255, g: 0, b: 0 }));
        let space_tile = Tile::new(' ', Style::new().fg(Color::Rgb { r: 0, g: 255, b: 0 }));
        // draw_layers clears buffer first, so pass all layers in one call.
        renderer.draw_layers(
            [
                (0, Pos::new(0, 0), &bg_tile, None),
                (1, Pos::new(0, 0), &space_tile, None),
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

        let bg = Tile::new(
            ' ',
            Style::new().bg(Color::Rgb {
                r: 10,
                g: 10,
                b: 10,
            }),
        );
        let fg = Tile::new('@', Style::new().fg(Color::Rgb { r: 0, g: 255, b: 0 }));
        // draw_layers clears buffer first, so pass all layers in one call.
        renderer.draw_layers(
            [
                (0, Pos::new(0, 0), &bg, None),
                (1, Pos::new(0, 0), &fg, None),
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
                .fg(Color::Rgb { r: 0, g: 255, b: 0 })
                .bg(Color::Rgb { r: 255, g: 0, b: 0 }),
        );
        // Occupied (non-empty, via the `Tile::new` builder) space with no explicit
        // background: `Color::Default`.
        let occupied_default_bg = Tile::new(' ', Style::default());
        // draw_layers clears buffer first, so pass all layers in one call.
        renderer.draw_layers(
            [
                (0, Pos::new(0, 0), &glyph_on_red, None),
                (1, Pos::new(0, 0), &occupied_default_bg, None),
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

        let bg = Tile::new(
            ' ',
            Style::new().bg(Color::Rgb {
                r: 10,
                g: 10,
                b: 10,
            }),
        );
        let fg =
            Tile::new('@', Style::new().fg(Color::Rgb { r: 0, g: 255, b: 0 })).with_offset(1, 0);
        // draw_layers clears buffer first, so pass all layers in one call.
        renderer.draw_layers(
            [
                (0, Pos::new(0, 0), &bg, None),
                (1, Pos::new(0, 0), &fg, None),
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
    fn blit_cell_respects_sub_cell_offset() {
        // `Output::draw` (the non-layered path used by `blit_cell`) must
        // apply `tile.dx()/dy()` the same way `draw_layers` does.
        let mut renderer = test_renderer();

        let fg =
            Tile::new('@', Style::new().fg(Color::Rgb { r: 0, g: 255, b: 0 })).with_offset(1, 0);
        Output::draw(&mut renderer, [(Pos::new(0, 0), &fg, None)].into_iter()).unwrap();

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
    fn pixel_snapshot_render_scene() {
        // Render a small multi-layer scene and snapshot the pixel output.
        let opts = SoftwareBackendBuilder::new()
            .grid_size(2, 2)
            .scale(1)
            .build()
            .unwrap();
        let mut renderer = opts.run_headless().unwrap();

        // Layer 0: dark background, ':' at (0,0) in dim blue, '.' at (1,0) in dim gray.
        let bg = Tile::new(
            ':',
            Style::new()
                .fg(Color::Rgb {
                    r: 60,
                    g: 60,
                    b: 80,
                })
                .bg(Color::Rgb {
                    r: 20,
                    g: 20,
                    b: 30,
                }),
        );
        let dot = Tile::new(
            '.',
            Style::new()
                .fg(Color::Rgb {
                    r: 40,
                    g: 40,
                    b: 50,
                })
                .bg(Color::Rgb {
                    r: 20,
                    g: 20,
                    b: 30,
                }),
        );
        let entity = Tile::new(
            '@',
            Style::new()
                .fg(Color::Rgb { r: 0, g: 255, b: 0 })
                .bg(Color::Rgb {
                    r: 10,
                    g: 10,
                    b: 10,
                }),
        )
        .with_offset(1, 0);
        // Single draw_layers call (clears buffer first).
        renderer.draw_layers(
            [
                (0, Pos::new(0, 0), &bg, None),
                (0, Pos::new(1, 0), &dot, None),
                (1, Pos::new(0, 0), &entity, None),
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
        let mut renderer = opts.run_headless().unwrap();

        let base = Tile::new(
            ' ',
            Style::new().bg(Color::Rgb {
                r: 20,
                g: 20,
                b: 20,
            }),
        );
        // Layer 1 overlay: an opaque space (non-empty) with a red background.
        let overlay = Tile::new(' ', Style::new().bg(Color::Rgb { r: 200, g: 0, b: 0 }));
        // Layer 1 empty cell (default tile) must be skipped.
        let empty = Tile::default();

        renderer.draw_layers(
            [
                (0, Pos::new(0, 0), &base, None),
                (0, Pos::new(1, 0), &base, None),
                (1, Pos::new(0, 0), &overlay, None),
                (1, Pos::new(1, 0), &empty, None),
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
            .run_headless()
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
        let mut items: Vec<(u8, Pos, &Tile, Option<&str>)> = Vec::new();
        for y in 0..rows {
            for x in 0..cols {
                let t = match over {
                    Some((ox, oy, ot)) if ox == x && oy == y => ot,
                    _ => tile,
                };
                items.push((0, Pos::new(x, y), t, None));
            }
        }
        r.draw_layers(items.into_iter()).unwrap();
    }

    fn bg_tile(r: u8, g: u8, b: u8) -> Tile {
        Tile::new(' ', Style::new().bg(Color::Rgb { r, g, b }))
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
        draw_fill(&mut r, 2, 3, &red, None); // identical redraw: nothing changed
        assert_eq!(r.ctx.damage_rows, None);
        // Headless present() is a no-op and must still succeed.
        assert!(r.present().is_ok());
    }

    #[test]
    fn damage_band_is_tight_for_a_localized_change() {
        let mut r = damage_renderer(2, 3);
        let red = bg_tile(200, 0, 0);
        draw_fill(&mut r, 2, 3, &red, None); // baseline
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
        let mut items: Vec<(u8, Pos, &Tile, Option<&str>)> = Vec::new();
        for y in 0..3u16 {
            for x in 0..2u16 {
                let t = if (x, y) == (0, 0) || (x, y) == (1, 2) {
                    &blue
                } else {
                    &red
                };
                items.push((0, Pos::new(x, y), t, None));
            }
        }
        r.draw_layers(items.into_iter()).unwrap();
        assert_eq!(r.ctx.damage_rows, Some((0, 3 * CELL_H_PX)));
    }

    #[test]
    fn resize_marks_full_frame_damage() {
        let mut r = damage_renderer(2, 3);
        let red = bg_tile(200, 0, 0);
        draw_fill(&mut r, 2, 3, &red, None);
        draw_fill(&mut r, 2, 3, &red, None);
        assert_eq!(r.ctx.damage_rows, None);
        // A resize invalidates the shadow buffer and forces a full repaint so
        // no stale pixels survive at the new size.
        r.resize(Size {
            width: 4,
            height: 5,
        });
        assert_eq!(r.ctx.damage_rows, Some((0, 5 * CELL_H_PX)));
    }

    // ── Dirty-cell repaint (retroglyph#302) ──────────────────────────────
    //
    // `draw_layers` always receives every cell (see `Output::needs_full_frame`), but internally
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
                    Style::new().fg(Color::Rgb { r: 0, g: 255, b: 0 }).bg(bg)
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
            (0u8, pos, t, None)
        });
        r.draw_layers(content).unwrap();
    }

    #[test]
    fn dirty_cell_repaint_leaves_unchanged_cell_pixels_untouched() {
        // 3x1 grid, all cells the same dark background, distinct glyphs so a stray repaint of an
        // untouched cell would be visible. Change only the middle cell's glyph and re-draw; the
        // other two cells' pixels must be byte-for-byte identical to the first frame.
        let mut r = damage_renderer(3, 1);
        let base = glyph_scene(
            3,
            1,
            Color::Rgb {
                r: 10,
                g: 10,
                b: 10,
            },
            (1, 0),
            '@',
        );
        draw_scene(&mut r, 3, &base);
        let before = r.pixels().to_vec();

        let mut changed = base.clone();
        changed[1] = Tile::new(
            '#',
            Style::new()
                .fg(Color::Rgb { r: 0, g: 0, b: 255 })
                .bg(Color::Rgb {
                    r: 10,
                    g: 10,
                    b: 10,
                }),
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
        let base = glyph_scene(2, 1, Color::Rgb { r: 0, g: 0, b: 0 }, (0, 0), ' ');
        draw_scene(&mut r, 2, &base);

        let mut changed = base;
        changed[0] = Tile::new(' ', Style::new().bg(Color::Rgb { r: 200, g: 0, b: 0 }));
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
        // the fallback repaint path runs regardless of the dirty set -- assert the buffer is
        // still correct (not that any particular code path ran).
        let mut r = damage_renderer(2, 1);
        let bg = Tile::new(' ', Style::new().bg(Color::Rgb { r: 5, g: 5, b: 5 }));
        let offset_fg =
            Tile::new('@', Style::new().fg(Color::Rgb { r: 0, g: 255, b: 0 })).with_offset(1, 0);

        let draw = |r: &mut SoftwareRenderer| {
            r.draw_layers(
                [
                    (0, Pos::new(0, 0), &bg, None),
                    (1, Pos::new(0, 0), &offset_fg, None),
                    (0, Pos::new(1, 0), &bg, None),
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
        // allocated with an opaque background -- the layer-set change must not be missed by the
        // dirty-cell path (layer 0's own cell never changed, so a naive per-cell diff limited to
        // previously-seen layers would skip it).
        let mut r = damage_renderer(1, 1);
        let base = Tile::new(
            ' ',
            Style::new().bg(Color::Rgb {
                r: 10,
                g: 10,
                b: 10,
            }),
        );
        r.draw_layers(core::iter::once((0, Pos::new(0, 0), &base, None)))
            .unwrap();

        let overlay = Tile::new(' ', Style::new().bg(Color::Rgb { r: 200, g: 0, b: 0 }));
        r.draw_layers(
            [
                (0, Pos::new(0, 0), &base, None),
                (1, Pos::new(0, 0), &overlay, None),
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
}
