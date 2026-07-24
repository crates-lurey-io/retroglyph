//! wasm32 WebGL2 context creation from the winit `<canvas>`.
//!
//! Selected by `lib.rs` on wasm32. Exposes the same
//! `GlContext::new`/`resize`/`present`/`flavor` API as the native module
//! (`context_native.rs`), so the renderer drives either without `cfg` in its body.
//!
//! Like `retroglyph-software`'s wasm surface, the canvas is located via the DOM
//! (`query_selector("canvas")`) rather than the raw window handle: winit reports
//! `RawWindowHandle::WebCanvas`, whose canvas can only be read through a forbidden `unsafe`
//! pointer cast, and winit appends exactly one canvas to the page.

#![allow(clippy::redundant_pub_crate)]

use crate::error::SurfaceError;
use crate::shaders::GlslFlavor;
use retroglyph_window::WindowHandle;
use std::sync::Arc;
use wasm_bindgen::JsCast as _;

/// A live WebGL2 context plus the `glow` handle for rendering.
pub(crate) struct GlContext {
    /// The `glow` context every render call goes through.
    pub gl: glow::Context,
    /// Kept only for [`is_context_lost`](Self::is_context_lost); all rendering goes through `gl`.
    raw: web_sys::WebGl2RenderingContext,
    canvas: web_sys::HtmlCanvasElement,
}

impl GlContext {
    /// Locates the canvas, acquires a WebGL2 context, and wraps it in a `glow` context.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError::Init`] if the canvas or WebGL2 context cannot be obtained.
    #[allow(clippy::needless_pass_by_value)]
    pub(crate) fn new(
        _window: &Arc<dyn WindowHandle>,
        width: u32,
        height: u32,
    ) -> Result<Self, SurfaceError> {
        let canvas = retroglyph_window::web::winit_canvas().map_err(SurfaceError::Init)?;

        canvas.set_width(width.max(1));
        canvas.set_height(height.max(1));

        let raw = canvas
            .get_context("webgl2")
            .map_err(|_| SurfaceError::Init("get_context(\"webgl2\") threw".to_owned()))?
            .ok_or_else(|| SurfaceError::Init("WebGL2 not available".to_owned()))?
            .dyn_into::<web_sys::WebGl2RenderingContext>()
            .map_err(|_| SurfaceError::Init("context is not WebGL2".to_owned()))?;

        let gl = glow::Context::from_webgl2_context(raw.clone());

        Ok(Self { gl, raw, canvas })
    }

    /// WebGL2 always uses the `300 es` GLSL flavor.
    pub(crate) const fn flavor(&self) -> GlslFlavor {
        GlslFlavor::Es300
    }

    /// Resizes the canvas backing store.
    pub(crate) fn resize(&self, width: u32, height: u32) {
        self.canvas.set_width(width.max(1));
        self.canvas.set_height(height.max(1));
    }

    /// No explicit buffer swap on the web: the browser composites the canvas after the draw call
    /// returns. Reports a lost context as a (recoverable) present error so the event loop can
    /// react.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError::Present`] if the WebGL2 context has been lost.
    pub(crate) fn present(&self) -> Result<(), SurfaceError> {
        if self.raw.is_context_lost() {
            return Err(SurfaceError::Present("WebGL2 context lost".to_owned()));
        }
        Ok(())
    }
}
