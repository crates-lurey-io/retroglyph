//! GPU rendering backend for retroglyph: native OpenGL 3.3 core and browser WebGL2, from a single
//! codebase via [`glow`].
//!
//! # Architecture
//!
//! [`GlBackendBuilder`] holds configuration (fonts, grid size, integer scale) and
//! [`build`](GlBackendBuilder::build)s a [`GlRenderer`]. The glyph source is a static
//! [`FontChain`] (a single [`BitmapFont`] is a chain of one); every font in the chain is
//! grid-packed into one `TEXTURE_2D_ARRAY` atlas and addressed by a flat slot id (issue #367's
//! grid-packing half, lifting the 256-layer cap). The renderer maintains
//! per-layer CPU-side instance arrays (one entry per cell: glyph slot + fg/bg RGB + flags) and a GL
//! context that is created lazily when the windowing loop calls
//! [`Presenter::init_surface`]:
//!
//! ```text
//! GlBackendBuilder (font, grid size, scale)
//!   |  .build()
//!   v
//! GlRenderer
//!   implements retroglyph_window::Presenter (an Output supertrait)
//!   wrapped by retroglyph_window::WindowBackend to become a full Backend
//!   (WindowBackend owns the input event queue and the no-op Cursor)
//!   |
//!   |  init_surface(window) -> GlContext (glutin native / WebGL2 wasm) + GlResources
//!   v
//! two instanced passes (backgrounds, then coverage-blended glyphs) per grid layer per present():
//! a unit quad instanced cols*rows times, sampling an R8 glyph atlas (TEXTURE_2D_ARRAY).
//! ```
//!
//! This backend composites grid layers itself on the GPU
//! ([`composites_layers`](retroglyph_core::backend::Output::composites_layers) returns `true`): it
//! receives the raw layered stream from the core `Terminal` and draws each layer back-to-front, so
//! an empty cell in a higher layer lets the layer beneath show through while an occupied cell is
//! opaque (issue #368), matching `retroglyph-software`'s per-pixel occlusion. It requests full
//! frames
//! ([`needs_full_frame`](retroglyph_core::backend::Output::needs_full_frame) returns `true`) and
//! redraws every cell of every layer each frame, so there is no orphaned-pixel problem from
//! sub-cell glyph spill.
//!
//! # Platform split
//!
//! Native builds create the GL context from the window's raw handles via `glutin`
//! (`context_native.rs`); wasm builds acquire a WebGL2 context from the winit `<canvas>`
//! (`context_wasm.rs`). Both expose the same internal `GlContext` API, so the renderer body has no
//! `cfg`.

#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod config;

mod atlas;
mod error;
mod glyphs;
mod renderer;
mod shaders;
#[cfg(feature = "tilesets")]
mod sprites;

// Headless offscreen render tests: create an EGL surfaceless context, run the real pipeline into an
// FBO, and read the pixels back to assert on them (issue #376). Linux/EGL only (see the module
// docs) and gated to `default-font` since the tests build a renderer from the embedded atlas.
#[cfg(all(test, target_os = "linux", feature = "default-font"))]
mod headless;

// Live WebGL2 render smoke test (issue #370): the browser sibling of `headless`, run under
// `wasm-bindgen-test` in headless Chrome (`just test-wasm-gl`). Gated to wasm32 + `default-font`.
#[cfg(all(test, target_arch = "wasm32", feature = "default-font"))]
mod webgl_smoke;

// Live WebGL2 context-loss recovery test (issue #373): forces a real lost/restored cycle via the
// `WEBGL_lose_context` extension and asserts the pipeline rebuilds and renders again. Same gating
// and `just test-wasm-gl` runner as `webgl_smoke`.
#[cfg(all(test, target_arch = "wasm32", feature = "default-font"))]
mod webgl_recovery;

// Platform-specific GL context, swapped by target. Both expose the same `GlContext` API (see the
// module docs), the same pattern `retroglyph-software` uses for its window surface.
#[cfg(not(target_arch = "wasm32"))]
#[path = "context_native.rs"]
mod context;
#[cfg(target_arch = "wasm32")]
#[path = "context_wasm.rs"]
mod context;

pub use config::{GlBackendBuilder, GlBackendError};
pub use error::SurfaceError;
// Re-export the font types so a consumer can build a custom atlas without a separate dependency.
pub use retroglyph_window::font::{self as font, BitmapFont, FontChain};

use context::GlContext;
use glyphs::GlyphCache;
use renderer::{FLAG_HAS_BG, FLAG_HAS_GLYPH, GlResources, Instance};
use retroglyph_core::backend::Output;
use retroglyph_core::color::Color;
use retroglyph_core::grid::{Pos, Size};
use retroglyph_core::tile::Tile;
use retroglyph_window::palette::{DEFAULT_BG, DEFAULT_FG};
use retroglyph_window::{CellGeometry, Presenter, WindowHandle};
use shaders::GlslFlavor;
#[cfg(feature = "tilesets")]
use sprites::{SpriteInstance, SpriteSet, SpriteSlot};
use std::sync::Arc;

// Compile the crate README's code blocks as doctests so the quick start can't silently rot.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

/// The live GL renderer: a [`Presenter`], wrapped in
/// [`WindowBackend`](retroglyph_window::WindowBackend) to form a full
/// [`Backend`](retroglyph_core::Backend) for the windowing loop.
///
/// It deliberately does not implement [`Input`](retroglyph_core::backend::Input) or
/// [`Cursor`](retroglyph_core::backend::Cursor) itself: a GL renderer cannot present without a
/// live context, so there is no headless-with-input use for a bare `Terminal<GlRenderer>`. In
/// windowed use `WindowBackend` owns the input queue (with its `Mouse(Moved)` coalescing) and the
/// no-op cursor, so a duplicate queue here would only ever be dead. See the sub-cell offset note
/// on [`Presenter`] for the shared rendering contract.
///
/// Build one with [`GlBackendBuilder`]. Before the windowing loop calls
/// [`init_surface`](Presenter::init_surface) there is no GL context; drawing updates only the
/// CPU-side instance array, and [`present`](Presenter::present) is a no-op. Once the surface
/// exists, `present` uploads changed cells and issues the single instanced draw call.
pub struct GlRenderer {
    /// Character-to-atlas-slot map for the bitmap font (grid-packed, issue #367).
    glyphs: GlyphCache,
    cols: u16,
    rows: u16,
    /// Cell/surface pixel geometry (glyph size x scale); the single source of the `cell_size`
    /// contract, delegated to by [`Presenter::cell_size`].
    geometry: CellGeometry,
    /// Atlas slot for the space glyph, used to initialize blank cells.
    space_glyph: u16,
    /// Per-layer instance arrays (index = grid layer id), each `cols * rows` in row-major cell
    /// order. `layers[0]` is the always-opaque base; higher layers composite over it back-to-front
    /// (see [`present`](Presenter::present)). Rebuilt each frame by [`Output::draw_layers`], since
    /// this backend requests full frames. There is always at least the base layer.
    layers: Vec<Vec<Instance>>,
    /// The decoded sprite atlas (issue #366), if a tileset was loaded. Retained so the GPU atlas
    /// can be rebuilt after a WebGL2 context loss.
    #[cfg(feature = "tilesets")]
    sprite_set: Option<SpriteSet>,
    /// Per-layer sprite instances, parallel to `layers`, rebuilt each frame by
    /// [`Output::draw_layers`]. Empty layers (or a renderer with no tileset) carry no sprites.
    #[cfg(feature = "tilesets")]
    sprite_layers: Vec<Vec<SpriteInstance>>,
    /// Glyphs already reported as needing a span, so a redraw loop logs each one once instead of
    /// every frame. See `retroglyph_window::sprite_cache::warn_sprite_needs_span`.
    #[cfg(feature = "tilesets")]
    warned_oversized: std::collections::BTreeSet<char>,
    /// The current surface size in physical pixels (set by [`resize_surface`](Presenter::resize_surface)).
    surface_size: (u32, u32),
    /// GL context + resources. `None` until [`init_surface`](Presenter::init_surface).
    gpu: Option<Gpu>,
}

/// The live GL context and its resources, present only after
/// [`init_surface`](Presenter::init_surface).
struct Gpu {
    ctx: GlContext,
    res: GlResources,
}

impl GlRenderer {
    /// Builds a renderer for the given glyph cache, grid size, and scale. Called by
    /// [`GlBackendBuilder::build`].
    ///
    /// Glyph cells wider or taller than 255 unscaled pixels are clamped to 255 (the
    /// [`CellGeometry`] limit).
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn new(glyphs: GlyphCache, cols: u16, rows: u16, scale: u16) -> Self {
        let (cell_w, cell_h) = glyphs.cell_size();
        let geometry = CellGeometry::new(cell_w.min(255) as u8, cell_h.min(255) as u8, scale);
        let space_glyph = glyphs.space_slot();
        let count = usize::from(cols) * usize::from(rows);
        let base = base_blank(space_glyph);
        let layers = vec![vec![base; count]];
        Self {
            glyphs,
            cols,
            rows,
            geometry,
            space_glyph,
            layers,
            #[cfg(feature = "tilesets")]
            sprite_set: None,
            #[cfg(feature = "tilesets")]
            sprite_layers: Vec::new(),
            #[cfg(feature = "tilesets")]
            warned_oversized: std::collections::BTreeSet::new(),
            surface_size: geometry.surface_size(cols, rows),
            gpu: None,
        }
    }

    /// Attaches a decoded sprite atlas (issue #366). Called by [`GlBackendBuilder::build`] when a
    /// tileset was registered; the GPU atlas is built later in [`build_resources`](Self::build_resources).
    #[cfg(feature = "tilesets")]
    pub(crate) fn set_sprites(&mut self, set: SpriteSet) {
        self.sprite_set = Some(set);
    }

    /// The base-layer blank instance: space glyph, default colors, opaque default background, no
    /// glyph drawn. Layer 0 always paints its background (the opaque base), so an untouched base
    /// cell is the default background.
    const fn base_blank(&self) -> Instance {
        base_blank(self.space_glyph)
    }

    /// Reports a sprite drawn larger than one cell without a span to reserve the cells it covers.
    ///
    /// Shares `retroglyph-window`'s diagnostic with the software backend so both name the same
    /// fix. A tile that already declares a span is fine and says nothing.
    #[cfg(feature = "tilesets")]
    fn warn_if_sprite_needs_span(&mut self, tile: &Tile, sprite: SpriteSlot) {
        if tile.span() != (1, 1) {
            return;
        }
        retroglyph_window::sprite_cache::warn_sprite_needs_span(
            &mut self.warned_oversized,
            tile.glyph(),
            (u32::from(sprite.w), u32::from(sprite.h)),
            (
                u32::from(self.geometry.glyph_w),
                u32::from(self.geometry.glyph_h),
            ),
        );
    }

    /// Total cell count for the current grid.
    fn cell_count(&self) -> usize {
        usize::from(self.cols) * usize::from(self.rows)
    }

    /// Writes one tile into the base layer's instance array at `pos`, if in bounds. Used by the
    /// single-layer [`Output::draw`] path; the multi-layer compositing path goes through
    /// [`draw_layers`](Output::draw_layers).
    fn write_tile(&mut self, pos: Pos, tile: &Tile) {
        let (x, y) = (usize::from(pos.x), usize::from(pos.y));
        let cols = usize::from(self.cols);
        if x >= cols || y >= usize::from(self.rows) {
            return;
        }
        let idx = y * cols + x;
        let slot = self.glyphs.resolve(tile.glyph());
        self.layers[0][idx] = base_instance(slot, tile);
    }

    /// Builds the GL resources for the current instance array on an already-current context:
    /// compiles the program, uploads the glyph atlas and the full instance buffer, and sets the
    /// glyph-size and projection uniforms.
    ///
    /// Shared by [`Presenter::init_surface`] (windowed) and the headless render-test path so both
    /// exercise byte-for-byte the same setup: the point of the render tests is to catch a break
    /// in exactly this pipeline, so it must not diverge from the real one.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError::Init`] if a shader fails to compile or the program fails to link.
    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn build_resources(
        &self,
        gl: &glow::Context,
        flavor: GlslFlavor,
    ) -> Result<GlResources, SurfaceError> {
        let (w, h) = self.surface_size;
        let atlas = self.glyphs.initial_atlas();
        #[cfg_attr(not(feature = "tilesets"), allow(unused_mut))]
        let mut res = GlResources::new(gl, flavor, &atlas, self.cell_count())?;
        res.upload(gl, &self.layers[0]);
        let (cw, ch) = self.glyphs.cell_size();
        #[allow(clippy::cast_precision_loss)]
        res.set_glyph_size(gl, cw as f32, ch as f32);
        // Build the RGBA sprite atlas + program on the same context (issue #366).
        #[cfg(feature = "tilesets")]
        if let Some(set) = &self.sprite_set {
            res.attach_sprites(gl, flavor, set)?;
            #[allow(clippy::cast_precision_loss)]
            res.set_sprite_glyph_size(gl, cw as f32, ch as f32);
        }
        let (cell_w, cell_h) = self.geometry.cell_size();
        res.set_projection(
            gl,
            w as f32,
            h as f32,
            cell_w as f32,
            cell_h as f32,
            i32::from(self.cols),
        );
        Ok(res)
    }
}

/// `(u8, u8, u8)` -> `[u8; 3]`, for packing resolved colors into an [`Instance`].
const fn to_arr(rgb: (u8, u8, u8)) -> [u8; 3] {
    [rgb.0, rgb.1, rgb.2]
}

/// The base-layer blank instance for `space_glyph`: opaque default background, no glyph. Free
/// function so [`GlRenderer::new`] can build it before `self` exists.
const fn base_blank(space_glyph: u16) -> Instance {
    Instance::new(
        space_glyph,
        to_arr(DEFAULT_FG),
        to_arr(DEFAULT_BG),
        0,
        0,
        FLAG_HAS_BG,
    )
}

/// Builds the base-layer (layer 0) [`Instance`] for `tile` at the already-resolved atlas `slot`:
/// the background is always opaque (default-substituted), and the glyph is drawn only when the tile
/// is non-empty.
///
/// A `slot` of `None` is a character no font in the chain can draw, not even as the substituted
/// solid block; the cell keeps its background and draws no glyph, matching `retroglyph-software`.
const fn base_instance(slot: Option<u16>, tile: &Tile) -> Instance {
    let fg = to_arr(tile.style().foreground().resolve_rgb(DEFAULT_FG));
    let bg = to_arr(tile.style().background().resolve_rgb(DEFAULT_BG));
    let (slot, drawable) = match slot {
        Some(slot) => (slot, FLAG_HAS_GLYPH),
        None => (0, 0),
    };
    let flags = FLAG_HAS_BG | if tile.is_empty() { 0 } else { drawable };
    Instance::new(slot, fg, bg, tile.dx(), tile.dy(), flags)
}

// ── Output ───────────────────────────────────────────────────────────────────

impl Output for GlRenderer {
    // Drawing only touches CPU memory (the instance array); it never fails. GL failures surface
    // through `Presenter::present`'s `SurfaceError` instead.
    type Error = core::convert::Infallible;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (Pos, &'a Tile, Option<&'a str>)>,
    {
        for (pos, tile, _extra) in content {
            self.write_tile(pos, tile);
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u8, Pos, &'a Tile, Option<&'a str>)>,
    {
        // This backend requests full frames, so `content` is every cell of every allocated layer in
        // layer-major (0..=max) then row-major order (see `Grid::layers`). Rebuild the per-layer
        // arrays from scratch: reset the base to blanks and drop higher layers, growing them back
        // as the stream references them. Cells a layer doesn't stream stay transparent (flags == 0).
        let base = self.base_blank();
        let cell_count = self.cell_count();
        self.layers.truncate(1);
        let base_layer = &mut self.layers[0];
        if base_layer.len() == cell_count {
            base_layer.fill(base);
        } else {
            *base_layer = vec![base; cell_count];
        }

        // Per-cell running background, updated bottom-up as layers are processed. An occupied
        // higher-layer tile with a `Color::Default` background inherits this instead of being
        // transparent: matching `retroglyph-software`'s `resolve_bg_fill`, an occupied tile is
        // opaque and erases the glyph beneath it, repainting whichever background a lower layer
        // last established (down to layer 0's default). This relies on the layer-major stream order
        // above, so a layer's lower neighbours are always processed first.
        let mut inherited_bg = vec![to_arr(DEFAULT_BG); cell_count];

        // Sprite instances are collected per layer in lockstep with `self.layers` (issue #366):
        // reset to just the (empty) base layer; higher layers are grown alongside `self.layers`.
        #[cfg(feature = "tilesets")]
        {
            self.sprite_layers.truncate(1);
            if self.sprite_layers.is_empty() {
                self.sprite_layers.push(Vec::new());
            }
            self.sprite_layers[0].clear();
        }

        let cols = usize::from(self.cols);
        let rows = usize::from(self.rows);
        for (layer_id, pos, tile, _extra) in content {
            let (x, y) = (usize::from(pos.x), usize::from(pos.y));
            if x >= cols || y >= rows {
                continue;
            }
            let l = usize::from(layer_id);
            while self.layers.len() <= l {
                // Higher layers default to fully transparent cells (flags == 0).
                self.layers.push(vec![
                    Instance::new(self.space_glyph, [0; 3], [0; 3], 0, 0, 0);
                    cell_count
                ]);
                #[cfg(feature = "tilesets")]
                self.sprite_layers.push(Vec::new());
            }
            let idx = y * cols + x;

            // A cell whose glyph has a sprite draws the sprite instead of a bitmap glyph (issue
            // #366); the glyph instance keeps only the background (per `resolve_bg_fill`).
            #[cfg(feature = "tilesets")]
            #[allow(clippy::cast_possible_truncation)]
            let (cx, cy) = (x as u16, y as u16);

            // A cell covered by a multi-cell span (retroglyph#412) draws no glyph of its own: the
            // span's anchor emitted one sprite across the whole footprint, and this cell's glyph
            // is that sprite's text fallback, for backends that can't draw it. It takes the
            // anchor's background so the footprint sits on one uniform backdrop. The stream is
            // row-major within a layer, so the anchor's instance is always already written.
            if let Some((back_x, back_y)) = tile.span_offset() {
                let anchor = idx
                    .checked_sub(usize::from(back_y) * cols + usize::from(back_x))
                    .and_then(|anchor_idx| self.layers[l].get(anchor_idx).copied());
                if let Some(anchor) = anchor {
                    let covered = Instance::new(
                        self.space_glyph,
                        anchor.fg,
                        anchor.bg,
                        0,
                        0,
                        anchor.flags & FLAG_HAS_BG,
                    );
                    if covered.flags & FLAG_HAS_BG != 0 {
                        inherited_bg[idx] = covered.bg;
                    }
                    self.layers[l][idx] = covered;
                    continue;
                }
            }

            if layer_id == 0 {
                let slot = self.glyphs.resolve(tile.glyph());
                let inst = base_instance(slot, tile);
                #[cfg(feature = "tilesets")]
                if let Some(sprite) = self.sprite_set.as_ref().and_then(|s| s.slot(tile.glyph())) {
                    // Keep layer 0's opaque background; drop the glyph, the sprite covers it.
                    let sprite_inst =
                        Instance::new(inst.glyph, inst.fg, inst.bg, 0, 0, inst.flags & FLAG_HAS_BG);
                    inherited_bg[idx] = sprite_inst.bg;
                    self.layers[0][idx] = sprite_inst;
                    let (span_w, span_h) = tile.span();
                    let align = sprite.align_offset(
                        span_w,
                        span_h,
                        self.geometry.glyph_w,
                        self.geometry.glyph_h,
                    );
                    self.warn_if_sprite_needs_span(tile, sprite);
                    self.sprite_layers[0].push(SpriteInstance::new(
                        cx,
                        cy,
                        sprite.layer,
                        sprite.w,
                        sprite.h,
                        tile.dx() + align.0,
                        tile.dy() + align.1,
                    ));
                    continue;
                }
                inherited_bg[idx] = inst.bg;
                self.layers[0][idx] = inst;
                continue;
            }
            if tile.is_empty() {
                // Transparent: nothing drawn, and the running background is unchanged.
                self.layers[l][idx] = Instance::new(self.space_glyph, [0; 3], [0; 3], 0, 0, 0);
                continue;
            }
            // Occupied higher-layer tile: opaque background (own colour, or the inherited one when
            // the tile's background is `Default`) plus its glyph, unless no font in the chain can
            // draw that character at all (see `base_instance`).
            let resolved = self.glyphs.resolve(tile.glyph());
            let glyph = resolved.unwrap_or(0);
            let has_glyph = if resolved.is_some() {
                FLAG_HAS_GLYPH
            } else {
                0
            };
            let fg = to_arr(tile.style().foreground().resolve_rgb(DEFAULT_FG));
            let bg_color = tile.style().background();
            let bg = if bg_color == Color::Default {
                inherited_bg[idx]
            } else {
                let resolved = to_arr(bg_color.resolve_rgb(DEFAULT_BG));
                inherited_bg[idx] = resolved;
                resolved
            };
            #[cfg(feature = "tilesets")]
            if let Some(sprite) = self.sprite_set.as_ref().and_then(|s| s.slot(tile.glyph())) {
                // No bitmap glyph. An occupied higher-layer sprite cell with a `Default` background
                // paints no background (the sprite's own alpha provides coverage, so lower layers
                // show through its transparent pixels), matching `resolve_bg_fill`'s has_sprite
                // rule; an explicit background is still painted opaque.
                let has_bg = if bg_color == Color::Default {
                    0
                } else {
                    FLAG_HAS_BG
                };
                self.layers[l][idx] = Instance::new(glyph, fg, bg, 0, 0, has_bg);
                let (span_w, span_h) = tile.span();
                let align = sprite.align_offset(
                    span_w,
                    span_h,
                    self.geometry.glyph_w,
                    self.geometry.glyph_h,
                );
                self.warn_if_sprite_needs_span(tile, sprite);
                self.sprite_layers[l].push(SpriteInstance::new(
                    cx,
                    cy,
                    sprite.layer,
                    sprite.w,
                    sprite.h,
                    tile.dx() + align.0,
                    tile.dy() + align.1,
                ));
                continue;
            }
            self.layers[l][idx] =
                Instance::new(glyph, fg, bg, tile.dx(), tile.dy(), FLAG_HAS_BG | has_glyph);
        }
        Ok(())
    }

    fn needs_full_frame(&self) -> bool {
        // Composited layers plus sub-cell glyph spill mean a partial redraw could leave orphaned
        // pixels; redraw every cell of every layer each frame.
        true
    }

    fn composites_layers(&self) -> bool {
        // Draw the raw layered stream back-to-front on the GPU (issue #368) instead of letting the
        // core flatten it, so per-layer transparency works the same as on `retroglyph-software`.
        true
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        // Upload is deferred to `present`, which owns the GL context.
        Ok(())
    }

    fn size(&self) -> Size {
        Size {
            width: self.cols,
            height: self.rows,
        }
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        let base = self.base_blank();
        self.layers.truncate(1);
        for cell in &mut self.layers[0] {
            *cell = base;
        }
        Ok(())
    }

    fn resize(&mut self, size: Size) {
        self.cols = size.width;
        self.rows = size.height;
        let base = self.base_blank();
        self.layers = vec![vec![base; self.cell_count()]];
    }
}

// GlRenderer implements neither `Input` nor `Cursor`: `WindowBackend<GlRenderer>` supplies both
// for windowed use (its input queue coalesces `Mouse(Moved)`; its cursor is a no-op), and a GL
// renderer has no headless-with-input path that would need its own. See the type-level docs.

// ── Presenter ────────────────────────────────────────────────────────────────

impl Presenter for GlRenderer {
    type SurfaceError = SurfaceError;

    fn init_surface(&mut self, window: Arc<dyn WindowHandle>) -> Result<(), SurfaceError> {
        let (w, h) = self.surface_size;
        let ctx = GlContext::new(&window, w, h)?;
        let res = self.build_resources(&ctx.gl, ctx.flavor())?;
        self.gpu = Some(Gpu { ctx, res });
        Ok(())
    }

    fn resize_surface(&mut self, width: u32, height: u32) {
        self.surface_size = (width, height);
        if let Some(gpu) = &self.gpu {
            gpu.ctx.resize(width, height);
        }
    }

    #[allow(clippy::cast_precision_loss)]
    fn present(&mut self) -> Result<(), SurfaceError> {
        let cell_count = self.cell_count();
        let cols = i32::from(self.cols);
        let (w, h) = self.surface_size;
        let (cell_w, cell_h) = self.geometry.cell_size();
        let (cell_w, cell_h) = (cell_w as f32, cell_h as f32);

        // WebGL2 context-loss recovery (issue #373): if the context was lost and has since been
        // restored, every GL object (program, buffers, atlas texture) was invalidated, so rebuild
        // them on the now-live context before drawing. Always a no-op on native, where
        // `take_needs_rebuild` is `const false`. Taken out of `self.gpu` for the rebuild so
        // `build_resources`' `&self` borrow doesn't overlap the `&mut self.gpu` one.
        if self
            .gpu
            .as_ref()
            .is_some_and(|gpu| gpu.ctx.take_needs_rebuild())
        {
            let mut gpu = self.gpu.take().expect("is_some_and matched above");
            match self.build_resources(&gpu.ctx.gl, gpu.ctx.flavor()) {
                Ok(res) => {
                    gpu.res = res;
                    self.gpu = Some(gpu);
                }
                Err(e) => {
                    self.gpu = Some(gpu);
                    return Err(e);
                }
            }
        }

        // Split borrow: `gpu` borrows `self.gpu`, while `self.layers` is a disjoint field, so
        // direct field access to it stays legal below.
        let Some(gpu) = self.gpu.as_mut() else {
            // No surface yet: nothing to present.
            return Ok(());
        };

        // Keep the GPU instance buffer sized to the current grid (grid resizes arrive via
        // `Output::resize`, out of band from surface resizes).
        if gpu.res.capacity() != cell_count {
            gpu.res.resize_instances(&gpu.ctx.gl, cell_count);
        }

        gpu.res
            .set_projection(&gpu.ctx.gl, w as f32, h as f32, cell_w, cell_h, cols);
        // Composite every layer back-to-front: clear once, then upload and draw each layer's
        // instances in turn (issue #368). This backend requests full frames, so `self.layers`
        // already holds the whole current frame.
        gpu.res.clear(&gpu.ctx.gl);
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        for l in 0..self.layers.len() {
            gpu.res.upload(&gpu.ctx.gl, &self.layers[l]);
            gpu.res.draw_layer(&gpu.ctx.gl, cell_count as i32);
            // Sprite pass for this layer, over its glyph passes and source-over blended (issue
            // #366). Parallel to `self.layers`; a layer with no sprite cells draws nothing.
            #[cfg(feature = "tilesets")]
            if let Some(sprites) = self.sprite_layers.get(l) {
                gpu.res.draw_sprites(&gpu.ctx.gl, sprites);
            }
        }
        gpu.ctx.present()
    }

    fn cell_size(&self) -> (u32, u32) {
        self.geometry.cell_size()
    }
}

impl Drop for GlRenderer {
    fn drop(&mut self) {
        if let Some(gpu) = &self.gpu {
            gpu.res.delete(&gpu.ctx.gl);
        }
    }
}

#[cfg(all(test, feature = "default-font"))]
mod compositing_tests {
    use super::{FLAG_HAS_BG, FLAG_HAS_GLYPH};
    use crate::GlBackendBuilder;
    use retroglyph_core::backend::Output;
    use retroglyph_core::color::Color;
    use retroglyph_core::grid::Pos;
    use retroglyph_core::style::Style;
    use retroglyph_core::tile::Tile;

    const RED: Color = Color::Rgb { r: 255, g: 0, b: 0 };

    #[test]
    fn draw_records_sub_cell_offset_and_flags_in_the_base_layer() {
        let mut r = GlBackendBuilder::new()
            .grid_size(4, 2)
            .build()
            .expect("default-font builds");
        let tile = Tile::new('A', Style::new()).with_offset(-3, 5);
        r.draw(core::iter::once((Pos::new(1, 0), &tile, None)))
            .expect("draw is infallible");

        let inst = r.layers[0][1];
        assert_eq!(inst.dx, -3);
        assert_eq!(inst.dy, 5);
        let a_slot = r.glyphs.resolve('A').expect("'A' is in CP437");
        assert_eq!(inst.glyph, a_slot);
        // A non-empty tile on the base layer draws both its glyph and its (base) background.
        assert_eq!(inst.flags, FLAG_HAS_BG | FLAG_HAS_GLYPH);
    }

    #[test]
    fn base_layer_blank_cells_are_opaque_background_only() {
        let r = GlBackendBuilder::new()
            .grid_size(3, 3)
            .build()
            .expect("default-font builds");
        // Untouched base cells: opaque default background, no glyph, no offset.
        assert!(
            r.layers[0]
                .iter()
                .all(|i| i.dx == 0 && i.dy == 0 && i.flags == FLAG_HAS_BG)
        );
    }

    #[test]
    fn composites_layers_and_requests_full_frames() {
        let r = GlBackendBuilder::new()
            .grid_size(2, 1)
            .build()
            .expect("default-font builds");
        assert!(r.composites_layers());
        assert!(r.needs_full_frame());
    }

    #[test]
    fn draw_layers_encodes_the_occlusion_rule_per_layer() {
        let mut r = GlBackendBuilder::new()
            .grid_size(3, 1)
            .build()
            .expect("default-font builds");

        // Layer 0: an opaque glyph with a real background at (0,0).
        let base = Tile::new('X', Style::new().bg(RED));
        // Layer 1: (0,0) empty (transparent), (1,0) glyph with default bg (transparent bg),
        // (2,0) glyph with a real bg (opaque).
        let empty = Tile::default();
        let glyph_default_bg = Tile::new('Y', Style::new());
        let glyph_real_bg = Tile::new('Z', Style::new().bg(RED));
        let stream = [
            (0u8, Pos::new(0, 0), &base, None),
            (1u8, Pos::new(0, 0), &empty, None),
            (1u8, Pos::new(1, 0), &glyph_default_bg, None),
            (1u8, Pos::new(2, 0), &glyph_real_bg, None),
        ];
        r.draw_layers(stream.iter().copied())
            .expect("draw_layers is infallible");

        // Base layer cell 0 draws both.
        assert_eq!(r.layers[0][0].flags, FLAG_HAS_BG | FLAG_HAS_GLYPH);
        // A second layer was allocated.
        assert_eq!(r.layers.len(), 2);
        // Higher-layer empty cell: fully transparent (nothing drawn -> lower layer shows).
        assert_eq!(r.layers[1][0].flags, 0);
        // Higher-layer occupied cell with a Default background is opaque (it erases the glyph
        // beneath), inheriting the background from below: here the untouched base cell.
        assert_eq!(r.layers[1][1].flags, FLAG_HAS_BG | FLAG_HAS_GLYPH);
        assert_eq!(r.layers[1][1].bg, r.layers[0][1].bg);
        // Higher-layer glyph with a real background: both, with its own colour.
        assert_eq!(r.layers[1][2].flags, FLAG_HAS_BG | FLAG_HAS_GLYPH);
        assert_eq!(r.layers[1][2].bg, [255, 0, 0]);
    }

    #[test]
    fn draw_layers_full_frame_drops_a_removed_higher_layer() {
        let mut r = GlBackendBuilder::new()
            .grid_size(2, 1)
            .build()
            .expect("default-font builds");
        let tile = Tile::new('Q', Style::new());
        // Frame 1: two layers.
        r.draw_layers(core::iter::once((1u8, Pos::new(0, 0), &tile, None)))
            .expect("draw_layers");
        assert_eq!(r.layers.len(), 2);
        // Frame 2: only the base layer is streamed, so the higher layer must not linger.
        r.draw_layers(core::iter::once((0u8, Pos::new(0, 0), &tile, None)))
            .expect("draw_layers");
        assert_eq!(r.layers.len(), 1);
    }

    /// A span's covered cells draw no glyph of their own and take the anchor's background, so one
    /// piece of artwork sits on one uniform backdrop (retroglyph#412).
    #[test]
    fn draw_layers_gives_span_covered_cells_the_anchors_background_and_no_glyph() {
        use retroglyph_core::Grid;

        let mut r = GlBackendBuilder::new()
            .grid_size(3, 1)
            .build()
            .expect("default-font builds");

        let mut grid = Grid::new(3, 1);
        grid.write_span(0, 0, 0, &["C="], Style::new().bg(RED))
            .expect("2x1 span fits");
        let tiles: Vec<(u8, Pos, Tile)> = (0..3)
            .map(|x| (0u8, Pos::new(x, 0), *grid.tile(0, (x, 0)).unwrap()))
            .collect();
        r.draw_layers(tiles.iter().map(|(l, pos, t)| (*l, *pos, t, None)))
            .expect("draw_layers is infallible");

        let (anchor, covered, free) = (r.layers[0][0], r.layers[0][1], r.layers[0][2]);
        assert_eq!(anchor.flags, FLAG_HAS_BG | FLAG_HAS_GLYPH, "anchor draws");
        assert_eq!(covered.flags, FLAG_HAS_BG, "covered cell draws no glyph");
        assert_eq!(
            covered.bg, anchor.bg,
            "covered cell inherits the anchor's bg"
        );
        assert_eq!(covered.bg, [255, 0, 0]);
        // A cell outside the span is untouched by any of this.
        assert_eq!(free.flags, FLAG_HAS_BG);
        assert_ne!(free.bg, [255, 0, 0]);
    }

    /// Covered-cell suppression is grid state, not a tileset feature, so it holds with the
    /// `tilesets` feature off too: a span with no sprite behind it renders as its anchor glyph
    /// alone, the same on both pixel backends.
    #[test]
    fn draw_layers_suppresses_covered_glyphs_without_a_sprite() {
        use retroglyph_core::Grid;

        let mut r = GlBackendBuilder::new()
            .grid_size(2, 1)
            .build()
            .expect("default-font builds");
        let mut grid = Grid::new(2, 1);
        grid.write_span(0, 0, 0, &["AB"], Style::new()).unwrap();
        let tiles: Vec<(u8, Pos, Tile)> = (0..2)
            .map(|x| (0u8, Pos::new(x, 0), *grid.tile(0, (x, 0)).unwrap()))
            .collect();
        r.draw_layers(tiles.iter().map(|(l, pos, t)| (*l, *pos, t, None)))
            .expect("draw_layers is infallible");

        assert_eq!(r.layers[0][0].flags & FLAG_HAS_GLYPH, FLAG_HAS_GLYPH);
        assert_eq!(r.layers[0][1].flags & FLAG_HAS_GLYPH, 0);
    }
}
