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
    /// Surface width in pixels, tracked so [`present`](Self::present) can build
    /// a damage `Rect` (softbuffer's `Buffer` doesn't expose its width).
    width: u32,
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

// Inherits the default `is_recoverable() -> true`: softbuffer's error enum has no
// `Lost`/`Outdated`/`Timeout` discrimination the way `wgpu::SurfaceError` does, so every present
// failure here is treated as potentially transient, matching this crate's existing (pre-trait)
// behavior.
impl retroglyph_window::RecoverableError for SurfaceError {}

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
            width: 0,
        })
    }

    /// Resizes the surface to `width` x `height` pixels.
    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        if let (Some(w), Some(h)) = (NonZeroU32::new(width), NonZeroU32::new(height)) {
            let _ = self.surface.resize(w, h);
            self.width = width;
        }
    }

    /// Copies `pixels` (`0x00RRGGBB`) into the surface buffer and presents only
    /// the changed row band `[y0, y1)` via `present_with_damage`, so softbuffer
    /// blits just those rows to the window.
    ///
    /// Falls back to a full present if the band or width is degenerate, or if
    /// the buffer size does not match (a resize is mid-flight).
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError::Surface`] if the softbuffer buffer cannot be
    /// acquired or presented.
    pub(crate) fn present(
        &mut self,
        pixels: &[u32],
        damage: (u32, u32),
    ) -> Result<(), SurfaceError> {
        let mut buffer = self.surface.buffer_mut().map_err(SurfaceError::Surface)?;
        if needs_full_present_fallback(pixels.len(), buffer.len()) {
            buffer.fill(0);
            return buffer.present().map_err(SurfaceError::Surface);
        }
        buffer.copy_from_slice(pixels);
        match damage_rect(self.width, damage) {
            Some(rect) => buffer
                .present_with_damage(&[rect])
                .map_err(SurfaceError::Surface),
            None => buffer.present().map_err(SurfaceError::Surface),
        }
    }
}

/// Decides whether [`WindowSurface::present`] must fall back to a full-buffer
/// present because `pixels` doesn't match the softbuffer buffer's current
/// length (a resize is mid-flight).
const fn needs_full_present_fallback(pixels_len: usize, buffer_len: usize) -> bool {
    pixels_len != buffer_len
}

/// Converts a damage row band `[y0, y1)` and the surface `width` into a
/// softbuffer damage [`Rect`](softbuffer::Rect) spanning the full row width,
/// or `None` if the band or width is degenerate, in which case
/// [`WindowSurface::present`] falls back to a full present instead.
const fn damage_rect(width: u32, damage: (u32, u32)) -> Option<softbuffer::Rect> {
    let (y0, y1) = damage;
    match (
        NonZeroU32::new(width),
        NonZeroU32::new(y1.saturating_sub(y0)),
    ) {
        (Some(width), Some(height)) => Some(softbuffer::Rect {
            x: 0,
            y: y0,
            width,
            height,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{NonZeroU32, damage_rect, needs_full_present_fallback};

    #[test]
    fn full_present_fallback_on_length_mismatch() {
        assert!(needs_full_present_fallback(3, 4));
        assert!(needs_full_present_fallback(0, 4));
    }

    #[test]
    fn no_fallback_when_lengths_match() {
        assert!(!needs_full_present_fallback(4, 4));
        assert!(!needs_full_present_fallback(0, 0));
    }

    #[test]
    fn damage_rect_converts_band_to_full_width_rect() {
        let rect = damage_rect(80, (4, 10)).expect("non-degenerate band");
        assert_eq!(rect.x, 0);
        assert_eq!(rect.y, 4);
        assert_eq!(rect.width, NonZeroU32::new(80).unwrap());
        assert_eq!(rect.height, NonZeroU32::new(6).unwrap());
    }

    #[test]
    fn damage_rect_none_when_width_is_zero() {
        assert!(damage_rect(0, (0, 10)).is_none());
    }

    #[test]
    fn damage_rect_none_when_band_is_empty() {
        assert!(damage_rect(80, (5, 5)).is_none());
    }

    #[test]
    fn damage_rect_none_when_band_is_inverted() {
        // y1 < y0: saturating_sub yields 0, treated the same as an empty band.
        assert!(damage_rect(80, (10, 5)).is_none());
    }
}
