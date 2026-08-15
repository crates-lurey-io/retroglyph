//! GPU rendering backend for retroglyph: native OpenGL 3.3 core and browser WebGL2, from a single
//! codebase via [`glow`].
//!
//! # Architecture
//!
//! [`config::GlBackendBuilder`] holds configuration (fonts, grid size, integer scale) and
//! [`build`](config::GlBackendBuilder::build)s a [`GlRenderer`]. The glyph source is a static
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
//!   implements retroglyph_window::presenter::Presenter (an Output supertrait)
//!   wrapped by retroglyph_window::backend::WindowBackend to become a full Backend
//!   (WindowBackend owns the input event queue and the no-op Cursor)
//!   |
//!   |  init_surface(window) -> GlContext (glutin native / WebGL2 wasm) + GlResources
//!   v
//! two instanced passes (backgrounds, then coverage-blended glyphs) per grid layer per present():
//! a unit quad instanced cols*rows times, sampling an R8 glyph atlas (TEXTURE_2D_ARRAY).
//! ```
//!
//! This backend composites grid layers itself on the GPU
//! ([`compositing`](retroglyph_core::backend::Output::compositing) returns
//! [`retroglyph_core::backend::Compositing::PixelLayered`]): it
//! receives the raw layered stream from the core `Terminal` and draws each layer back-to-front, so
//! an empty cell in a higher layer lets the layer beneath show through while an occupied cell is
//! opaque (issue #368), matching `retroglyph-software`'s per-pixel occlusion. It requests full
//! frames (`needs_full_frame: true`) and redraws every cell of every layer each frame, so there
//! is no orphaned-pixel problem from sub-cell glyph spill.
//!
//! # Platform split
//!
//! Native builds create the GL context from the window's raw handles via `glutin`
//! (`context_native.rs`); wasm builds acquire a WebGL2 context from the winit `<canvas>`
//! (`context_wasm.rs`). Both expose the same internal `GlContext` API, so the renderer body has no
//! `cfg`.
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
//! Embeds the Unscii 16 default font so a caller can build a renderer with no font of its own.
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
//! PNG sprite/tileset support (issue #366): decodes sprite sheets into an RGBA `TEXTURE_2D_ARRAY`
//! atlas and draws them in a second, source-over blended pass.
//!
//! Forwards to `retroglyph-window`'s shared tileset decode, and (Linux only, where it's a
//! dependency at all) to the `retroglyph-software` dev-dependency's own `tilesets`, so the two
//! stay in lockstep: without this, `cargo test -p retroglyph-gl` (this feature off) still pulls in
//! `retroglyph-window/tilesets` transitively through that dev-dependency's forced-on `tilesets`
//! below, and the `PresenterBuilder` impl's `tileset` method (gated on this crate's own `tilesets`
//! feature, matching every other tileset-gated item in this crate) would then be missing an item
//! the trait requires whenever `retroglyph-window/tilesets` is on, regardless of this crate's own
//! flag (retroglyph#1192). Harmless outside a test build: `retroglyph-software` is dev-only, so
//! this half of the forward is a no-op for a plain `cargo build`/`check`.
//! <!-- gen-features:end -->

#![doc(
    html_logo_url = "https://raw.githubusercontent.com/crates-lurey-io/retroglyph/main/docs/public/assets/logo.svg"
)]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/crates-lurey-io/retroglyph/main/docs/public/assets/logo.svg"
)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod config;

pub mod error;
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

// Re-exports a dependency's own public types so a consumer can build a custom atlas without a
// separate direct dependency on retroglyph-window (STYLE_GUIDE.md exception 3).
pub use retroglyph_window::font::{self as font, BitmapFont, FontChain};

use context::GlContext;
use error::SurfaceError;
use renderer::{FLAG_HAS_BG, FLAG_HAS_GLYPH, GlResources, Instance};
use retroglyph_core::backend::Compositing;
use retroglyph_core::backend::DrawCell;
use retroglyph_core::backend::Output;
use retroglyph_core::color::Color;
use retroglyph_core::dev_only;
use retroglyph_core::grid::HasSize;
use retroglyph_core::grid::Size;
use retroglyph_core::tile::Tile;
use retroglyph_window::atlas::GlyphAtlas;
use retroglyph_window::geometry::CellGeometry;
use retroglyph_window::palette::{DEFAULT_BG, DEFAULT_FG};
use retroglyph_window::presenter::{Presenter, WindowHandle, cell_art_glyph};
#[cfg(feature = "tilesets")]
use retroglyph_window::sprite_cache::SpriteTint;
use shaders::GlslFlavor;
#[cfg(feature = "tilesets")]
use sprites::{SpriteInstance, SpriteSet, SpriteSlot};
use std::sync::Arc;

// Compile the crate README's code blocks as doctests so the quick start can't silently rot.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

/// The live GL renderer: a [`Presenter`], wrapped in
/// [`WindowBackend`](retroglyph_window::backend::WindowBackend) to form a full
/// [`Backend`](retroglyph_core::backend::Backend) for the windowing loop.
///
/// It does not implement [`Input`](retroglyph_core::backend::Input) or
/// [`Cursor`](retroglyph_core::backend::Cursor) itself: a GL renderer cannot present without a
/// live context, so there is no headless-with-input use for a bare `Terminal<GlRenderer>`. In
/// windowed use `WindowBackend` owns the input queue (with its `Mouse(Moved)` coalescing) and the
/// no-op cursor, so a duplicate queue here would only ever be dead. See the sub-cell offset note
/// on [`Presenter`] for the shared rendering contract.
///
/// Build one with [`config::GlBackendBuilder`]. Before the windowing loop calls
/// [`init_surface`](Presenter::init_surface) there is no GL context; drawing updates only the
/// CPU-side instance array, and [`present`](Presenter::present) is a no-op. Once the surface
/// exists, `present` uploads changed cells and issues the single instanced draw call.
pub struct GlRenderer {
    /// Character-to-atlas-slot map for the bitmap font (grid-packed, issue #367).
    glyphs: GlyphAtlas,
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
    /// Glyphs already reported as having a dropped tint, so a redraw loop logs each one once
    /// instead of every frame. See `retroglyph_window::sprite_cache::warn_tint_needs_sprite`.
    #[cfg(feature = "tilesets")]
    warned_dropped_tint: std::collections::BTreeSet<char>,
    /// Characters already reported as resolving to the atlas's notdef fallback rather than their
    /// own glyph, so a redraw loop logs each one once instead of every frame. See
    /// `retroglyph_window::diagnostics::warn_notdef_glyph`.
    warned_notdef: std::collections::BTreeSet<char>,
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
    /// [`config::GlBackendBuilder::build`].
    ///
    /// Glyph cells wider or taller than 255 unscaled pixels are clamped to 255 (the
    /// [`CellGeometry`] limit).
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn new(glyphs: GlyphAtlas, cols: u16, rows: u16, scale: u16) -> Self {
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
            #[cfg(feature = "tilesets")]
            warned_dropped_tint: std::collections::BTreeSet::new(),
            warned_notdef: std::collections::BTreeSet::new(),
            surface_size: geometry.surface_size(cols, rows),
            gpu: None,
        }
    }

    /// Attaches a decoded sprite atlas (issue #366). Called by
    /// [`config::GlBackendBuilder::build`] when a tileset was registered; the GPU atlas is built
    /// later in [`build_resources`](Self::build_resources).
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
        if tile.is_span_anchor() {
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

    /// Reports a tint set on a cell whose glyph resolved to a bitmap font rather than a sprite, so
    /// the tint was silently dropped (retroglyph#564).
    ///
    /// Shares `retroglyph-window`'s diagnostic with the software backend so both name the same
    /// fix. Called from the branch that already knows the sprite atlas has no slot for this
    /// glyph; a tint on a cell that does resolve to a sprite is handled, not dropped, and says
    /// nothing here.
    #[cfg(feature = "tilesets")]
    fn warn_if_tint_needs_sprite(&mut self, glyph: char, tint: retroglyph_core::color::Tint) {
        retroglyph_window::sprite_cache::warn_tint_needs_sprite(
            &mut self.warned_dropped_tint,
            glyph,
            tint,
        );
    }

    /// Pushes one sprite instance for `tile` on layer `l` at cell `(cx, cy)`: aligns it within its
    /// span box, warns once if it needed a span but didn't declare one, and resolves its tint
    /// against `sprite`'s sheet color. Shared verbatim between the layer-0 and higher-layer sprite
    /// dispatch branches in `draw_layers` (retroglyph#1374): the CPU/GPU parity contract (span
    /// alignment, oversize warning, `SpriteTint::resolve` argument order) lives in exactly one
    /// place, so a fix here reaches every layer instead of needing to land twice.
    ///
    /// The caller still owns which `Instance` is written and how `inherited_bg`/`sprite_bg` update;
    /// this only appends to `self.sprite_layers[l]`.
    #[cfg(feature = "tilesets")]
    fn emit_sprite(
        &mut self,
        l: usize,
        cx: u16,
        cy: u16,
        tile: &Tile,
        sprite: SpriteSlot,
        tint: retroglyph_core::color::Tint,
    ) {
        let (span_w, span_h) = tile.span();
        let align =
            sprite.align_offset(span_w, span_h, self.geometry.glyph_w, self.geometry.glyph_h);
        self.warn_if_sprite_needs_span(tile, sprite);
        self.sprite_layers[l].push(SpriteInstance::new(
            cx,
            cy,
            sprite.layer,
            sprite.w,
            sprite.h,
            tile.dx() + align.0,
            tile.dy() + align.1,
            SpriteTint::resolve(sprite.color, tile.style().foreground(), tint, DEFAULT_FG),
        ));
    }

    /// Reports a character that resolved to the atlas's substituted "not defined" glyph rather
    /// than its own shape: a legitimate cell on its own (a solid block can be drawn on purpose),
    /// so this is the only place a caller finds out no font in the chain actually covers `ch`
    /// (retroglyph#1292).
    ///
    /// Shares `retroglyph-window`'s diagnostic with the software backend so both name the same
    /// fix. `self.glyphs.resolve` returns a flat slot with no notdef bit, so unlike the software
    /// backend (which already has a `ResolvedGlyph` in hand from the resolve it needs anyway for
    /// rendering) this re-resolves `ch` through [`GlyphAtlas::is_notdef`] to find out. The whole
    /// check sits inside [`dev_only!`], so a release build pays for neither call.
    fn warn_if_notdef_glyph(&mut self, ch: char) {
        dev_only!({
            if self.glyphs.is_notdef(ch) {
                retroglyph_window::diagnostics::warn_notdef_glyph(&mut self.warned_notdef, ch);
            }
        });
    }

    /// Total cell count for the current grid.
    fn cell_count(&self) -> usize {
        usize::from(self.cols) * usize::from(self.rows)
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
    /// Returns [`error::SurfaceError::Init`] if a shader fails to compile or the program fails to
    /// link.
    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn build_resources(
        &self,
        gl: &glow::Context,
        flavor: GlslFlavor,
    ) -> Result<GlResources, SurfaceError> {
        let (w, h) = self.surface_size;
        let atlas = self.glyphs.data();
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
/// the background is always opaque (default-substituted), and the glyph is drawn only when
/// [`cell_art_glyph`] says this tile draws art (see its docs for the blank/span-covered rules).
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
    let flags = FLAG_HAS_BG
        | if cell_art_glyph(tile).is_none() {
            0
        } else {
            drawable
        };
    Instance::new(slot, fg, bg, tile.dx(), tile.dy(), flags)
}

// ── Output ───────────────────────────────────────────────────────────────────

impl Output for GlRenderer {
    // Drawing only touches CPU memory (the instance array); it never fails. GL failures surface
    // through `Presenter::present`'s `SurfaceError` instead.
    type Error = core::convert::Infallible;

    // No `draw` override: this backend always composites (`compositing` returns `PixelLayered`
    // below), so `Terminal::present` never calls single-layer `draw` and the default
    // implementation (forwards to `draw_layers`) is exactly right. See retroglyph#561; this used
    // to have its own `write_tile`-based body that wrote glyph instances only and silently never
    // read a cell's tint, which is exactly the kind of drift a dead, hand-maintained second
    // implementation invites.
    #[allow(clippy::too_many_lines)]
    fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = DrawCell<'a>>,
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

        // Per-cell record of whether the occupant drawn at that index dispatched to a sprite
        // (issue #366), keyed the same way as `inherited_bg`. A span's covered cells hold only a
        // text-fallback glyph that never has a sprite of its own, so the covered-cell branch below
        // consults this at the *anchor's* index to answer "does this span dispatch to a sprite",
        // matching `retroglyph-software`'s `resolve_cell_bg` (retroglyph#726). Reused across layers:
        // a lower layer's `true` is always overwritten before a higher layer's covered cell can read
        // it, because the anchor of any span is written before its covered cells (row-major stream).
        let mut sprite_bg = vec![false; cell_count];

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
        for draw_cell in content {
            let (layer_id, pos, tile) = (draw_cell.layer, draw_cell.pos, draw_cell.tile);
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
            // is that sprite's text fallback, for backends that can't draw it. Every cell of a span
            // shares one `Style` (see `Grid::write_span_cells`), so this cell's own tile already
            // carries the same colours as the anchor; the anchor is consulted only to answer "does
            // this span dispatch to a sprite" (via `sprite_bg`), the same split
            // `retroglyph-software`'s `resolve_cell_bg` documents. Resolving the running inherited
            // background at this cell's own index, not the anchor's, keeps a span from smearing one
            // column's inheritance across the whole footprint (retroglyph#726). The stream is
            // row-major within a layer, so the anchor is always already written.
            if tile.span_offset().is_some() {
                let anchor_idx = tile
                    .span_anchor_index(idx, cols)
                    .filter(|&anchor_idx| anchor_idx < cell_count);
                if let Some(anchor_idx) = anchor_idx {
                    let has_sprite = sprite_bg[anchor_idx];
                    let fg = to_arr(tile.style().foreground().resolve_rgb(DEFAULT_FG));
                    let bg_color = tile.style().background();
                    let (bg, has_bg) = if l == 0 || bg_color != Color::Default {
                        (to_arr(bg_color.resolve_rgb(DEFAULT_BG)), FLAG_HAS_BG)
                    } else if has_sprite {
                        (inherited_bg[idx], 0)
                    } else {
                        (inherited_bg[idx], FLAG_HAS_BG)
                    };
                    if has_bg != 0 {
                        inherited_bg[idx] = bg;
                    }
                    self.layers[l][idx] = Instance::new(self.space_glyph, fg, bg, 0, 0, has_bg);
                    continue;
                }
            }

            if layer_id == 0 {
                let slot = self.glyphs.resolve(tile.glyph());
                self.warn_if_notdef_glyph(tile.glyph());
                let inst = base_instance(slot, tile);
                // Sprite dispatch is gated on `cell_art_glyph`, not the raw `tile.glyph()`: a
                // blank layer-0 cell (`is_empty()`, e.g. an untouched grid cell) draws no art at
                // all, even if its glyph happens to have a registered sprite (retroglyph#762).
                #[cfg(feature = "tilesets")]
                {
                    let art_glyph = cell_art_glyph(tile);
                    if let Some(sprite) =
                        art_glyph.and_then(|g| self.sprite_set.as_ref().and_then(|s| s.slot(g)))
                    {
                        // Keep layer 0's opaque background; drop the glyph, the sprite covers it.
                        let sprite_inst = Instance::new(
                            inst.glyph,
                            inst.fg,
                            inst.bg,
                            0,
                            0,
                            inst.flags & FLAG_HAS_BG,
                        );
                        inherited_bg[idx] = sprite_inst.bg;
                        sprite_bg[idx] = true;
                        self.layers[0][idx] = sprite_inst;
                        self.emit_sprite(0, cx, cy, tile, sprite, draw_cell.tint);
                        continue;
                    }
                    if let Some(g) = art_glyph {
                        self.warn_if_tint_needs_sprite(g, draw_cell.tint);
                    }
                }
                inherited_bg[idx] = inst.bg;
                self.layers[0][idx] = inst;
                continue;
            }
            if cell_art_glyph(tile).is_none() {
                // Transparent: nothing drawn, and the running background is unchanged. This
                // branch runs after the span-covered `continue` above, so a `None` here always
                // means blank, never span-covered.
                self.layers[l][idx] = Instance::new(self.space_glyph, [0; 3], [0; 3], 0, 0, 0);
                continue;
            }
            // Occupied higher-layer tile: opaque background (own colour, or the inherited one when
            // the tile's background is `Default`) plus its glyph, unless no font in the chain can
            // draw that character at all (see `base_instance`).
            let resolved = self.glyphs.resolve(tile.glyph());
            self.warn_if_notdef_glyph(tile.glyph());
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
                sprite_bg[idx] = true;
                self.layers[l][idx] = Instance::new(glyph, fg, bg, 0, 0, has_bg);
                self.emit_sprite(l, cx, cy, tile, sprite, draw_cell.tint);
                continue;
            }
            #[cfg(feature = "tilesets")]
            self.warn_if_tint_needs_sprite(tile.glyph(), draw_cell.tint);
            sprite_bg[idx] = false;
            self.layers[l][idx] =
                Instance::new(glyph, fg, bg, tile.dx(), tile.dy(), FLAG_HAS_BG | has_glyph);
        }
        Ok(())
    }

    fn compositing(&self) -> Compositing {
        // Draw the raw layered stream back-to-front on the GPU (issue #368) instead of letting the
        // core flatten it, so per-layer transparency works the same as on `retroglyph-software`.
        // Composited layers plus sub-cell glyph spill mean a partial redraw could leave orphaned
        // pixels; redraw every cell of every layer each frame.
        Compositing::PixelLayered {
            needs_full_frame: true,
        }
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        // Upload is deferred to `present`, which owns the GL context.
        Ok(())
    }

    fn size(&self) -> Size {
        Size::new(self.cols, self.rows)
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        let base = self.base_blank();
        self.layers.truncate(1);
        for cell in &mut self.layers[0] {
            *cell = base;
        }
        // Sprite instances are collected per layer in lockstep with `self.layers` (issue #366);
        // a stale, larger `sprite_layers` would otherwise survive the clear and get redrawn by
        // `present` (issue #727).
        #[cfg(feature = "tilesets")]
        {
            self.sprite_layers.truncate(1);
            if self.sprite_layers.is_empty() {
                self.sprite_layers.push(Vec::new());
            }
            self.sprite_layers[0].clear();
        }
        Ok(())
    }

    fn resize(&mut self, size: Size) {
        self.cols = size.width();
        self.rows = size.height();
        let base = self.base_blank();
        self.layers = vec![vec![base; self.cell_count()]];
        // See the comment in `clear`: `sprite_layers` must stay in lockstep with `layers` so
        // `present` doesn't redraw sprites left over from before the resize (issue #727).
        #[cfg(feature = "tilesets")]
        {
            self.sprite_layers = vec![Vec::new()];
        }
    }
}

// GlRenderer implements neither `Input` nor `Cursor`: `WindowBackend<GlRenderer>` supplies both
// for windowed use (its input queue coalesces `Mouse(Moved)`; its cursor is a no-op), and a GL
// renderer has no headless-with-input path that would need its own. See the type-level docs.

// ── Presenter ────────────────────────────────────────────────────────────────

impl Presenter for GlRenderer {
    type SurfaceError = SurfaceError;

    fn init_surface(&mut self, window: Arc<dyn WindowHandle>) -> Result<(), SurfaceError> {
        // Re-entry (surface-loss recovery, issue #728): a previous `Gpu` may still be installed,
        // e.g. from `try_recover_surface` re-calling this after repeated present failures. Delete
        // its GL objects and drop its context before building the replacement, the same cleanup
        // `impl Drop for GlRenderer` does, so nothing from the old context is orphaned.
        if let Some(gpu) = self.gpu.take() {
            gpu.res.delete(&gpu.ctx.gl);
        }
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

    fn geometry(&self) -> CellGeometry {
        self.geometry
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
    use crate::config::GlBackendBuilder;
    use retroglyph_core::backend::Compositing;
    use retroglyph_core::backend::DrawCell;
    use retroglyph_core::backend::Output;
    use retroglyph_core::color::Color;
    use retroglyph_core::color::Style;
    use retroglyph_core::grid::Pos;
    use retroglyph_core::tile::Tile;

    const RED: Color = Color::rgb(255, 0, 0);

    #[test]
    fn draw_records_sub_cell_offset_and_flags_in_the_base_layer() {
        let mut r = GlBackendBuilder::new()
            .grid_size(4, 2)
            .build()
            .expect("default-font builds");
        let tile = Tile::new('A', Style::new()).with_offset(-3, 5);
        // `Output::draw` has no override on this backend (retroglyph#561): this exercises the
        // trait's default, which forwards to `draw_layers` tagged onto layer 0.
        r.draw(core::iter::once(DrawCell::new(Pos::new(1, 0), &tile)))
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
    fn build_rejects_a_grid_and_scale_that_overflow_the_surface_size() {
        // retroglyph#729: `CellGeometry::surface_size` multiplied cols/rows/scale as plain `u32`,
        // which overflows for a `u16` scale this large (unlike the software backend's `u8` scale).
        let result = GlBackendBuilder::new()
            .grid_size(u16::MAX, 1)
            .scale(u16::MAX)
            .build();
        assert!(matches!(
            result,
            Err(crate::config::GlBackendError::SurfaceTooLarge)
        ));
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
        assert_eq!(
            r.compositing(),
            Compositing::PixelLayered {
                needs_full_frame: true
            }
        );
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
            DrawCell::on_layer(0, Pos::new(0, 0), &base),
            DrawCell::on_layer(1, Pos::new(0, 0), &empty),
            DrawCell::on_layer(1, Pos::new(1, 0), &glyph_default_bg),
            DrawCell::on_layer(1, Pos::new(2, 0), &glyph_real_bg),
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
        r.draw_layers(core::iter::once(DrawCell::on_layer(
            1,
            Pos::new(0, 0),
            &tile,
        )))
        .expect("draw_layers");
        assert_eq!(r.layers.len(), 2);
        // Frame 2: only the base layer is streamed, so the higher layer must not linger.
        r.draw_layers(core::iter::once(DrawCell::on_layer(
            0,
            Pos::new(0, 0),
            &tile,
        )))
        .expect("draw_layers");
        assert_eq!(r.layers.len(), 1);
    }

    /// A span's covered cells draw no glyph of their own and take the anchor's background, so one
    /// piece of artwork sits on one uniform backdrop (retroglyph#412).
    #[test]
    fn draw_layers_gives_span_covered_cells_the_anchors_background_and_no_glyph() {
        use retroglyph_core::grid::Grid;

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
        r.draw_layers(
            tiles
                .iter()
                .map(|(l, pos, t)| DrawCell::on_layer(*l, *pos, t)),
        )
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

    /// retroglyph#726: a `Color::Default`-background span on a higher layer must not smear the
    /// anchor's column across the whole footprint. Layer 0 has a different background under each
    /// half of the span (red under the anchor, blue under the covered cell); the covered cell's
    /// `Default` background must inherit from *its own* column (blue), matching
    /// `retroglyph-software`'s `resolve_cell_bg`, not the anchor's (red).
    #[test]
    fn draw_layers_resolves_a_span_covered_cells_default_background_at_its_own_column() {
        use retroglyph_core::grid::Grid;

        const BLUE: Color = Color::rgb(0, 0, 255);

        let mut r = GlBackendBuilder::new()
            .grid_size(2, 1)
            .build()
            .expect("default-font builds");

        let mut grid = Grid::new(2, 1);
        grid.put_tile(0, (0, 0), Tile::new(' ', Style::new().bg(RED)));
        grid.put_tile(0, (1, 0), Tile::new(' ', Style::new().bg(BLUE)));
        grid.write_span(1, 0, 0, &["C="], Style::new())
            .expect("2x1 span fits");

        let mut tiles: Vec<(u8, Pos, Tile)> = (0..2)
            .map(|x| (0u8, Pos::new(x, 0), *grid.tile(0, (x, 0)).unwrap()))
            .collect();
        tiles.extend((0..2).map(|x| (1u8, Pos::new(x, 0), *grid.tile(1, (x, 0)).unwrap())));
        r.draw_layers(
            tiles
                .iter()
                .map(|(l, pos, t)| DrawCell::on_layer(*l, *pos, t)),
        )
        .expect("draw_layers is infallible");

        let covered = r.layers[1][1];
        assert_eq!(covered.flags, FLAG_HAS_BG, "covered cell draws no glyph");
        assert_eq!(
            covered.bg,
            [0, 0, 255],
            "covered cell inherits its own column's background, not the anchor's"
        );
    }

    /// Covered-cell suppression is grid state, not a tileset feature, so it holds with the
    /// `tilesets` feature off too: a span with no sprite behind it renders as its anchor glyph
    /// alone, the same on both pixel backends.
    #[test]
    fn draw_layers_suppresses_covered_glyphs_without_a_sprite() {
        use retroglyph_core::grid::Grid;

        let mut r = GlBackendBuilder::new()
            .grid_size(2, 1)
            .build()
            .expect("default-font builds");
        let mut grid = Grid::new(2, 1);
        grid.write_span(0, 0, 0, &["AB"], Style::new()).unwrap();
        let tiles: Vec<(u8, Pos, Tile)> = (0..2)
            .map(|x| (0u8, Pos::new(x, 0), *grid.tile(0, (x, 0)).unwrap()))
            .collect();
        r.draw_layers(
            tiles
                .iter()
                .map(|(l, pos, t)| DrawCell::on_layer(*l, *pos, t)),
        )
        .expect("draw_layers is infallible");

        assert_eq!(r.layers[0][0].flags & FLAG_HAS_GLYPH, FLAG_HAS_GLYPH);
        assert_eq!(r.layers[0][1].flags & FLAG_HAS_GLYPH, 0);
    }

    /// A single 8x16 opaque tile mapped to `'S'`. See `dropped_tint_tests::one_tile_png`'s doc
    /// comment for why this is a hardcoded byte literal rather than built with the `image` crate.
    #[cfg(feature = "tilesets")]
    fn one_tile_png() -> Vec<u8> {
        vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x10, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x2B, 0x8A, 0x3E, 0x7D, 0x00, 0x00, 0x00, 0x15, 0x49, 0x44, 0x41, 0x54, 0x78,
            0xDA, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x1F, 0x1F, 0x66, 0x18, 0x55, 0x30, 0x92, 0x14,
            0x00, 0x00, 0x09, 0x79, 0xFF, 0x01, 0x4F, 0x5C, 0x4F, 0x78, 0x00, 0x00, 0x00, 0x00,
            0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ]
    }

    /// retroglyph#726, the `has_sprite` arm `resolve_cell_bg`/the covered-cell branch above share:
    /// a `Color::Default`-background span whose *anchor* dispatches to a sprite paints no
    /// background on its covered cells (the sprite's own alpha provides coverage), matching
    /// `resolve_bg_fill`'s `has_sprite` rule. Layer 0's default background stays outside
    /// `inherited_bg`'s influence here on purpose: this only asserts the covered cell is
    /// transparent, not what shows through it.
    #[cfg(feature = "tilesets")]
    #[test]
    fn draw_layers_paints_no_background_on_a_span_covered_cell_whose_anchor_has_a_sprite() {
        use retroglyph_core::grid::Grid;
        use retroglyph_window::tileset::{Codepage, TilesetOptions};

        let opts = TilesetOptions::builder(one_tile_png())
            .tile_size(8, 16)
            .codepage(Codepage::Custom(vec!['S']))
            .build()
            .expect("valid single-tile tileset");
        let mut r = GlBackendBuilder::new()
            .grid_size(2, 1)
            .tileset(opts)
            .build()
            .expect("gl renderer with tileset");

        let mut grid = Grid::new(2, 1);
        grid.write_span(1, 0, 0, &["S="], Style::new())
            .expect("2x1 span fits");
        let tiles: Vec<(u8, Pos, Tile)> = (0..2)
            .map(|x| (1u8, Pos::new(x, 0), *grid.tile(1, (x, 0)).unwrap()))
            .collect();
        r.draw_layers(
            tiles
                .iter()
                .map(|(l, pos, t)| DrawCell::on_layer(*l, *pos, t)),
        )
        .expect("draw_layers is infallible");

        let covered = r.layers[1][1];
        assert_eq!(
            covered.flags & FLAG_HAS_BG,
            0,
            "a sprite anchor's covered cell paints no background"
        );
    }

    // ── Notdef diagnostic (retroglyph#1292) ─────────────────────────────────

    /// The substituted solid block is a legitimate cell on its own, so nothing about the
    /// rendered instance distinguishes it from a real one; this is the diagnostic that closes
    /// that gap on the base layer.
    #[test]
    fn layer_0_reports_a_character_no_font_covers() {
        let mut r = GlBackendBuilder::new()
            .grid_size(1, 1)
            .build()
            .expect("default-font builds");
        // Outside unscii16's CP437 repertoire.
        let tile = Tile::new('あ', Style::new());
        r.draw_layers(core::iter::once(DrawCell::on_layer(
            0,
            Pos::new(0, 0),
            &tile,
        )))
        .expect("draw_layers is infallible");

        assert_eq!(r.warned_notdef.contains(&'あ'), retroglyph_core::dev::DEV);
    }

    /// A character CP437 does cover must never be reported, dev build or not.
    #[test]
    fn layer_0_does_not_report_a_covered_character() {
        let mut r = GlBackendBuilder::new()
            .grid_size(1, 1)
            .build()
            .expect("default-font builds");
        let tile = Tile::new('A', Style::new());
        r.draw_layers(core::iter::once(DrawCell::on_layer(
            0,
            Pos::new(0, 0),
            &tile,
        )))
        .expect("draw_layers is infallible");

        assert!(!r.warned_notdef.contains(&'A'));
    }

    /// The `layer_id != 0` branch is a separate code path from layer 0's; it must report the same
    /// thing.
    #[test]
    fn a_higher_layer_reports_a_character_no_font_covers() {
        let mut r = GlBackendBuilder::new()
            .grid_size(1, 1)
            .build()
            .expect("default-font builds");
        let base = Tile::new(' ', Style::new());
        let tile = Tile::new('あ', Style::new());
        r.draw_layers(
            [
                DrawCell::on_layer(0, Pos::new(0, 0), &base),
                DrawCell::on_layer(1, Pos::new(0, 0), &tile),
            ]
            .into_iter(),
        )
        .expect("draw_layers is infallible");

        assert_eq!(r.warned_notdef.contains(&'あ'), retroglyph_core::dev::DEV);
    }
}

/// Dropped-tint diagnostic (retroglyph#564): a tint set on a cell whose glyph resolved to a
/// bitmap font rather than a sprite is silently dropped, the same trap retroglyph#537 fell into.
/// These exercise `GlRenderer::draw_layers` directly (no GL context needed: only the CPU-side
/// `warned_dropped_tint` set and instance arrays are inspected).
#[cfg(all(test, feature = "default-font", feature = "tilesets"))]
mod dropped_tint_tests {
    use crate::config::GlBackendBuilder;
    use retroglyph_core::backend::DrawCell;
    use retroglyph_core::backend::Output;
    use retroglyph_core::color::Style;
    use retroglyph_core::color::Tint;
    use retroglyph_core::grid::Pos;
    use retroglyph_core::tile::Tile;
    use retroglyph_window::tileset::{Codepage, TilesetOptions};

    /// A single 8x16 opaque red tile mapped to `'S'`, the Unscii cell size.
    ///
    /// A hardcoded byte literal rather than built with the `image` crate: unlike
    /// `retroglyph-software`, this crate only pulls `image` in as a dev-dependency on Linux and
    /// wasm32 (see `headless.rs`/`webgl_smoke.rs`), so a test that must build on every platform
    /// (this one; it needs no GL context) cannot depend on it being present to encode a PNG.
    fn one_tile_png() -> Vec<u8> {
        vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x10, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x2B, 0x8A, 0x3E, 0x7D, 0x00, 0x00, 0x00, 0x15, 0x49, 0x44, 0x41, 0x54, 0x78,
            0xDA, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x1F, 0x1F, 0x66, 0x18, 0x55, 0x30, 0x92, 0x14,
            0x00, 0x00, 0x09, 0x79, 0xFF, 0x01, 0x4F, 0x5C, 0x4F, 0x78, 0x00, 0x00, 0x00, 0x00,
            0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ]
    }

    fn renderer_with_sprite(cols: u16, rows: u16) -> crate::GlRenderer {
        let opts = TilesetOptions::builder(one_tile_png())
            .tile_size(8, 16)
            .codepage(Codepage::Custom(vec!['S']))
            .build()
            .expect("valid single-tile tileset");
        GlBackendBuilder::new()
            .grid_size(cols, rows)
            .tileset(opts)
            .build()
            .expect("default-font builds")
    }

    #[test]
    fn layer_0_reports_a_tint_on_a_glyph_without_a_sprite() {
        // 'X' has no tileset entry, so it falls back to the bitmap font, and any tint on it is
        // dropped.
        let mut r = renderer_with_sprite(1, 1);
        let tile = Tile::new('X', Style::new());
        r.draw_layers(core::iter::once(
            DrawCell::on_layer(0, Pos::new(0, 0), &tile).with_tint(Tint::multiply(128, 128, 128)),
        ))
        .expect("draw_layers is infallible");

        assert_eq!(
            r.warned_dropped_tint.contains(&'X'),
            retroglyph_core::dev::DEV
        );
    }

    #[test]
    fn layer_0_does_not_report_a_tint_on_a_glyph_that_has_a_sprite() {
        let mut r = renderer_with_sprite(1, 1);
        let tile = Tile::new('S', Style::new());
        r.draw_layers(core::iter::once(
            DrawCell::on_layer(0, Pos::new(0, 0), &tile).with_tint(Tint::multiply(128, 128, 128)),
        ))
        .expect("draw_layers is infallible");

        assert!(!r.warned_dropped_tint.contains(&'S'));
    }

    #[test]
    fn a_higher_layer_reports_a_tint_on_a_glyph_without_a_sprite() {
        // The `layer_id != 0` branch is a separate code path from layer 0's; it must report the
        // same thing.
        let mut r = renderer_with_sprite(1, 1);
        let base = Tile::new(' ', Style::new());
        let tile = Tile::new('X', Style::new());
        r.draw_layers(
            [
                DrawCell::on_layer(0, Pos::new(0, 0), &base),
                DrawCell::on_layer(1, Pos::new(0, 0), &tile).with_tint(Tint::multiply(1, 1, 1)),
            ]
            .into_iter(),
        )
        .expect("draw_layers is infallible");

        assert_eq!(
            r.warned_dropped_tint.contains(&'X'),
            retroglyph_core::dev::DEV
        );
    }

    #[test]
    fn tint_none_is_never_reported() {
        let mut r = renderer_with_sprite(1, 1);
        let tile = Tile::new('X', Style::new());
        r.draw_layers(core::iter::once(DrawCell::on_layer(
            0,
            Pos::new(0, 0),
            &tile,
        )))
        .expect("draw_layers is infallible");

        assert!(r.warned_dropped_tint.is_empty());
    }

    /// Draws a sprite on two layers, so `sprite_layers` has more than the (always present) base
    /// layer entry to be reset (issue #727).
    fn renderer_with_a_sprite_on_two_layers() -> crate::GlRenderer {
        let mut r = renderer_with_sprite(1, 1);
        let sprite = Tile::new('S', Style::new());
        r.draw_layers(
            [
                DrawCell::on_layer(0, Pos::new(0, 0), &sprite),
                DrawCell::on_layer(1, Pos::new(0, 0), &sprite),
            ]
            .into_iter(),
        )
        .expect("draw_layers is infallible");
        r
    }

    #[test]
    fn clear_resets_sprite_layers_to_a_single_empty_layer() {
        let mut r = renderer_with_a_sprite_on_two_layers();
        assert_eq!(r.sprite_layers.len(), 2);
        assert!(!r.sprite_layers[0].is_empty());

        r.clear().expect("clear is infallible");

        assert_eq!(r.sprite_layers.len(), 1);
        assert!(r.sprite_layers[0].is_empty());
    }

    #[test]
    fn resize_resets_sprite_layers_to_a_single_empty_layer() {
        use retroglyph_core::grid::Size;

        let mut r = renderer_with_a_sprite_on_two_layers();
        assert_eq!(r.sprite_layers.len(), 2);
        assert!(!r.sprite_layers[0].is_empty());

        r.resize(Size::new(2, 2));

        assert_eq!(r.sprite_layers.len(), 1);
        assert!(r.sprite_layers[0].is_empty());
    }
}

// ── Output conformance (retroglyph#763) ─────────────────────────────────────────

/// `GlRenderer` deliberately implements neither `Input` nor `Cursor` (see the type-level docs),
/// so only [`assert_output_contract`](retroglyph_core::testing::conformance::assert_output_contract)
/// applies here.
#[cfg(all(test, feature = "default-font"))]
mod output_conformance_tests {
    use crate::GlRenderer;
    use crate::config::GlBackendBuilder;
    use retroglyph_core::backend::Output;
    use retroglyph_core::grid::HasSize;
    use retroglyph_core::grid::Size;
    use retroglyph_core::testing::conformance::{Observable, fnv1a};

    /// `Instance` has no `PartialEq` (it's a tightly-packed, `#[repr(C)]` upload buffer, not a
    /// value type elsewhere in the crate needs to compare), so this compares the fields directly.
    fn instances_equal(a: &crate::renderer::Instance, b: &crate::renderer::Instance) -> bool {
        a.glyph == b.glyph
            && a.flags == b.flags
            && a.fg == b.fg
            && a.bg == b.bg
            && a.dx == b.dx
            && a.dy == b.dy
    }

    fn conformance_renderer(size: Size) -> GlRenderer {
        GlBackendBuilder::new()
            .grid_size(size.width(), size.height())
            .build()
            .expect("default-font build must not fail for a nonzero grid")
    }

    /// [`Observable::snapshot`] hashes only the CPU-side instance data that changed since the
    /// previous call, per that trait's docs. `GlRenderer` has no CPU-readable framebuffer without
    /// a real GL context (see `headless.rs`'s Linux-only pixel-readback tests), but its `layers`
    /// field is the exact per-cell data every draw uploads verbatim to the GPU on the next
    /// present, so hashing it is equivalent to hashing the frame for everything this contract
    /// checks (clear/resize/out-of-range handling never touch the GPU at all).
    struct GlObserver {
        renderer: GlRenderer,
        previous: Vec<Vec<crate::renderer::Instance>>,
    }

    impl GlObserver {
        fn new(size: Size) -> Self {
            let renderer = conformance_renderer(size);
            let previous = renderer.layers.clone();
            Self { renderer, previous }
        }
    }

    impl Output for GlObserver {
        type Error = core::convert::Infallible;

        fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
        where
            I: Iterator<Item = retroglyph_core::backend::DrawCell<'a>>,
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
            Output::clear(&mut self.renderer)
        }

        fn resize(&mut self, size: Size) {
            Output::resize(&mut self.renderer, size);
        }
    }

    impl Observable for GlObserver {
        fn snapshot(&mut self) -> u64 {
            let current = &self.renderer.layers;
            let mut hash = fnv1a(b"gl-diff");
            for (layer, (was, now)) in self.previous.iter().zip(current.iter()).enumerate() {
                for (index, (was, now)) in was.iter().zip(now.iter()).enumerate() {
                    if !instances_equal(was, now) {
                        hash ^= fnv1a(&(layer as u64).to_ne_bytes());
                        hash ^= fnv1a(&(index as u64).to_ne_bytes());
                        hash ^= fnv1a(&now.glyph.to_ne_bytes());
                        hash ^= fnv1a(&[now.flags]);
                        hash ^= fnv1a(&now.fg);
                        hash ^= fnv1a(&now.bg);
                        hash ^= fnv1a(&now.dx.to_ne_bytes());
                        hash ^= fnv1a(&now.dy.to_ne_bytes());
                    }
                }
            }
            // A resize changes the number of layers/cells outright: fold that in too, or a
            // shrink-then-grow back to the same per-cell content would hash identically to no
            // change at all.
            hash ^= fnv1a(&(current.len() as u64).to_ne_bytes());
            for layer in current {
                hash ^= fnv1a(&(layer.len() as u64).to_ne_bytes());
            }
            self.previous = current.clone();
            hash
        }
    }

    #[test]
    fn satisfies_the_output_contract() {
        retroglyph_core::testing::conformance::assert_output_contract(GlObserver::new);
    }
}
