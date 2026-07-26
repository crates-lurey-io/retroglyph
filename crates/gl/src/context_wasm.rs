//! wasm32 WebGL2 context creation from the winit `<canvas>`.
//!
//! Selected by `lib.rs` on wasm32. Exposes the same
//! `GlContext::new`/`resize`/`present`/`flavor`/`take_needs_rebuild` API as the native module
//! (`context_native.rs`), so the renderer drives either without `cfg` in its body. This module adds
//! the real work behind `take_needs_rebuild`: WebGL2 context-loss/restore recovery (issue #373),
//! which has no native equivalent (a lost desktop GL context is fatal).
//!
//! Like `retroglyph-software`'s wasm surface, the canvas is located via the DOM
//! (`query_selector("canvas")`) rather than the raw window handle: winit reports
//! `RawWindowHandle::WebCanvas`, whose canvas can only be read through a forbidden `unsafe`
//! pointer cast, and winit appends exactly one canvas to the page.

#![allow(clippy::redundant_pub_crate)]

use crate::error::SurfaceError;
use crate::shaders::GlslFlavor;
use retroglyph_window::WindowHandle;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use wasm_bindgen::JsCast as _;
use wasm_bindgen::closure::Closure;

/// A live WebGL2 context plus the `glow` handle for rendering.
pub(crate) struct GlContext {
    /// The `glow` context every render call goes through.
    pub gl: glow::Context,
    /// Kept only for [`is_context_lost`](Self::is_context_lost); all rendering goes through `gl`.
    raw: web_sys::WebGl2RenderingContext,
    canvas: web_sys::HtmlCanvasElement,
    /// WebGL2 context-loss/restore handlers (issue #373). Keeps the JS closures alive for the
    /// context's lifetime and exposes the one-shot "a restore happened, rebuild GL objects" signal
    /// the renderer polls each frame.
    loss: ContextLoss,
}

/// Registers the `webglcontextlost`/`webglcontextrestored` listeners on the canvas and tracks
/// whether a restore has happened since the renderer last rebuilt its GL objects.
///
/// A WebGL2 context can be lost at any time (GPU reset, a backgrounded tab reclaiming resources)
/// and every GL object (program, buffers, atlas texture) is invalidated when it is. The
/// browser only fires `webglcontextrestored` (and hands back a usable context) if the page called
/// `event.preventDefault()` on the matching `webglcontextlost`; without that the context stays lost
/// forever. So `on_lost` exists purely to call `preventDefault`, and `on_restored` flips
/// `needs_rebuild`, which [`GlContext::take_needs_rebuild`] hands to the renderer so it can
/// recreate its resources on the now-live context (see `GlRenderer::present`).
struct ContextLoss {
    /// The canvas the listeners are attached to, kept so [`Drop`] can detach them.
    canvas: web_sys::HtmlCanvasElement,
    /// Set by `on_restored`, consumed (and cleared) by [`take_needs_rebuild`](Self::take_needs_rebuild).
    needs_rebuild: Rc<Cell<bool>>,
    /// `webglcontextlost` handler: calls `preventDefault` so the browser will restore the context.
    on_lost: Closure<dyn FnMut(web_sys::Event)>,
    /// `webglcontextrestored` handler: flags that GL objects must be rebuilt.
    on_restored: Closure<dyn FnMut(web_sys::Event)>,
}

impl ContextLoss {
    /// Attaches both listeners to `canvas`.
    pub(crate) fn register(canvas: &web_sys::HtmlCanvasElement) -> Self {
        let needs_rebuild = Rc::new(Cell::new(false));

        // The default action of `webglcontextlost` is to leave the context permanently lost;
        // `preventDefault` is what tells the browser to attempt a restore.
        let on_lost = Closure::<dyn FnMut(web_sys::Event)>::new(|event: web_sys::Event| {
            event.prevent_default();
        });

        let flag = Rc::clone(&needs_rebuild);
        let on_restored =
            Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
                flag.set(true);
            });

        let _ = canvas
            .add_event_listener_with_callback("webglcontextlost", on_lost.as_ref().unchecked_ref());
        let _ = canvas.add_event_listener_with_callback(
            "webglcontextrestored",
            on_restored.as_ref().unchecked_ref(),
        );

        Self {
            canvas: canvas.clone(),
            needs_rebuild,
            on_lost,
            on_restored,
        }
    }

    /// Returns whether a restore has happened since the last call, clearing the flag (one-shot).
    fn take_needs_rebuild(&self) -> bool {
        self.needs_rebuild.replace(false)
    }
}

impl Drop for ContextLoss {
    fn drop(&mut self) {
        let _ = self.canvas.remove_event_listener_with_callback(
            "webglcontextlost",
            self.on_lost.as_ref().unchecked_ref(),
        );
        let _ = self.canvas.remove_event_listener_with_callback(
            "webglcontextrestored",
            self.on_restored.as_ref().unchecked_ref(),
        );
    }
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
        let loss = ContextLoss::register(&canvas);

        Ok(Self {
            gl,
            raw,
            canvas,
            loss,
        })
    }

    /// Whether the WebGL2 context was lost and has since been restored, meaning every GL object
    /// (program, buffers, atlas texture) was invalidated and must be rebuilt on the now-live
    /// context. One-shot: returns `true` at most once per restore, so the renderer rebuilds exactly
    /// once. The native `GlContext` mirrors this method (always `false`) so the renderer polls it
    /// without a `cfg`.
    pub(crate) fn take_needs_rebuild(&self) -> bool {
        self.loss.take_needs_rebuild()
    }

    /// WebGL2 always uses the `300 es` GLSL flavor.
    //
    // `&self` is unused (the answer is constant on the web) but is kept to mirror the native
    // `GlContext::flavor(&self)`, which does branch on the created context's API: the renderer
    // calls `ctx.flavor()` generically across both, so the signatures must match.
    #[allow(clippy::unused_self)]
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
