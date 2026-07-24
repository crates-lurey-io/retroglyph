//! GPU rendering backend for retroglyph: native OpenGL 3.3 core and browser WebGL2, from a single
//! codebase via [`glow`].
//!
//! # Architecture
//!
//! [`GlBackendBuilder`] holds configuration (font, grid size, integer scale) and
//! [`build`](GlBackendBuilder::build)s a [`GlRenderer`]. The renderer maintains a CPU-side
//! instance array (one entry per cell: glyph id + fg/bg RGB) and a GL context that is created
//! lazily when the windowing loop calls
//! [`Presenter::init_surface`]:
//!
//! ```text
//! GlBackendBuilder (font, grid size, scale)
//!   |  .build()
//!   v
//! GlRenderer
//!   implements retroglyph_core::{Output, Input, Cursor} (= Backend)
//!   implements retroglyph_window::Presenter (an Output supertrait)
//!   |
//!   |  init_surface(window) -> GlContext (glutin native / WebGL2 wasm) + GlResources
//!   v
//! one draw_elements_instanced per present(): a unit quad instanced cols*rows times,
//! sampling an R8 glyph atlas (TEXTURE_2D_ARRAY), blending mix(bg, fg, coverage).
//! ```
//!
//! Rendering is one instanced draw call per frame (the beamterm/alacritty/xterm.js model). Layers
//! are flattened by the core `Terminal` before they reach this backend
//! ([`composites_layers`](retroglyph_core::backend::Output::composites_layers) returns `false`),
//! so v1 does no GPU-side layer compositing. The GPU redraws every cell each frame, so there is no
//! orphaned-pixel problem despite this backend not requesting full frames
//! ([`needs_full_frame`](retroglyph_core::backend::Output::needs_full_frame) returns `false`): it
//! only needs the changed-cell diff to keep its instance array current.
//!
//! # Platform split
//!
//! Native builds create the GL context from the window's raw handles via `glutin`
//! (`context_native.rs`); wasm builds acquire a WebGL2 context from the winit `<canvas>`
//! (`context_wasm.rs`). Both expose the same internal `GlContext` API, so the renderer body has no
//! `cfg`.

pub mod config;

mod atlas;
mod error;
mod renderer;
mod shaders;

// Headless offscreen render tests: create an EGL surfaceless context, run the real pipeline into an
// FBO, and read the pixels back to assert on them (issue #376). Linux/EGL only -- see the module
// docs -- and gated to `default-font` since the tests build a renderer from the embedded atlas.
#[cfg(all(test, target_os = "linux", feature = "default-font"))]
mod headless;

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
pub use retroglyph_window::font::{self as font, BitmapFont};

use context::GlContext;
use renderer::{GlResources, Instance};
use retroglyph_core::backend::{Cursor, Input, Output};
use retroglyph_core::event::Event;
use retroglyph_core::grid::{Pos, Size};
use retroglyph_core::tile::Tile;
use retroglyph_window::palette::{DEFAULT_BG, DEFAULT_FG};
use retroglyph_window::{Presenter, WindowHandle};
use shaders::GlslFlavor;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

// Compile the crate README's code blocks as doctests so the quick start can't silently rot.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

/// The live GL renderer: a [`Backend`](retroglyph_core::Backend) and [`Presenter`].
///
/// Build one with [`GlBackendBuilder`]. Before the windowing loop calls
/// [`init_surface`](Presenter::init_surface) there is no GL context; drawing updates only the
/// CPU-side instance array, and [`present`](Presenter::present) is a no-op. Once the surface
/// exists, `present` uploads changed cells and issues the single instanced draw call.
pub struct GlRenderer {
    font: BitmapFont,
    cols: u16,
    rows: u16,
    cell_w: u32,
    cell_h: u32,
    /// Atlas layer for the space glyph, used to initialize blank cells.
    space_glyph: u16,
    /// One entry per cell (`cols * rows`), row-major. Patched by [`Output::draw`]; the changed
    /// sub-range is uploaded on [`present`](Presenter::present).
    instances: Vec<Instance>,
    /// The sub-range of `instances` changed since the last GPU upload (see [`DirtyRange`]).
    dirty: DirtyRange,
    /// Input events pushed by the windowing loop, drained by [`Input::poll_event`].
    events: VecDeque<Event>,
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
    /// Builds a renderer for the given font, grid size, and scale. Called by
    /// [`GlBackendBuilder::build`].
    pub(crate) fn new(font: BitmapFont, cols: u16, rows: u16, scale: u16) -> Self {
        let cell_w = u32::from(font.glyph_width) * u32::from(scale);
        let cell_h = u32::from(font.glyph_height) * u32::from(scale);
        let space_glyph = u16::from(font.char_to_index(' '));
        let blank = Instance::new(space_glyph, to_arr(DEFAULT_FG), to_arr(DEFAULT_BG), 0, 0);
        let count = usize::from(cols) * usize::from(rows);
        let instances = vec![blank; count];
        Self {
            font,
            cols,
            rows,
            cell_w,
            cell_h,
            space_glyph,
            instances,
            dirty: DirtyRange::full(count),
            events: VecDeque::new(),
            surface_size: (cols_px(cols, cell_w), cols_px(rows, cell_h)),
            gpu: None,
        }
    }

    /// The blank instance (space glyph, default colors, no offset) used to clear/resize cells.
    const fn blank(&self) -> Instance {
        Instance::new(
            self.space_glyph,
            to_arr(DEFAULT_FG),
            to_arr(DEFAULT_BG),
            0,
            0,
        )
    }

    /// Total cell count for the current grid.
    fn cell_count(&self) -> usize {
        usize::from(self.cols) * usize::from(self.rows)
    }

    /// Writes one tile into the instance array at `pos`, if in bounds.
    fn write_tile(&mut self, pos: Pos, tile: &Tile) {
        let (x, y) = (usize::from(pos.x), usize::from(pos.y));
        let cols = usize::from(self.cols);
        if x >= cols || y >= usize::from(self.rows) {
            return;
        }
        let glyph = u16::from(self.font.char_to_index(tile.glyph()));
        let fg = to_arr(tile.style().foreground().resolve_rgb(DEFAULT_FG));
        let bg = to_arr(tile.style().background().resolve_rgb(DEFAULT_BG));
        let idx = y * cols + x;
        self.instances[idx] = Instance::new(glyph, fg, bg, tile.dx(), tile.dy());
        self.dirty.insert(idx);
    }

    /// Pushes an input event (called by the windowing loop). Public inherent method so the
    /// [`Input`] impl can forward to it without ambiguity.
    pub fn push_event(&mut self, event: Event) {
        self.events.push_back(event);
    }

    /// Builds the GL resources for the current instance array on an already-current context:
    /// compiles the program, uploads the glyph atlas and the full instance buffer, and sets the
    /// glyph-size and projection uniforms.
    ///
    /// Shared by [`Presenter::init_surface`] (windowed) and the headless render-test path so both
    /// exercise byte-for-byte the same setup -- the point of the render tests is to catch a break
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
        let atlas = atlas::AtlasData::build(&self.font);
        let res = GlResources::new(gl, flavor, &atlas, self.cell_count())?;
        res.upload(gl, &self.instances);
        res.set_glyph_size(
            gl,
            f32::from(self.font.glyph_width),
            f32::from(self.font.glyph_height),
        );
        res.set_projection(
            gl,
            w as f32,
            h as f32,
            self.cell_w as f32,
            self.cell_h as f32,
            i32::from(self.cols),
        );
        Ok(res)
    }
}

/// `(u8, u8, u8)` -> `[u8; 3]`, for packing resolved colors into an [`Instance`].
const fn to_arr(rgb: (u8, u8, u8)) -> [u8; 3] {
    [rgb.0, rgb.1, rgb.2]
}

/// Cells along one axis times the per-cell pixel size.
const fn cols_px(cells: u16, cell: u32) -> u32 {
    cells as u32 * cell
}

/// Half-open range `[lo, hi)` of instance indices changed since the last GPU upload.
///
/// [`present`](Presenter::present) uploads exactly `instances[lo..hi]` instead of the whole
/// `cols * rows` buffer. A single contiguous range keeps [`insert`](Self::insert) O(1) and captures
/// the common terminal update (a changed line or rectangular region) tightly; scattered writes
/// conservatively widen to their bounding range rather than tracking each cell individually. The
/// empty state uses `lo > hi` (`usize::MAX .. 0`) so a fresh range needs no `Option`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct DirtyRange {
    lo: usize,
    hi: usize,
}

impl DirtyRange {
    /// The empty range: nothing pending. `lo >= hi`, so [`take`](Self::take) yields `None`.
    const EMPTY: Self = Self {
        lo: usize::MAX,
        hi: 0,
    };

    /// A range covering every cell in `0..count` (a full re-upload). Empty when `count == 0`.
    const fn full(count: usize) -> Self {
        Self { lo: 0, hi: count }
    }

    /// Whether the range covers no cells.
    const fn is_empty(&self) -> bool {
        self.lo >= self.hi
    }

    /// Widens the range to include cell `idx`.
    const fn insert(&mut self, idx: usize) {
        if idx < self.lo {
            self.lo = idx;
        }
        if idx + 1 > self.hi {
            self.hi = idx + 1;
        }
    }

    /// Returns the pending range and resets to empty, or `None` when nothing is dirty.
    fn take(&mut self) -> Option<core::ops::Range<usize>> {
        let range = (!self.is_empty()).then_some(self.lo..self.hi);
        *self = Self::EMPTY;
        range
    }
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
        let blank = self.blank();
        for cell in &mut self.instances {
            *cell = blank;
        }
        self.dirty = DirtyRange::full(self.instances.len());
        Ok(())
    }

    fn resize(&mut self, size: Size) {
        self.cols = size.width;
        self.rows = size.height;
        let blank = self.blank();
        self.instances = vec![blank; self.cell_count()];
        self.dirty = DirtyRange::full(self.instances.len());
    }
}

// ── Input ────────────────────────────────────────────────────────────────────

impl Input for GlRenderer {
    fn poll_event(&mut self, _timeout: Duration) -> Option<Event> {
        // Non-blocking: the windowing loop drives frame timing.
        self.events.pop_front()
    }

    fn push_event(&mut self, event: Event) {
        Self::push_event(self, event);
    }
}

// ── Cursor (no hardware cursor in windowed mode) ─────────────────────────────

impl Cursor for GlRenderer {}

// ── Presenter ────────────────────────────────────────────────────────────────

impl Presenter for GlRenderer {
    type SurfaceError = SurfaceError;

    fn init_surface(&mut self, window: Arc<dyn WindowHandle>) -> Result<(), SurfaceError> {
        let (w, h) = self.surface_size;
        let ctx = GlContext::new(&window, w, h)?;
        let res = self.build_resources(&ctx.gl, ctx.flavor())?;
        // The whole buffer was just uploaded above; nothing is pending.
        self.dirty = DirtyRange::EMPTY;
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
        let (cell_w, cell_h) = (self.cell_w as f32, self.cell_h as f32);

        // Split borrow: `gpu` borrows `self.gpu`, while `self.instances`/`self.dirty` are disjoint
        // fields, so direct field access to them stays legal below.
        let Some(gpu) = self.gpu.as_mut() else {
            // No surface yet: nothing to present.
            return Ok(());
        };

        // Keep the GPU instance buffer sized to the current grid (grid resizes arrive via
        // `Output::resize`, out of band from surface resizes).
        if gpu.res.capacity() != cell_count {
            gpu.res.resize_instances(&gpu.ctx.gl, cell_count);
            // The reallocated buffer holds no cell data yet; re-upload everything.
            self.dirty = DirtyRange::full(cell_count);
        }
        if let Some(range) = self.dirty.take() {
            // Clamp against the current instance count in case a grid resize shrank the array
            // between the write and this present.
            let end = range.end.min(self.instances.len());
            let start = range.start.min(end);
            gpu.res
                .upload_range(&gpu.ctx.gl, start, &self.instances[start..end]);
        }

        gpu.res
            .set_projection(&gpu.ctx.gl, w as f32, h as f32, cell_w, cell_h, cols);
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        gpu.res.draw(&gpu.ctx.gl, cell_count as i32);
        gpu.ctx.present()
    }

    fn cell_size(&self) -> (u32, u32) {
        (self.cell_w, self.cell_h)
    }
}

impl Drop for GlRenderer {
    fn drop(&mut self) {
        if let Some(gpu) = &self.gpu {
            gpu.res.delete(&gpu.ctx.gl);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DirtyRange;

    #[test]
    fn empty_takes_nothing() {
        let mut d = DirtyRange::EMPTY;
        assert!(d.is_empty());
        assert_eq!(d.take(), None);
    }

    #[test]
    fn full_covers_all_cells() {
        assert_eq!(DirtyRange::full(6).take(), Some(0..6));
        // A zero-cell grid is empty, not a `0..0` upload.
        assert!(DirtyRange::full(0).is_empty());
    }

    #[test]
    fn single_insert_is_a_one_cell_range() {
        let mut d = DirtyRange::EMPTY;
        d.insert(3);
        assert_eq!(d.take(), Some(3..4));
    }

    #[test]
    fn scattered_inserts_widen_to_the_bounding_range() {
        let mut d = DirtyRange::EMPTY;
        d.insert(10);
        d.insert(2);
        d.insert(7);
        // Bounding range of {2, 7, 10} is [2, 11).
        assert_eq!(d.take(), Some(2..11));
    }

    #[test]
    fn take_resets_to_empty() {
        let mut d = DirtyRange::full(4);
        assert_eq!(d.take(), Some(0..4));
        assert!(d.is_empty());
        assert_eq!(d.take(), None);
    }
}

#[cfg(all(test, feature = "default-font"))]
mod offset_tests {
    use crate::GlBackendBuilder;
    use retroglyph_core::backend::Output;
    use retroglyph_core::grid::Pos;
    use retroglyph_core::style::Style;
    use retroglyph_core::tile::Tile;

    #[test]
    fn draw_records_sub_cell_offset_in_the_instance() {
        let mut r = GlBackendBuilder::new()
            .grid_size(4, 2)
            .build()
            .expect("default-font builds");
        let tile = Tile::new('A', Style::new()).with_offset(-3, 5);
        r.draw(core::iter::once((Pos::new(1, 0), &tile, None)))
            .expect("draw is infallible");

        let inst = r.instances[1];
        assert_eq!(inst.dx, -3);
        assert_eq!(inst.dy, 5);
        assert_eq!(inst.glyph, u16::from(r.font.char_to_index('A')));
    }

    #[test]
    fn blank_cells_carry_no_offset() {
        let r = GlBackendBuilder::new()
            .grid_size(3, 3)
            .build()
            .expect("default-font builds");
        assert!(r.instances.iter().all(|i| i.dx == 0 && i.dy == 0));
    }
}
