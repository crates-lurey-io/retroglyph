//! wasm32 `Canvas2D` window surface.
//!
//! Selected by `lib.rs` on wasm32. Exposes the same
//! `WindowSurface::new`/`resize`/`present` API and its own `SurfaceError` as
//! the native `surface_native.rs`, so the renderer drives either without
//! `cfg` in its body.
//!
//! Bypasses softbuffer's web backend, whose `present()` reallocates a fresh
//! RGBA `Vec` every frame (profiled as the dominant per-frame cost). This
//! surface owns a persistent RGBA byte buffer that is reused across frames and
//! only reallocated on resize.

// See surface_native.rs: `pub(crate)` is correct for the crate-internal
// `WindowSurface`; the nursery `redundant_pub_crate` lint conflicts with
// `unreachable_pub` for the module-per-platform pattern.
#![allow(clippy::redundant_pub_crate)]

use retroglyph_window::WindowHandle;
use std::sync::Arc;
use wasm_bindgen::JsCast as _;

/// Canvas + cached 2D context + persistent RGBA byte buffer.
pub(crate) struct WindowSurface {
    ctx: web_sys::CanvasRenderingContext2d,
    canvas: web_sys::HtmlCanvasElement,
    /// Persistent RGBA8 buffer, length `width * height * 4`. Reused across
    /// frames; only [`resize`](Self::resize) reallocates it.
    rgba: Vec<u8>,
    width: u32,
    height: u32,
}

/// Error locating or using the backing `<canvas>` element.
#[derive(Debug)]
pub enum SurfaceError {
    /// Failed to locate or use the backing `<canvas>` element.
    Canvas(String),
}

impl core::fmt::Display for SurfaceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Canvas(msg) => write!(f, "canvas surface: {msg}"),
        }
    }
}

impl std::error::Error for SurfaceError {}

impl WindowSurface {
    /// Locates winit's `<canvas>` and caches it plus its 2D context.
    ///
    /// This winit version reports `RawWindowHandle::WebCanvas`, whose canvas
    /// object can only be read through an `unsafe` pointer cast (forbidden
    /// here). winit appends exactly one canvas to the page, so grab it
    /// directly instead of going through the raw handle.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError::Canvas`] if the canvas or its 2D context cannot
    /// be located.
    // `window` is unused: the canvas is found via the DOM, not the raw handle
    // (see above). The parameter stays to match the native `new` and the
    // `Presenter::init_surface` signature it feeds.
    #[allow(clippy::needless_pass_by_value)]
    pub(crate) fn new(_window: Arc<dyn WindowHandle>) -> Result<Self, SurfaceError> {
        let document = web_sys::window()
            .ok_or_else(|| SurfaceError::Canvas("no global `Window`".to_owned()))?
            .document()
            .ok_or_else(|| SurfaceError::Canvas("no `Document`".to_owned()))?;

        let canvas: web_sys::HtmlCanvasElement = document
            .query_selector("canvas")
            .map_err(|_| SurfaceError::Canvas("query_selector() threw".to_owned()))?
            .ok_or_else(|| SurfaceError::Canvas("no canvas element found".to_owned()))?
            .dyn_into()
            .map_err(|_| SurfaceError::Canvas("queried element is not a canvas".to_owned()))?;

        let ctx: web_sys::CanvasRenderingContext2d = canvas
            .get_context("2d")
            .map_err(|_| SurfaceError::Canvas("getContext(\"2d\") threw".to_owned()))?
            .ok_or_else(|| SurfaceError::Canvas("2d context unavailable".to_owned()))?
            .dyn_into()
            .map_err(|_| {
                SurfaceError::Canvas("getContext(\"2d\") returned unexpected type".to_owned())
            })?;

        Ok(Self {
            ctx,
            canvas,
            rgba: Vec::new(),
            width: 0,
            height: 0,
        })
    }

    /// Sets the canvas backing size (which clears its bitmap, matching
    /// softbuffer's own resize behavior) and resizes the persistent RGBA
    /// buffer. Reallocating here, not in [`present`](Self::present), keeps
    /// steady-state frames allocation-free.
    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        self.canvas.set_width(width);
        self.canvas.set_height(height);
        self.width = width;
        self.height = height;
        let len = width as usize * height as usize * 4;
        self.rgba.clear();
        self.rgba.resize(len, 0);
    }

    /// Converts only the changed row band `[y0, y1)` of `pixels`
    /// (`0x00RRGGBB`) into the persistent `rgba` buffer in place (no per-frame
    /// allocation), then blits just that band via `put_image_data` at row `y0`.
    /// Rows outside the band are unchanged since the last present, so they stay
    /// correct on the canvas without being re-uploaded.
    ///
    /// The band `ImageData` is rebuilt each frame because a raw
    /// `js_sys::Uint8ClampedArray::view` into WASM linear memory would detach
    /// across any intervening memory growth (and its constructor is `unsafe`,
    /// which this crate forbids). The constructor copies the band into a
    /// JS-owned buffer, but `rgba` itself is allocated once and reused, so the
    /// `flat_map`/`Vec::collect` allocation softbuffer's web backend performs
    /// on every present is gone.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError::Canvas`] if `ImageData` construction or
    /// `put_image_data` fails.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    pub(crate) fn present(
        &mut self,
        pixels: &[u32],
        damage: (u32, u32),
    ) -> Result<(), SurfaceError> {
        if self.width == 0 || self.height == 0 {
            return Ok(()); // not yet sized
        }
        let expected_len = self.width as usize * self.height as usize * 4;
        if self.rgba.len() != expected_len || pixels.len() * 4 != expected_len {
            // Surface resize is still pending; skip rather than present at
            // mismatched dimensions.
            return Ok(());
        }

        let w = self.width as usize;
        let y0 = damage.0 as usize;
        let y1 = (damage.1 as usize).min(self.height as usize);
        if y1 <= y0 {
            return Ok(());
        }

        // Convert only the damaged rows into `rgba` in place.
        let (start, end) = (y0 * w, y1 * w);
        for (px, chunk) in pixels[start..end]
            .iter()
            .zip(self.rgba[start * 4..end * 4].chunks_exact_mut(4))
        {
            // Truncation is intended: `pixel_buf` packs 0x00RRGGBB, so only the
            // low byte of each shifted channel is meaningful.
            chunk[0] = (px >> 16) as u8;
            chunk[1] = (px >> 8) as u8;
            chunk[2] = *px as u8;
            chunk[3] = 255;
        }

        // Build an ImageData covering just the band and blit it at row y0, so
        // the browser only uploads and paints the changed rows.
        let band = &self.rgba[start * 4..end * 4];
        let band_h = (y1 - y0) as u32;
        let image_data = web_sys::ImageData::new_with_u8_clamped_array_and_sh(
            wasm_bindgen::Clamped(band),
            self.width,
            band_h,
        )
        .map_err(|_| SurfaceError::Canvas("ImageData construction failed".to_owned()))?;

        self.ctx
            .put_image_data(&image_data, 0.0, y0 as f64)
            .map_err(|_| SurfaceError::Canvas("put_image_data() threw".to_owned()))
    }
}
