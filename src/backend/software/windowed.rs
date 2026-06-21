//! `WindowedBackend` subtrait — window-surface operations for backends that
//! render to a winit window.
//!
//! Only windowed backends ([`SoftwareRenderer`](super::SoftwareRenderer), and
//! future `GlowRenderer`/`WgpuRenderer`) implement this.  `Headless` and
//! `Crossterm` implement only [`Backend`](crate::backend::Backend).

//! `WindowedBackend` subtrait — window-surface operations for backends that
//! render to a winit window.
//!
//! Only windowed backends ([`SoftwareRenderer`](super::SoftwareRenderer), and
//! future `GlowRenderer`/`WgpuRenderer`) implement this.  `Headless` and
//! `Crossterm` implement only [`Backend`](crate::backend::Backend).
//!
//! TODO: If backends are moved outside this monorepo, consider moving
//! `WindowedBackend` to `crate::backend` so it lives alongside `Backend`.

use crate::backend::Backend;
use crate::event::Event;
use std::sync::Arc;
use winit::window::Window;

use super::SurfaceError;

/// A [`Backend`] that can present rendered frames to a winit window surface.
///
/// # Backend-specific behavior
///
/// | Backend | `present()` | `init_surface()` |
/// |---|---|---|
/// | [`SoftwareRenderer`](super::SoftwareRenderer) | Copies pixel buffer to softbuffer surface | Creates `softbuffer::Context` + `Surface` |
/// | `GlowRenderer` (future) | Uploads data textures + draws full-screen quad | Creates WebGL2 context from canvas |
/// | `WgpuRenderer` (future) | Submits render pass + presents swap chain | Creates `wgpu::Surface` + `Device` |
pub trait WindowedBackend: Backend {
    /// Present the current frame to the window surface.
    ///
    /// Called by the `ApplicationHandler` after each game tick.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError::Surface`] if the surface buffer can't be
    /// acquired or presented (e.g., context lost on WASM or DRI/KMS page
    /// flip pending).  The caller should log and continue — a lost frame
    /// is not fatal.
    fn present(&mut self) -> Result<(), SurfaceError>;

    /// Initialize the window surface.
    ///
    /// Called from `ApplicationHandler::resumed()`. The backend creates its
    /// platform-specific surface (softbuffer context, WebGL context, wgpu
    /// surface) from the window.
    ///
    /// # Errors
    ///
    /// Returns a [`SurfaceError`] if context or surface creation fails.
    fn init_surface(&mut self, window: &Arc<Window>) -> Result<(), SurfaceError>;

    /// Resize the window surface.
    ///
    /// Called from `ApplicationHandler::window_event` on `WindowEvent::Resized`.
    fn resize_surface(&mut self, width: u32, height: u32);

    /// Return the cell size in pixels `(width, height)`.
    ///
    /// Returns pixel dimensions as `(u32, u32)` rather than
    /// [`Size`](crate::grid::Size) because grid coordinates are `u16` but
    /// pixel arithmetic uses `u32` (from winit `PhysicalSize`).
    fn cell_size(&self) -> (u32, u32);

    /// Push an external event into the backend's event buffer.
    ///
    /// Called by `ApplicationHandler` when winit events are translated.
    /// The default delegates to [`Backend::push_event`].
    fn push_window_event(&mut self, event: Event) {
        Backend::push_event(self, event);
    }
}
