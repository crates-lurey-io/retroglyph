//! Native window surface: a softbuffer context + surface.
//!
//! Selected by `lib.rs` on every non-wasm32 target. The wasm32 counterpart in
//! `surface_wasm.rs` exposes the same `WindowSurface::new`/`resize`/`present`
//! API and its own `SurfaceError`, so the renderer drives either without
//! `cfg` in its body.

// `WindowSurface` is crate-internal, so `pub(crate)` is the correct visibility
// (`unreachable_pub` agrees). The nursery `redundant_pub_crate` lint disagrees
// only because this module is not itself `pub`; the two lints conflict for the
// module-per-platform pattern, and `pub(crate)` is the honest choice.
#![allow(clippy::redundant_pub_crate)]

use retroglyph_window::WindowHandle;
use std::num::NonZeroU32;
use std::sync::Arc;

/// Softbuffer-backed window surface.
///
/// Holds both the `Context` and `Surface`. The `_context` must outlive
/// `surface` (softbuffer requires it), but is only stored, not read. The
/// handle type is `Arc<dyn WindowHandle>` (raw-window-handle), not a winit
/// type: this crate rasterizes and presents, and any windowing library that
/// yields raw handles can drive it (see `retroglyph_window::Presenter`).
pub(crate) struct WindowSurface {
    _context: softbuffer::Context<Arc<dyn WindowHandle>>,
    surface: softbuffer::Surface<Arc<dyn WindowHandle>, Arc<dyn WindowHandle>>,
}

/// Errors creating or presenting the native window surface.
#[derive(Debug)]
pub enum SurfaceError {
    /// Failed to create the softbuffer context from the window.
    Context(softbuffer::SoftBufferError),
    /// Failed to create or present the softbuffer surface.
    Surface(softbuffer::SoftBufferError),
}

impl core::fmt::Display for SurfaceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Context(e) => write!(f, "softbuffer context: {e}"),
            Self::Surface(e) => write!(f, "softbuffer surface: {e}"),
        }
    }
}

impl std::error::Error for SurfaceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Context(e) | Self::Surface(e) => Some(e),
        }
    }
}

impl WindowSurface {
    /// Creates a softbuffer context and surface from a raw window handle.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError`] if the softbuffer context or surface cannot be
    /// created.
    pub(crate) fn new(window: Arc<dyn WindowHandle>) -> Result<Self, SurfaceError> {
        let context = softbuffer::Context::new(window.clone()).map_err(SurfaceError::Context)?;
        let surface = softbuffer::Surface::new(&context, window).map_err(SurfaceError::Surface)?;
        Ok(Self {
            _context: context,
            surface,
        })
    }

    /// Resizes the surface to `width` x `height` pixels.
    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        if let (Some(w), Some(h)) = (NonZeroU32::new(width), NonZeroU32::new(height)) {
            let _ = self.surface.resize(w, h);
        }
    }

    /// Copies `pixels` (`0x00RRGGBB`) into the surface buffer and presents.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError::Surface`] if the softbuffer buffer cannot be
    /// acquired or presented.
    pub(crate) fn present(&mut self, pixels: &[u32]) -> Result<(), SurfaceError> {
        let mut buffer = self.surface.buffer_mut().map_err(SurfaceError::Surface)?;
        if pixels.len() == buffer.len() {
            buffer.copy_from_slice(pixels);
        } else {
            buffer.fill(0);
        }
        buffer.present().map_err(SurfaceError::Surface)
    }
}
