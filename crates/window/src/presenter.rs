//! The rasterization seam between the shared windowing layer and renderer
//! crates.
//!
//! A [`Presenter`] is the *output* half of
//! [`Backend`](retroglyph_core::Backend) plus window-surface operations. It
//! deliberately has no input methods: the winit loop owns input, and
//! [`WindowBackend`](crate::WindowBackend) forwards translated events into its
//! own queue.
//!
//! | Presenter | `present()` | `init_surface()` |
//! |---|---|---|
//! | `SoftwareRenderer` (retroglyph-software) | Copies pixel buffer to softbuffer surface | Creates `softbuffer::Context` + `Surface` |
//! | `WgpuRenderer` (future) | Submits render pass + presents swap chain | Creates `wgpu::Surface` + `Device` |
//! | `GlRenderer` (future) | Draws full-screen quad + swaps buffers | Creates GL context from the window |

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use retroglyph_core::backend::BackendError;
use retroglyph_core::grid::{Pos, Size};
use retroglyph_core::tile::Tile;
use std::sync::Arc;

/// A window/display handle pair, as one trait.
///
/// The seam speaks [`raw-window-handle`](raw_window_handle), not
/// `winit::window::Window`, so presenters are windowing-library-agnostic:
/// softbuffer, wgpu, and glutin all accept these handles directly, and a
/// non-winit loop (SDL2, tao) can drive the same presenters later. It also
/// keeps winit's frequent major bumps out of every renderer crate's public
/// API; only `retroglyph-window` tracks winit.
///
/// `raw-window-handle` has no combined trait, and surface libraries need to
/// *own* the handle (softbuffer stores it for the surface's lifetime), so
/// presenters receive `Arc<dyn WindowHandle>` -- rwh implements the handle
/// traits for `Arc<H: ?Sized>`, so the trait object passes straight into
/// `softbuffer::Surface::new` / `wgpu::Instance::create_surface`.
pub trait WindowHandle: HasWindowHandle + HasDisplayHandle {}

impl<T: HasWindowHandle + HasDisplayHandle + ?Sized> WindowHandle for T {}

/// A renderer that rasterizes grid content and presents it to a winit window
/// surface.
///
/// Mirrors the output half of [`Backend`](retroglyph_core::Backend) (`draw`,
/// `draw_layers`, `flush`, `size`, `clear`, `resize`) so
/// [`WindowBackend`](crate::WindowBackend) can delegate those methods
/// wholesale, and adds the surface lifecycle (`init_surface`,
/// `resize_surface`, `present`, `cell_size`) that the winit loop drives.
///
/// The `needs_full_frame` and `composites_layers` defaults are `true`: every
/// windowed presenter is a pixel-family backend that composites layers
/// itself, receiving the raw per-layer stream instead of a pre-flattened
/// single layer. Only character-cell terminal backends return `false`, and
/// those implement [`Backend`](retroglyph_core::Backend) directly instead of
/// this trait.
pub trait Presenter {
    /// Rasterization error (mirrors `Backend::Error`).
    ///
    /// In-memory rasterizers are infallible and use
    /// [`core::convert::Infallible`].
    type Error: BackendError;

    /// Surface lifecycle error (context creation, buffer acquisition,
    /// present).
    type SurfaceError: core::fmt::Debug + core::fmt::Display;

    /// Rasterize changed cells (single layer).
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] if rasterization fails.
    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (Pos, &'a Tile)>;

    /// Rasterize the full layered frame.
    ///
    /// Because [`needs_full_frame`](Self::needs_full_frame) defaults to
    /// `true`, this receives every cell of every allocated layer and should
    /// clear its target before drawing.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] if rasterization fails.
    fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u8, Pos, &'a Tile)>;

    /// Flush buffered rasterization work.
    ///
    /// Distinct from [`present`](Self::present): `flush` completes drawing
    /// into the presenter's own target; `present` pushes that target to the
    /// OS window.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] if the flush fails.
    fn flush(&mut self) -> Result<(), Self::Error>;

    /// Current grid dimensions in cells.
    #[must_use]
    fn size(&self) -> Size;

    /// Clear the rasterization target.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] if the clear fails.
    fn clear(&mut self) -> Result<(), Self::Error>;

    /// Resize the grid (in cells), reallocating the rasterization target.
    fn resize(&mut self, size: Size);

    /// Whether the full frame is required on every `draw_layers` call.
    ///
    /// Defaults to `true` for the windowed family (sub-cell offsets spill
    /// pixels across cells; partial redraws would leave orphans).
    #[must_use]
    fn needs_full_frame(&self) -> bool {
        true
    }

    /// Whether this presenter composites layers itself, receiving the raw
    /// `(layer, Pos, Tile)` stream instead of a pre-flattened single layer.
    ///
    /// Defaults to `true` for the windowed family.
    #[must_use]
    fn composites_layers(&self) -> bool {
        true
    }

    /// Initialize the window surface.
    ///
    /// Called once from the loop's `resumed` handler. The presenter creates
    /// its platform surface (softbuffer surface, wgpu device+surface, GL
    /// context) from the raw window/display handles.
    ///
    /// # Errors
    ///
    /// Returns [`Self::SurfaceError`] if surface or context creation fails.
    fn init_surface(&mut self, window: Arc<dyn WindowHandle>) -> Result<(), Self::SurfaceError>;

    /// Resize the window surface to a new physical pixel size.
    ///
    /// Called from the winit loop on `WindowEvent::Resized`.
    fn resize_surface(&mut self, width: u32, height: u32);

    /// Present the rasterized frame to the window surface.
    ///
    /// Called by the winit loop after each app tick. A lost frame is not
    /// fatal; the loop logs the error and continues.
    ///
    /// # Errors
    ///
    /// Returns [`Self::SurfaceError`] if the surface buffer can't be acquired
    /// or presented (e.g. context lost on wasm, page flip pending on
    /// DRI/KMS).
    fn present(&mut self) -> Result<(), Self::SurfaceError>;

    /// Cell size in physical pixels `(width, height)`.
    ///
    /// `(u32, u32)` rather than [`Size`] because grid coordinates are `u16`
    /// but pixel arithmetic uses `u32` (winit `PhysicalSize`).
    #[must_use]
    fn cell_size(&self) -> (u32, u32);
}
