//! GPU rendering backend for retroglyph: Vulkan, Metal, and D3D12 from a single codebase via
//! [`wgpu`].
//!
//! # Architecture
//!
//! [`WgpuBackendBuilder`] holds configuration (fonts, grid size, integer scale) and
//! [`build`](WgpuBackendBuilder::build)s a [`WgpuRenderer`]. The glyph source is a static
//! [`FontChain`] (a single [`BitmapFont`] is a chain of one); every font in the chain is
//! grid-packed into one `R8` array-texture atlas and addressed by a flat slot id (see
//! [`retroglyph_window::atlas`]). The renderer keeps one CPU-side instance array per grid layer and
//! creates its device lazily, when the windowing loop calls [`Presenter::init_surface`]:
//!
//! ```text
//! WgpuBackendBuilder (font, grid size, scale)
//!   |  .build()
//!   v
//! WgpuRenderer
//!   implements retroglyph_window::Presenter (an Output supertrait)
//!   wrapped by retroglyph_window::WindowBackend to become a full Backend
//!   (WindowBackend owns the input event queue and the no-op Cursor)
//!   |
//!   |  init_surface(window) -> wgpu Device + Queue + Surface, then GpuResources
//!   v
//! one render pass per present(): every grid layer back to front, each drawn as
//! backgrounds then coverage-blended glyphs (then sprites), from one instance
//! buffer uploaded once per frame.
//! ```
//!
//! A cell costs 16 bytes and no index buffer: the vertex shader derives the quad's corners from the
//! vertex index and the cell's `(column, row)` from the instance index, so the only per-instance
//! data is the glyph slot, two colors, the sub-cell offset, and the compositing flags.
//!
//! This backend composites grid layers itself on the GPU
//! ([`composites_layers`](Output::composites_layers) returns `true`): it receives the raw layered
//! stream from the core `Terminal` and draws each layer back to front, so an empty cell in a higher
//! layer lets the layer beneath show through while an occupied cell is opaque, matching
//! `retroglyph-software`'s per-pixel occlusion. It requests full frames
//! ([`needs_full_frame`](Output::needs_full_frame) returns `true`) and redraws every cell of every
//! layer each frame, so there is no orphaned-pixel problem from sub-cell glyph spill.
//!
//! # Choosing between this and `retroglyph-gl`
//!
//! Both are GPU backends drawing the same instanced-quad pipeline, and both produce pixel-identical
//! output (each is checked against the `retroglyph-software` CPU rasterizer). They differ in which
//! driver stack they reach and what they cost to depend on:
//!
//! | | `retroglyph-wgpu` | `retroglyph-gl` |
//! | --- | --- | --- |
//! | APIs | Vulkan, Metal, D3D12 | OpenGL 3.3, WebGL2 |
//! | Browser | yes, WebGPU (see [Platform support](#platform-support)) | yes, WebGL2 |
//! | `unsafe` in the backend | none | unavoidable (every GL call) |
//! | Direct dependencies, transitively | 85 crates | 54 crates |
//! | Clean debug build of the crate | 15s | 7s |
//! | Offscreen render tests | every platform | Linux/EGL only |
//!
//! The dependency and build-time figures are for one host (macOS, `--all-features`); the ratio is
//! what matters, not the absolute numbers. The difference is `naga` (the shader front end that
//! compiles this crate's WGSL) plus `wgpu-core` (the validation and state-tracking layer that
//! makes the API safe), not the three backend APIs: only one hardware abstraction layer compiles
//! per platform, since `wgpu-hal`'s backends are target-gated. A macOS build pulls the Metal
//! stack and no Vulkan; a Linux build pulls `ash` and no Metal.
//!
//! Both cover the browser today (WebGPU here, WebGL2 there); pick `retroglyph-gl` when a smaller
//! dependency tree matters more than the rest, or this one for validation-layer diagnostics, a
//! modern driver path, and a backend with no `unsafe` in it. On wasm32 this backend's device is
//! ready asynchronously (see [Platform support](#platform-support)), where `retroglyph-gl`'s WebGL2
//! context is ready synchronously; that is the one real difference the browser adds to the choice.
//!
//! # Performance
//!
//! A 200x60 grid with three layers (36000 cells, 562 KiB of instance data) costs 266us per frame
//! end to end: flattening the layers, uploading, encoding six draws, submitting, and waiting for
//! the GPU to finish. That is roughly 4% of a 60 Hz frame budget, on an M-series laptop at
//! `--release`, and it is larger than any grid the examples use.
//!
//! The number is here to set expectations, not to invite tuning: a character grid is a trivial
//! workload for a modern GPU, and the design choices above (deriving position from the instance
//! index, packing every layer into one upload) are about keeping the per-frame *bandwidth* small
//! rather than about winning a benchmark. Emitting four real vertices per cell instead, with
//! explicit positions and UVs, is the other common shape for this renderer and would cost roughly
//! six times the per-frame bytes.
//!
//! # Platform support
//!
//! Native (Vulkan, Metal, D3D12) and the browser (WebGPU) both work.
//!
//! [`Presenter::init_surface`] is synchronous, but `request_adapter` and `request_device` are not,
//! and a browser's main thread has no way to block on a future the way native does. So the two
//! targets take different paths through the same call: `Instance::create_surface` *is* synchronous
//! everywhere, so on wasm32 `init_surface` creates the surface from the window's canvas, spawns the
//! adapter and device request with `wasm_bindgen_futures::spawn_local`, and returns immediately;
//! [`present`](Presenter::present) polls the result each frame and draws nothing until it lands.
//! Native, by contrast, blocks on the same requests with `pollster::block_on` inside
//! `init_surface` itself, so the device is always ready by the time it returns. See
//! `crate::gpu::PendingGpu` for the shared-state cell this deferral is built on.
//!
//! The one user-visible consequence is on wasm32 only: the first frames after startup are blank
//! (whatever was drawn before the device exists is not shown) until the adapter and device resolve,
//! typically well under a second. Native has no such gap.
//!
//! `retroglyph-gl` also covers the browser, through WebGL2, whose context creation is synchronous
//! and so needs no equivalent deferral. Pick between the two using the table above and the
//! dependency/build-time comparison it links to.
//!
//! # Environment variables
//!
//! `wgpu` reads its own configuration from the environment, and this crate passes it through
//! rather than overriding it:
//!
//! | Variable | Effect |
//! | --- | --- |
//! | `WGPU_BACKEND` | Restricts the backends to try (`vulkan`, `metal`, `dx12`). |
//! | `WGPU_POWER_PREF` | `low` or `high`; defaults to `low`, since a character grid is a handful of draw calls per frame. |
//! | `WGPU_VALIDATION` / `WGPU_DEBUG` | Toggle the driver's validation and debug layers. |
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
//! PNG sprite/tileset support: decodes sprite sheets into an RGBA array-texture atlas and draws
//! them in a third, source-over blended pass per grid layer.
//!
//! Forwards to `retroglyph-window`'s shared tileset decode.
//! <!-- gen-features:end -->

#![doc(html_logo_url = "https://raw.githubusercontent.com/crates-lurey-io/retroglyph/main/docs/public/assets/logo.svg")]
#![doc(html_favicon_url = "https://raw.githubusercontent.com/crates-lurey-io/retroglyph/main/docs/public/assets/logo.svg")]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod config;

mod error;
mod gpu;
mod instance;
mod renderer;
mod shaders;
#[cfg(feature = "tilesets")]
mod sprite_set;

// Offscreen render tests: create a device with no surface at all, run the real pipeline into a
// texture, and read the pixels back to assert on them. Unlike `retroglyph-gl`'s EGL-surfaceless
// equivalent these need no platform-specific setup, so they run wherever wgpu finds an adapter.
#[cfg(all(test, feature = "default-font"))]
mod headless;

pub use config::{WgpuBackendBuilder, WgpuBackendError};
pub use error::SurfaceError;
// Re-export the font types so a consumer can build a custom atlas without a separate dependency.
pub use retroglyph_window::font::{self as font, BitmapFont, FontChain};

use gpu::{GpuContext, PendingGpu, WindowSurface, WindowedResult};
use instance::{Cell, FLAG_HAS_BG, FLAG_HAS_GLYPH};
use renderer::{GpuResources, LayerRange};
use retroglyph_core::backend::{DrawCell, Output};
use retroglyph_core::color::Color;
use retroglyph_core::grid::HasSize;
use retroglyph_core::grid::Size;
use retroglyph_core::tile::Tile;
use retroglyph_window::atlas::GlyphAtlas;
use retroglyph_window::palette::{DEFAULT_BG, DEFAULT_FG};
#[cfg(feature = "tilesets")]
use retroglyph_window::sprite_cache::SpriteTint;
use retroglyph_window::{CellGeometry, Presenter, WindowHandle, cell_art_glyph};
#[cfg(feature = "tilesets")]
use sprite_set::{SpriteInstance, SpriteSet, SpriteSlot};
use std::sync::Arc;

// Compile the crate README's code blocks as doctests so the quick start can't silently rot.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

/// The live wgpu renderer: a [`Presenter`], wrapped in
/// [`WindowBackend`](retroglyph_window::WindowBackend) to form a full
/// [`Backend`](retroglyph_core::backend::Backend) for the windowing loop.
///
/// It does not implement [`Input`](retroglyph_core::backend::Input) or
/// [`Cursor`](retroglyph_core::backend::Cursor) itself: a GPU renderer cannot present without a
/// live device, so there is no headless-with-input use for a bare `Terminal<WgpuRenderer>`. In
/// windowed use `WindowBackend` owns the input queue (with its `Mouse(Moved)` coalescing) and the
/// no-op cursor, so a duplicate queue here would only ever be dead. See the sub-cell offset note on
/// [`Presenter`] for the shared rendering contract.
///
/// Build one with [`WgpuBackendBuilder`]. Before the windowing loop calls
/// [`init_surface`](Presenter::init_surface) there is no device; drawing updates only the CPU-side
/// instance arrays, and [`present`](Presenter::present) is a no-op. Once the device exists,
/// `present` uploads the frame and encodes one render pass.
pub struct WgpuRenderer {
    /// Character-to-atlas-slot map for the bitmap font chain.
    glyphs: GlyphAtlas,
    cols: u16,
    rows: u16,
    /// Cell/surface pixel geometry (glyph size x scale); the single source of the `cell_size`
    /// contract, delegated to by [`Presenter::cell_size`].
    geometry: CellGeometry,
    /// Atlas slot for the space glyph, used to initialize blank cells.
    space_glyph: u16,
    /// Per-layer instance arrays (index = grid layer id), each `cols * rows` in row-major cell
    /// order. `layers[0]` is the always-opaque base; higher layers composite over it back to front.
    /// Rebuilt each frame by [`Output::draw_layers`], since this backend requests full frames.
    /// There is always at least the base layer.
    layers: Vec<Vec<Cell>>,
    /// Scratch buffer holding every layer's cells back to back for the per-frame upload. Kept as a
    /// field so a steady-state frame reuses one allocation instead of building a new one.
    upload: Vec<Cell>,
    /// Per-layer slices of `upload`, parallel to `layers`.
    ranges: Vec<LayerRange>,
    /// The decoded sprite atlas, if a tileset was loaded. Retained so the GPU atlas can be rebuilt
    /// if the device is recreated.
    #[cfg(feature = "tilesets")]
    sprite_set: Option<SpriteSet>,
    /// Per-layer sprite instances, parallel to `layers`, rebuilt each frame by
    /// [`Output::draw_layers`].
    #[cfg(feature = "tilesets")]
    sprite_layers: Vec<Vec<SpriteInstance>>,
    /// Sprite equivalents of `upload`/`ranges`.
    #[cfg(feature = "tilesets")]
    sprite_upload: Vec<SpriteInstance>,
    #[cfg(feature = "tilesets")]
    sprite_ranges: Vec<LayerRange>,
    /// Glyphs already reported as needing a span, so a redraw loop logs each one once instead of
    /// every frame. See `retroglyph_window::sprite_cache::warn_sprite_needs_span`.
    #[cfg(feature = "tilesets")]
    warned_oversized: std::collections::BTreeSet<char>,
    /// Glyphs already reported as having a dropped tint, so a redraw loop logs each one once
    /// instead of every frame. See `retroglyph_window::sprite_cache::warn_tint_needs_sprite`.
    #[cfg(feature = "tilesets")]
    warned_dropped_tint: std::collections::BTreeSet<char>,
    /// The current surface size in physical pixels (set by
    /// [`resize_surface`](Presenter::resize_surface)).
    surface_size: (u32, u32),
    /// Device, surface, and GPU resources. `None` until the device is ready: before
    /// [`init_surface`](Presenter::init_surface) is ever called, and on wasm32 for every frame
    /// while [`pending`](Self::pending) is still resolving.
    gpu: Option<Gpu>,
    /// wasm32 only in practice: the deferred adapter/device request started by
    /// [`init_surface`](Presenter::init_surface), polled by [`present`](Presenter::present) each
    /// frame until it resolves. Always `None` on native, where `init_surface` never returns
    /// without the device already installed in `Self::gpu`; `PendingGpu` is uninhabited there, so
    /// this field costs nothing to poll.
    pending: Option<PendingGpu>,
}

/// The live device, surface, and resources, present only after
/// [`init_surface`](Presenter::init_surface).
struct Gpu {
    context: GpuContext,
    /// The window's swap chain. `None` for the offscreen render tests, which render into a texture
    /// they own rather than a frame they acquire.
    surface: Option<WindowSurface>,
    resources: GpuResources,
}

impl WgpuRenderer {
    /// Builds a renderer for the given glyph atlas, grid size, and scale. Called by
    /// [`WgpuBackendBuilder::build`].
    ///
    /// Glyph cells wider or taller than 255 unscaled pixels are clamped to 255 (the
    /// [`CellGeometry`] limit).
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn new(glyphs: GlyphAtlas, cols: u16, rows: u16, scale: u16) -> Self {
        let (cell_w, cell_h) = glyphs.cell_size();
        let geometry = CellGeometry::new(cell_w.min(255) as u8, cell_h.min(255) as u8, scale);
        let space_glyph = glyphs.space_slot();
        let count = usize::from(cols) * usize::from(rows);
        Self {
            glyphs,
            cols,
            rows,
            geometry,
            space_glyph,
            layers: vec![vec![base_blank(space_glyph); count]],
            upload: Vec::new(),
            ranges: Vec::new(),
            #[cfg(feature = "tilesets")]
            sprite_set: None,
            #[cfg(feature = "tilesets")]
            sprite_layers: Vec::new(),
            #[cfg(feature = "tilesets")]
            sprite_upload: Vec::new(),
            #[cfg(feature = "tilesets")]
            sprite_ranges: Vec::new(),
            #[cfg(feature = "tilesets")]
            warned_oversized: std::collections::BTreeSet::new(),
            #[cfg(feature = "tilesets")]
            warned_dropped_tint: std::collections::BTreeSet::new(),
            surface_size: geometry.surface_size(cols, rows),
            gpu: None,
            pending: None,
        }
    }

    /// Attaches a decoded sprite atlas. Called by [`WgpuBackendBuilder::build`] when a tileset was
    /// registered; the GPU atlas is built later, in [`build_resources`](Self::build_resources).
    #[cfg(feature = "tilesets")]
    pub(crate) fn set_sprites(&mut self, set: SpriteSet) {
        self.sprite_set = Some(set);
    }

    /// The base-layer blank instance: space glyph, default colors, opaque default background, no
    /// glyph drawn. Layer 0 always paints its background (the opaque base), so an untouched base
    /// cell is the default background.
    const fn base_blank(&self) -> Cell {
        base_blank(self.space_glyph)
    }

    /// Total cell count for the current grid.
    fn cell_count(&self) -> usize {
        usize::from(self.cols) * usize::from(self.rows)
    }

    /// Reports a sprite drawn larger than one cell without a span to reserve the cells it covers.
    ///
    /// Shares `retroglyph-window`'s diagnostic with the other backends so all of them name the same
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
    #[cfg(feature = "tilesets")]
    fn warn_if_tint_needs_sprite(&mut self, glyph: char, tint: retroglyph_core::color::Tint) {
        retroglyph_window::sprite_cache::warn_tint_needs_sprite(
            &mut self.warned_dropped_tint,
            glyph,
            tint,
        );
    }

    /// Builds the GPU resources for the current grid on an existing device: compiles the pipelines,
    /// uploads the glyph and sprite atlases, and allocates the instance buffers.
    ///
    /// Shared by [`Presenter::init_surface`] (windowed) and the offscreen render tests, so both
    /// exercise the same setup: the point of those tests is to catch a break in exactly this
    /// pipeline, so it must not diverge from the real one.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError::Init`] if either atlas exceeds the device's texture limits.
    pub(crate) fn build_resources(
        &self,
        context: &GpuContext,
        target_format: wgpu::TextureFormat,
    ) -> Result<GpuResources, SurfaceError> {
        let atlas = self.glyphs.data();
        let capacity = u32::try_from(self.cell_count()).unwrap_or(u32::MAX);
        #[cfg_attr(not(feature = "tilesets"), allow(unused_mut))]
        let mut resources = GpuResources::new(
            &context.device,
            &context.queue,
            target_format,
            &atlas,
            capacity,
        )?;
        #[cfg(feature = "tilesets")]
        if let Some(set) = &self.sprite_set {
            resources.attach_sprites(&context.device, &context.queue, target_format, set)?;
        }
        Ok(resources)
    }

    /// Flattens the per-layer instance arrays into [`Self::upload`] and records each layer's slice
    /// in [`Self::ranges`].
    ///
    /// One contiguous buffer per frame is what lets the whole frame be a single `write_buffer` and
    /// a single render pass; see [`renderer`]'s module docs for why re-uploading between layers
    /// would be wrong rather than merely slower.
    fn flatten_layers(&mut self) {
        self.upload.clear();
        self.ranges.clear();
        for layer in &self.layers {
            let start = u32::try_from(self.upload.len()).unwrap_or(u32::MAX);
            let count = u32::try_from(layer.len()).unwrap_or(u32::MAX);
            self.upload.extend_from_slice(layer);
            self.ranges.push(LayerRange { start, count });
        }

        #[cfg(feature = "tilesets")]
        {
            self.sprite_upload.clear();
            self.sprite_ranges.clear();
            for layer in &self.sprite_layers {
                let start = u32::try_from(self.sprite_upload.len()).unwrap_or(u32::MAX);
                let count = u32::try_from(layer.len()).unwrap_or(u32::MAX);
                self.sprite_upload.extend_from_slice(layer);
                self.sprite_ranges.push(LayerRange { start, count });
            }
        }
    }

    /// Installs a device and resources without a surface, for the offscreen render tests.
    ///
    /// The windowed path installs the same pair in [`Presenter::init_surface`]; splitting this out
    /// is what lets `headless` drive [`encode_frame`](Self::encode_frame) (and therefore the whole
    /// production render path) against a texture it owns.
    #[cfg(all(test, feature = "default-font"))]
    fn install_offscreen(&mut self, context: GpuContext, resources: GpuResources) {
        self.gpu = Some(Gpu {
            context,
            surface: None,
            resources,
        });
    }

    /// The installed device, for the offscreen render tests' readback.
    #[cfg(all(test, feature = "default-font"))]
    fn offscreen_context(&self) -> Option<&GpuContext> {
        self.gpu.as_ref().map(|gpu| &gpu.context)
    }

    /// Builds resources for a newly ready device and surface, and installs both as [`Self::gpu`].
    ///
    /// Shared by [`Presenter::init_surface`] (native: the device is always ready by the time it
    /// returns) and [`Presenter::present`] (wasm32: called once the deferred adapter/device
    /// request resolves).
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError::Init`] if either atlas exceeds the device's texture limits.
    fn install_device(
        &mut self,
        context: GpuContext,
        surface: WindowSurface,
    ) -> Result<(), SurfaceError> {
        log::info!("retroglyph-wgpu: {}", context.describe());
        let resources = self.build_resources(&context, surface.view_format)?;
        self.gpu = Some(Gpu {
            context,
            surface: Some(surface),
            resources,
        });
        Ok(())
    }

    /// The sprite atlas layer size in texels, or `(0, 0)` without a tileset.
    fn sprite_tex_size(&self) -> (u32, u32) {
        #[cfg(feature = "tilesets")]
        {
            self.sprite_set.as_ref().map_or((0, 0), SpriteSet::tex_size)
        }
        #[cfg(not(feature = "tilesets"))]
        {
            (0, 0)
        }
    }

    /// Uploads the current frame and encodes it into `view`, without acquiring or presenting a
    /// surface frame.
    ///
    /// Split out from [`present`](Presenter::present) so the offscreen render tests can point the
    /// same code at a texture they own.
    fn encode_frame(&mut self, view: &wgpu::TextureView) {
        self.flatten_layers();
        let (screen, cell, glyph, cols, sprite_tex) = (
            self.surface_size,
            self.geometry.cell_size(),
            self.glyphs.cell_size(),
            self.cols,
            self.sprite_tex_size(),
        );
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        gpu.resources
            .set_uniforms(&gpu.context.queue, screen, cell, glyph, cols, sprite_tex);
        gpu.resources
            .upload_cells(&gpu.context.device, &gpu.context.queue, &self.upload);
        #[cfg(feature = "tilesets")]
        gpu.resources
            .upload_sprites(&gpu.context.device, &gpu.context.queue, &self.sprite_upload);

        let mut encoder =
            gpu.context
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("retroglyph frame"),
                });
        gpu.resources.render(
            &mut encoder,
            view,
            &self.ranges,
            #[cfg(feature = "tilesets")]
            &self.sprite_ranges,
        );
        gpu.context.queue.submit(Some(encoder.finish()));
    }
}

/// `(u8, u8, u8)` -> `[u8; 3]`, for packing resolved colors into a [`Cell`].
const fn to_arr(rgb: (u8, u8, u8)) -> [u8; 3] {
    [rgb.0, rgb.1, rgb.2]
}

/// The base-layer blank instance for `space_glyph`: opaque default background, no glyph. A free
/// function so [`WgpuRenderer::new`] can build it before `self` exists.
const fn base_blank(space_glyph: u16) -> Cell {
    Cell::new(
        space_glyph,
        to_arr(DEFAULT_FG),
        to_arr(DEFAULT_BG),
        0,
        0,
        FLAG_HAS_BG,
    )
}

/// Builds the base-layer (layer 0) [`Cell`] for `tile` at the already-resolved atlas `slot`: the
/// background is always opaque (default-substituted), and the glyph is drawn only when
/// [`cell_art_glyph`] says this tile draws art (see its docs for the blank/span-covered rules).
///
/// A `slot` of `None` is a character no font in the chain can draw, not even as the substituted
/// solid block; the cell keeps its background and draws no glyph, matching `retroglyph-software`.
const fn base_instance(slot: Option<u16>, tile: &Tile) -> Cell {
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
    Cell::new(slot, fg, bg, tile.dx(), tile.dy(), flags)
}

// ── Output ───────────────────────────────────────────────────────────────────

impl Output for WgpuRenderer {
    // Drawing only touches CPU memory (the instance arrays); it never fails. Device failures
    // surface through `Presenter::present`'s `SurfaceError` instead.
    type Error = core::convert::Infallible;

    // No `draw` override: this backend always composites (`composites_layers` returns `true`
    // below), so `Terminal::present` never calls single-layer `draw` and the default implementation
    // (which forwards to `draw_layers`) is exactly right. See retroglyph#561 for why a second,
    // hand-maintained body is worse than none.
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

        // Per-cell record of whether the occupant drawn at that index dispatched to a sprite, keyed
        // the same way as `inherited_bg`. A span's covered cells hold only a text-fallback glyph
        // that never has a sprite of its own, so the covered-cell branch below consults this at the
        // *anchor's* index to answer "does this span dispatch to a sprite", matching
        // `retroglyph-software`'s `resolve_cell_bg` (retroglyph#726). Reused across layers: a lower
        // layer's `true` is always overwritten before a higher layer's covered cell can read it,
        // because the anchor of any span is written before its covered cells (row-major stream).
        let mut sprite_bg = vec![false; cell_count];

        // Sprite instances are collected per layer in lockstep with `self.layers`: reset to just
        // the (empty) base layer; higher layers are grown alongside `self.layers`.
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
                self.layers
                    .push(vec![Cell::transparent(self.space_glyph); cell_count]);
                #[cfg(feature = "tilesets")]
                self.sprite_layers.push(Vec::new());
            }
            let idx = y * cols + x;

            #[cfg(feature = "tilesets")]
            #[allow(clippy::cast_possible_truncation)]
            let (cx, cy) = (x as u16, y as u16);

            // A cell covered by a multi-cell span (retroglyph#412) draws no glyph of its own: the
            // span's anchor emitted one sprite across the whole footprint, and this cell's glyph is
            // that sprite's text fallback, for backends that can't draw it. Every cell of a span
            // shares one `Style` (see `Grid::write_span_cells`), so this cell's own tile already
            // carries the same colors as the anchor; the anchor is consulted only to answer "does
            // this span dispatch to a sprite" (via `sprite_bg`), the same split
            // `retroglyph-software`'s `resolve_cell_bg` documents. Resolving the running inherited
            // background at this cell's own index, not the anchor's, keeps a span from smearing one
            // column's inheritance across the whole footprint (retroglyph#726).
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
                    self.layers[l][idx] = Cell::new(self.space_glyph, fg, bg, 0, 0, has_bg);
                    continue;
                }
            }

            if layer_id == 0 {
                let slot = self.glyphs.resolve(tile.glyph());
                let inst = base_instance(slot, tile);
                // Sprite dispatch is gated on `cell_art_glyph`, not the raw `tile.glyph()`: a blank
                // layer-0 cell (`is_empty()`, e.g. an untouched grid cell) draws no art at all,
                // even if its glyph happens to have a registered sprite (retroglyph#762).
                #[cfg(feature = "tilesets")]
                {
                    let art_glyph = cell_art_glyph(tile);
                    if let Some(sprite) =
                        art_glyph.and_then(|g| self.sprite_set.as_ref().and_then(|s| s.slot(g)))
                    {
                        // Keep layer 0's opaque background; drop the glyph, the sprite covers it.
                        let sprite_inst = Cell::new(
                            inst.glyph,
                            [inst.fg[0], inst.fg[1], inst.fg[2]],
                            [inst.bg[0], inst.bg[1], inst.bg[2]],
                            0,
                            0,
                            inst.flags & FLAG_HAS_BG,
                        );
                        inherited_bg[idx] = [inst.bg[0], inst.bg[1], inst.bg[2]];
                        sprite_bg[idx] = true;
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
                            SpriteTint::resolve(
                                sprite.color,
                                tile.style().foreground(),
                                draw_cell.tint,
                                DEFAULT_FG,
                            ),
                        ));
                        continue;
                    }
                    if let Some(g) = art_glyph {
                        self.warn_if_tint_needs_sprite(g, draw_cell.tint);
                    }
                }
                inherited_bg[idx] = [inst.bg[0], inst.bg[1], inst.bg[2]];
                self.layers[0][idx] = inst;
                continue;
            }
            if cell_art_glyph(tile).is_none() {
                // Transparent: nothing drawn, and the running background is unchanged. This branch
                // runs after the span-covered `continue` above, so a `None` here always means
                // blank, never span-covered.
                self.layers[l][idx] = Cell::transparent(self.space_glyph);
                continue;
            }
            // Occupied higher-layer tile: opaque background (its own color, or the inherited one
            // when the tile's background is `Default`) plus its glyph, unless no font in the chain
            // can draw that character at all (see `base_instance`).
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
                sprite_bg[idx] = true;
                self.layers[l][idx] = Cell::new(glyph, fg, bg, 0, 0, has_bg);
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
                    SpriteTint::resolve(
                        sprite.color,
                        tile.style().foreground(),
                        draw_cell.tint,
                        DEFAULT_FG,
                    ),
                ));
                continue;
            }
            #[cfg(feature = "tilesets")]
            self.warn_if_tint_needs_sprite(tile.glyph(), draw_cell.tint);
            sprite_bg[idx] = false;
            self.layers[l][idx] =
                Cell::new(glyph, fg, bg, tile.dx(), tile.dy(), FLAG_HAS_BG | has_glyph);
        }
        Ok(())
    }

    fn needs_full_frame(&self) -> bool {
        // Composited layers plus sub-cell glyph spill mean a partial redraw could leave orphaned
        // pixels; redraw every cell of every layer each frame.
        true
    }

    fn composites_layers(&self) -> bool {
        // Draw the raw layered stream back to front on the GPU instead of letting the core flatten
        // it, so per-layer transparency works the same as on `retroglyph-software`.
        true
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        // Upload is deferred to `present`, which owns the device.
        Ok(())
    }

    fn size(&self) -> Size {
        Size::new(self.cols, self.rows)
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        let base = self.base_blank();
        self.layers.truncate(1);
        self.layers[0].fill(base);
        // Sprite instances are collected per layer in lockstep with `self.layers`; a stale, larger
        // `sprite_layers` would otherwise survive the clear and get redrawn by `present`
        // (retroglyph#727).
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
        // `present` doesn't redraw sprites left over from before the resize (retroglyph#727).
        #[cfg(feature = "tilesets")]
        {
            self.sprite_layers = vec![Vec::new()];
        }
    }
}

// WgpuRenderer implements neither `Input` nor `Cursor`: `WindowBackend<WgpuRenderer>` supplies both
// for windowed use (its input queue coalesces `Mouse(Moved)`; its cursor is a no-op), and a GPU
// renderer has no headless-with-input path that would need its own. See the type-level docs.

// ── Presenter ────────────────────────────────────────────────────────────────

impl Presenter for WgpuRenderer {
    type SurfaceError = SurfaceError;

    fn init_surface(&mut self, window: Arc<dyn WindowHandle>) -> Result<(), SurfaceError> {
        // Re-entry (surface-loss recovery, retroglyph#728): a previous device may still be
        // installed, e.g. from the winit driver re-calling this after repeated present failures.
        // Drop it before building the replacement, so the old surface releases the window it owns
        // before a new surface asks for it. A previous deferred request (wasm32) is dropped too:
        // its result, once it lands, would otherwise install a device for a window that is no
        // longer current.
        self.gpu = None;
        self.pending = None;

        let (w, h) = self.surface_size;
        match GpuContext::windowed(window, w, h)? {
            WindowedResult::Ready(context, surface) => self.install_device(context, surface)?,
            // wasm32 only in practice (see `Self::pending`'s docs): the device isn't ready yet.
            // `present` polls `self.pending` each frame until it resolves, drawing nothing in the
            // meantime; drawing itself still updates the CPU-side arrays normally.
            WindowedResult::Pending(pending) => self.pending = Some(pending),
        }
        Ok(())
    }

    fn resize_surface(&mut self, width: u32, height: u32) {
        self.surface_size = (width, height);
        if let Some(gpu) = &mut self.gpu
            && let Some(surface) = gpu.surface.as_mut()
        {
            surface.resize(&gpu.context, width, height);
        }
    }

    fn present(&mut self) -> Result<(), SurfaceError> {
        // wasm32 only in practice: poll the deferred adapter/device request. While it is still in
        // flight, drop through to the no-device branch below and draw nothing this frame; once it
        // resolves, install the device (reconciling any resize that landed while it was pending)
        // and fall through to render the very same frame.
        if let Some(pending) = self.pending.as_ref() {
            match pending.poll() {
                None => {}
                Some(Ok((context, mut surface))) => {
                    self.pending = None;
                    let (w, h) = self.surface_size;
                    surface.resize(&context, w, h);
                    self.install_device(context, surface)?;
                }
                Some(Err(err)) => {
                    self.pending = None;
                    return Err(err);
                }
            }
        }

        // No device yet: nothing to present. Drawing has still updated the CPU-side arrays, so the
        // first frame after the device becomes ready shows the current grid rather than a blank
        // one. On wasm32 this is also the ordinary state for every frame before the deferred
        // request above resolves.
        let Some(gpu) = self.gpu.as_mut() else {
            return Ok(());
        };
        let Some(surface) = gpu.surface.as_ref() else {
            return Ok(());
        };
        let frame = surface.acquire(&gpu.context)?;
        self.encode_frame(&frame.view);
        // Reborrow: `encode_frame` needs `&mut self`, so the earlier borrow can't survive it.
        let gpu = self.gpu.as_ref().expect("context installed above");
        frame.present(&gpu.context);
        Ok(())
    }

    fn cell_size(&self) -> (u32, u32) {
        self.geometry.cell_size()
    }

    fn geometry(&self) -> CellGeometry {
        self.geometry
    }
}

#[cfg(all(test, feature = "default-font"))]
mod compositing_tests {
    use super::{FLAG_HAS_BG, FLAG_HAS_GLYPH, WgpuBackendBuilder};
    use retroglyph_core::backend::{DrawCell, Output};
    use retroglyph_core::color::{Color, Style};
    use retroglyph_core::grid::Pos;
    use retroglyph_core::tile::Tile;

    const RED: Color = Color::Rgb { r: 255, g: 0, b: 0 };

    #[test]
    fn draw_records_sub_cell_offset_and_flags_in_the_base_layer() {
        let mut r = WgpuBackendBuilder::new()
            .grid_size(4, 2)
            .build()
            .expect("default-font builds");
        let tile = Tile::new('A', Style::new()).with_offset(-3, 5);
        // `Output::draw` has no override on this backend: this exercises the trait's default, which
        // forwards to `draw_layers` tagged onto layer 0.
        r.draw(core::iter::once(DrawCell::new(Pos::new(1, 0), &tile)))
            .expect("draw is infallible");

        let inst = r.layers[0][1];
        assert_eq!((inst.dx, inst.dy), (-3, 5));
        let a_slot = r.glyphs.resolve('A').expect("'A' is in CP437");
        assert_eq!(inst.glyph, a_slot);
        // A non-empty tile on the base layer draws both its glyph and its (base) background.
        assert_eq!(inst.flags, FLAG_HAS_BG | FLAG_HAS_GLYPH);
    }

    #[test]
    fn base_layer_blank_cells_are_opaque_background_only() {
        let r = WgpuBackendBuilder::new()
            .grid_size(3, 3)
            .build()
            .expect("default-font builds");
        assert!(
            r.layers[0]
                .iter()
                .all(|i| i.dx == 0 && i.dy == 0 && i.flags == FLAG_HAS_BG)
        );
    }

    #[test]
    fn composites_layers_and_requests_full_frames() {
        let r = WgpuBackendBuilder::new()
            .grid_size(2, 1)
            .build()
            .expect("default-font builds");
        assert!(r.composites_layers());
        assert!(r.needs_full_frame());
    }

    #[test]
    fn draw_layers_encodes_the_occlusion_rule_per_layer() {
        let mut r = WgpuBackendBuilder::new()
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
        // Higher-layer empty cell: fully transparent, so the lower layer shows.
        assert_eq!(r.layers[1][0].flags, 0);
        // Higher-layer occupied cell with a Default background is opaque (it erases the glyph
        // beneath), inheriting the background from below: here the untouched base cell.
        assert_eq!(r.layers[1][1].flags, FLAG_HAS_BG | FLAG_HAS_GLYPH);
        assert_eq!(r.layers[1][1].bg, r.layers[0][1].bg);
        // Higher-layer glyph with a real background: both, with its own color.
        assert_eq!(r.layers[1][2].flags, FLAG_HAS_BG | FLAG_HAS_GLYPH);
        assert_eq!(r.layers[1][2].bg, [255, 0, 0, 0]);
    }

    #[test]
    fn draw_layers_full_frame_drops_a_removed_higher_layer() {
        let mut r = WgpuBackendBuilder::new()
            .grid_size(2, 1)
            .build()
            .expect("default-font builds");
        let tile = Tile::new('Q', Style::new());
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

        let mut r = WgpuBackendBuilder::new()
            .grid_size(3, 1)
            .build()
            .expect("default-font builds");

        let mut grid = Grid::new(3, 1);
        grid.write_span(0, 0, 0, &["C="], Style::new().bg(RED))
            .expect("2x1 span fits");
        let anchor = *grid.tile(0, (0, 0)).expect("anchor");
        let covered = *grid.tile(0, (1, 0)).expect("covered");
        assert!(covered.span_offset().is_some());

        r.draw_layers(
            [
                DrawCell::on_layer(0, Pos::new(0, 0), &anchor),
                DrawCell::on_layer(0, Pos::new(1, 0), &covered),
            ]
            .into_iter(),
        )
        .expect("draw_layers");

        // The covered cell paints the span's background and no glyph of its own.
        assert_eq!(r.layers[0][1].flags, FLAG_HAS_BG);
        assert_eq!(r.layers[0][1].bg, [255, 0, 0, 0]);
    }

    #[test]
    fn flatten_packs_every_layer_into_one_contiguous_upload() {
        let mut r = WgpuBackendBuilder::new()
            .grid_size(2, 2)
            .build()
            .expect("default-font builds");
        let tile = Tile::new('#', Style::new());
        r.draw_layers(core::iter::once(DrawCell::on_layer(
            1,
            Pos::new(0, 0),
            &tile,
        )))
        .expect("draw_layers");

        r.flatten_layers();
        assert_eq!(r.upload.len(), 8, "two layers of four cells");
        assert_eq!(r.ranges.len(), 2);
        assert_eq!((r.ranges[0].start, r.ranges[0].count), (0, 4));
        assert_eq!((r.ranges[1].start, r.ranges[1].count), (4, 4));
        // Each layer's slice must hold that layer's own cells, in order.
        assert_eq!(&r.upload[0..4], r.layers[0].as_slice());
        assert_eq!(&r.upload[4..8], r.layers[1].as_slice());
    }

    #[test]
    fn resize_rebuilds_the_grid_and_reports_the_new_size() {
        use retroglyph_core::grid::Size;
        let mut r = WgpuBackendBuilder::new()
            .grid_size(4, 4)
            .build()
            .expect("default-font builds");
        r.resize(Size::new(10, 3));
        assert_eq!(r.size(), Size::new(10, 3));
        assert_eq!(r.layers.len(), 1);
        assert_eq!(r.layers[0].len(), 30);
    }

    #[test]
    fn present_without_a_device_is_a_no_op() {
        use retroglyph_window::Presenter as _;
        let mut r = WgpuBackendBuilder::new()
            .grid_size(2, 1)
            .build()
            .expect("default-font builds");
        assert!(r.present().is_ok(), "no device is not an error");
    }

    /// retroglyph#726: a `Color::Default`-background span on a higher layer must not smear the
    /// anchor's column across the whole footprint. Layer 0 has a different background under each
    /// half of the span (red under the anchor, blue under the covered cell); the covered cell's
    /// `Default` background must inherit from *its own* column (blue), matching
    /// `retroglyph-software`'s `resolve_cell_bg`, not the anchor's (red).
    #[test]
    fn draw_layers_resolves_a_span_covered_cells_default_background_at_its_own_column() {
        use retroglyph_core::grid::Grid;

        const BLUE: Color = Color::Rgb { r: 0, g: 0, b: 255 };

        let mut r = WgpuBackendBuilder::new()
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
            [0, 0, 255, 0],
            "covered cell inherits its own column's background, not the anchor's"
        );
    }

    /// Covered-cell suppression is grid state, not a tileset feature, so it holds with the
    /// `tilesets` feature off too: a span with no sprite behind it renders as its anchor glyph
    /// alone, the same on every pixel backend.
    #[test]
    fn draw_layers_suppresses_covered_glyphs_without_a_sprite() {
        use retroglyph_core::grid::Grid;

        let mut r = WgpuBackendBuilder::new()
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
}

/// Sprite bookkeeping that has to stay in lockstep with the glyph layers (retroglyph#727) and the
/// diagnostic for a tint that had no sprite to land on (retroglyph#564).
#[cfg(all(test, feature = "default-font", feature = "tilesets"))]
mod sprite_layer_tests {
    use crate::{WgpuBackendBuilder, WgpuRenderer};
    use retroglyph_core::backend::{DrawCell, Output};
    use retroglyph_core::color::{Style, Tint};
    use retroglyph_core::grid::{Pos, Size};
    use retroglyph_core::tile::Tile;
    use retroglyph_window::tileset::{Codepage, TilesetOptions};

    /// A one-tile 8x16 opaque PNG mapped to `'S'`, encoded once at test time.
    fn renderer_with_sprite(cols: u16, rows: u16) -> WgpuRenderer {
        let mut img = image::RgbaImage::new(8, 16);
        for px in img.pixels_mut() {
            *px = image::Rgba([0xFF, 0xFF, 0xFF, 0xFF]);
        }
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encode png");
        let opts = TilesetOptions::builder(png)
            .tile_size(8, 16)
            .columns(1)
            .codepage(Codepage::Custom(vec!['S']))
            .build()
            .expect("valid one-tile tileset");
        WgpuBackendBuilder::new()
            .grid_size(cols, rows)
            .tileset(opts)
            .build()
            .expect("tileset builds")
    }

    /// Draws a sprite on two layers, so `sprite_layers` has more than the (always present) base
    /// layer entry to be reset.
    fn renderer_with_a_sprite_on_two_layers() -> WgpuRenderer {
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
        let mut r = renderer_with_a_sprite_on_two_layers();
        assert_eq!(r.sprite_layers.len(), 2);
        assert!(!r.sprite_layers[0].is_empty());

        r.resize(Size::new(2, 2));

        assert_eq!(r.sprite_layers.len(), 1);
        assert!(r.sprite_layers[0].is_empty());
    }

    #[test]
    fn flatten_packs_sprite_layers_in_lockstep_with_the_glyph_layers() {
        let mut r = renderer_with_a_sprite_on_two_layers();
        r.flatten_layers();
        assert_eq!(r.sprite_ranges.len(), r.ranges.len());
        assert_eq!(r.sprite_upload.len(), 2, "one sprite per layer");
        assert_eq!((r.sprite_ranges[0].start, r.sprite_ranges[0].count), (0, 1));
        assert_eq!((r.sprite_ranges[1].start, r.sprite_ranges[1].count), (1, 1));
    }

    /// retroglyph#564: a tint on a cell whose glyph resolved to a bitmap font rather than a sprite
    /// is silently dropped, so it is reported once per glyph.
    #[test]
    fn a_tint_on_a_font_glyph_is_reported_once() {
        let mut r = renderer_with_sprite(1, 1);
        let tile = Tile::new('X', Style::new());
        for _ in 0..3 {
            r.draw_layers(core::iter::once(
                DrawCell::on_layer(0, Pos::new(0, 0), &tile).with_tint(Tint::multiply(1, 2, 3)),
            ))
            .expect("draw_layers is infallible");
        }
        assert!(r.warned_dropped_tint.contains(&'X'));
        assert_eq!(
            r.warned_dropped_tint.len(),
            1,
            "reported once, not per frame"
        );
    }

    #[test]
    fn a_tint_on_a_sprite_cell_is_not_reported() {
        let mut r = renderer_with_sprite(1, 1);
        let tile = Tile::new('S', Style::new());
        r.draw_layers(core::iter::once(
            DrawCell::on_layer(0, Pos::new(0, 0), &tile).with_tint(Tint::multiply(1, 2, 3)),
        ))
        .expect("draw_layers is infallible");
        assert!(r.warned_dropped_tint.is_empty(), "the tint was applied");
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
}

/// Cross-backend `Output` conformance (retroglyph#763).
///
/// `WgpuRenderer` implements neither `Input` nor `Cursor`, so only
/// [`assert_output_contract`](retroglyph_core::testing::conformance::assert_output_contract)
/// applies; the other two harnesses have no facet here to check.
#[cfg(all(test, feature = "default-font"))]
mod conformance {
    use crate::{Cell, WgpuBackendBuilder, WgpuRenderer};
    use retroglyph_core::backend::{DrawCell, Output};
    use retroglyph_core::grid::HasSize as _;
    use retroglyph_core::grid::Size;
    use retroglyph_core::testing::conformance::{Observable, fnv1a};

    /// A renderer plus the instance data as of the previous [`snapshot`](Observable::snapshot), so
    /// each call can hash what changed rather than the whole frame.
    ///
    /// `WgpuRenderer` has no CPU-readable framebuffer without a device (see `headless`'s pixel
    /// readback tests), but its `layers` field is the exact per-cell data every draw uploads
    /// verbatim on the next present, so hashing that is equivalent to hashing the frame for
    /// everything this contract checks: clear, resize, and out-of-range handling never reach the
    /// GPU at all.
    struct WgpuObserver {
        renderer: WgpuRenderer,
        previous: Vec<Vec<Cell>>,
    }

    impl WgpuObserver {
        fn new(size: Size) -> Self {
            let renderer = WgpuBackendBuilder::new()
                .grid_size(size.width().max(1), size.height().max(1))
                .build()
                .expect("default-font builds");
            let previous = renderer.layers.clone();
            Self { renderer, previous }
        }
    }

    impl Output for WgpuObserver {
        type Error = core::convert::Infallible;

        fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
        where
            I: Iterator<Item = DrawCell<'a>>,
        {
            self.renderer.draw_layers(content)
        }

        fn needs_full_frame(&self) -> bool {
            self.renderer.needs_full_frame()
        }

        fn composites_layers(&self) -> bool {
            self.renderer.composites_layers()
        }

        fn flush(&mut self) -> Result<(), Self::Error> {
            self.renderer.flush()
        }

        fn size(&self) -> Size {
            self.renderer.size()
        }

        fn clear(&mut self) -> Result<(), Self::Error> {
            self.renderer.clear()
        }

        fn resize(&mut self, size: Size) {
            self.renderer.resize(size);
        }
    }

    impl Observable for WgpuObserver {
        fn snapshot(&mut self) -> u64 {
            let current = &self.renderer.layers;
            let mut hash = fnv1a(b"wgpu-diff");
            for (layer, (was, now)) in self.previous.iter().zip(current.iter()).enumerate() {
                for (index, (was, now)) in was.iter().zip(now.iter()).enumerate() {
                    if was != now {
                        hash ^= fnv1a(&(layer as u64).to_ne_bytes());
                        hash ^= fnv1a(&(index as u64).to_ne_bytes());
                        hash ^= fnv1a(bytemuck::bytes_of(now));
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
    fn output_contract() {
        retroglyph_core::testing::conformance::assert_output_contract(WgpuObserver::new);
    }
}
