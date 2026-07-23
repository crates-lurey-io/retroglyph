//! Native GL context creation via `glutin`, from the window's raw handles.
//!
//! Selected by `lib.rs` on every non-wasm target. Exposes the same
//! `GlContext::new`/`resize`/`present`/`flavor` API as the wasm module
//! (`context_wasm.rs`), so the renderer drives either without `cfg` in its body -- the same
//! module-swap pattern `retroglyph-software` uses for its window surface.
//!
//! The GL context is created from the already-created window's raw window/display handles (glutin
//! supports EGL/GLX/WGL/CGL from a `raw-window-handle`), so this needs no changes to
//! `retroglyph-window`: it slots straight into
//! [`Presenter::init_surface`](retroglyph_window::Presenter::init_surface)'s
//! `Arc<dyn WindowHandle>` contract.

#![allow(clippy::redundant_pub_crate)]

use crate::error::SurfaceError;
use crate::shaders::GlslFlavor;
use glutin::config::{ConfigTemplateBuilder, GlConfig as _};
// `GlContext as _` brings the `context_api()` method into scope without colliding with this
// module's own `GlContext` struct name.
use glutin::context::{
    ContextApi, ContextAttributesBuilder, GlContext as _, PossiblyCurrentContext, Version,
};
use glutin::display::Display;
use glutin::prelude::*;
use glutin::surface::{Surface, SurfaceAttributesBuilder, SwapInterval, WindowSurface};
use raw_window_handle::RawWindowHandle;
use retroglyph_window::WindowHandle;
use std::ffi::CString;
use std::num::NonZeroU32;
use std::sync::Arc;

/// A live native GL context plus its window surface and the `glow` handle for rendering.
pub(crate) struct GlContext {
    /// The `glow` context every render call goes through.
    pub gl: glow::Context,
    surface: Surface<WindowSurface>,
    context: PossiblyCurrentContext,
    flavor: GlslFlavor,
    // Keep the display alive for as long as the surface/context reference it.
    _display: Display,
}

impl GlContext {
    /// Creates a GL 3.3 core (or GLES 3.0 fallback) context for `window` at `width`x`height`
    /// physical pixels.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError::Init`] if any glutin step (display, config, context, surface, or
    /// make-current) fails.
    pub(crate) fn new(
        window: &Arc<dyn WindowHandle>,
        width: u32,
        height: u32,
    ) -> Result<Self, SurfaceError> {
        let raw_display = window
            .display_handle()
            .map_err(|e| SurfaceError::Init(format!("display handle: {e}")))?
            .as_raw();
        let raw_window = window
            .window_handle()
            .map_err(|e| SurfaceError::Init(format!("window handle: {e}")))?
            .as_raw();

        // SAFETY: `raw_display` comes from a live winit window owned by `retroglyph-window` and
        // stays valid for this display's lifetime.
        let display = unsafe { Display::new(raw_display, api_preference(raw_window)) }
            .map_err(|e| SurfaceError::Init(format!("create GL display: {e}")))?;

        let template = ConfigTemplateBuilder::new()
            .compatible_with_native_window(raw_window)
            .with_alpha_size(8)
            .build();
        // SAFETY: the template was built against this display's native window handle.
        let config = unsafe { display.find_configs(template) }
            .map_err(|e| SurfaceError::Init(format!("find GL configs: {e}")))?
            // Fewest samples: crisp, unantialiased glyph edges (text, not 3D).
            .reduce(|acc, cfg| {
                if cfg.num_samples() < acc.num_samples() {
                    cfg
                } else {
                    acc
                }
            })
            .ok_or_else(|| SurfaceError::Init("no suitable GL config".to_owned()))?;

        // Prefer desktop GL 3.3 core; fall back to GLES 3.0 (WebGL2 feature level, and our shaders
        // support the `300 es` flavor).
        let core_attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 3))))
            .build(Some(raw_window));
        let gles_attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::Gles(Some(Version::new(3, 0))))
            .build(Some(raw_window));
        // SAFETY: `config` and the raw window handle come from this display.
        let not_current = unsafe {
            display
                .create_context(&config, &core_attrs)
                .or_else(|_| display.create_context(&config, &gles_attrs))
        }
        .map_err(|e| SurfaceError::Init(format!("create GL context: {e}")))?;

        let (w, h) = nonzero_size(width, height);
        let surf_attrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(raw_window, w, h);
        // SAFETY: `config` and the raw window handle come from this display.
        let surface = unsafe { display.create_window_surface(&config, &surf_attrs) }
            .map_err(|e| SurfaceError::Init(format!("create window surface: {e}")))?;

        let context = not_current
            .make_current(&surface)
            .map_err(|e| SurfaceError::Init(format!("make GL context current: {e}")))?;

        // Best-effort vsync; a failure here is non-fatal.
        let _ = surface.set_swap_interval(
            &context,
            SwapInterval::Wait(NonZeroU32::new(1).expect("1 is non-zero")),
        );

        let flavor = match context.context_api() {
            ContextApi::Gles(_) => GlslFlavor::Es300,
            ContextApi::OpenGl(_) => GlslFlavor::Desktop330,
        };

        // SAFETY: the loader is only called synchronously here, while `display` is still borrowed
        // and current.
        let gl = unsafe {
            glow::Context::from_loader_function(|symbol| {
                CString::new(symbol).map_or(core::ptr::null(), |cname| {
                    display.get_proc_address(cname.as_c_str()).cast()
                })
            })
        };

        Ok(Self {
            gl,
            surface,
            context,
            flavor,
            _display: display,
        })
    }

    /// The GLSL flavor the created context needs (`330 core` for desktop GL, `300 es` for GLES).
    pub(crate) const fn flavor(&self) -> GlslFlavor {
        self.flavor
    }

    /// Resizes the GL surface (required on EGL/Wayland/macOS; a no-op elsewhere).
    pub(crate) fn resize(&self, width: u32, height: u32) {
        let (w, h) = nonzero_size(width, height);
        self.surface.resize(&self.context, w, h);
    }

    /// Swaps the back buffer to the window.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError::Present`] if the buffer swap fails.
    pub(crate) fn present(&self) -> Result<(), SurfaceError> {
        self.surface
            .swap_buffers(&self.context)
            .map_err(|e| SurfaceError::Present(format!("swap buffers: {e}")))
    }
}

/// Picks the platform-appropriate glutin display API. EGL covers Linux (X11 + Wayland) and is the
/// simplest portable choice; Windows prefers WGL with an EGL fallback; macOS uses CGL.
#[cfg(target_os = "windows")]
const fn api_preference(raw_window: RawWindowHandle) -> glutin::display::DisplayApiPreference {
    glutin::display::DisplayApiPreference::WglThenEgl(Some(raw_window))
}

/// See the Windows variant.
#[cfg(target_os = "macos")]
const fn api_preference(_raw_window: RawWindowHandle) -> glutin::display::DisplayApiPreference {
    glutin::display::DisplayApiPreference::Cgl
}

/// See the Windows variant.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const fn api_preference(_raw_window: RawWindowHandle) -> glutin::display::DisplayApiPreference {
    glutin::display::DisplayApiPreference::Egl
}

/// Clamps a surface size to non-zero (glutin surfaces cannot be 0-sized; a minimized window can
/// report 0).
const fn nonzero_size(width: u32, height: u32) -> (NonZeroU32, NonZeroU32) {
    const fn at_least_one(v: u32) -> NonZeroU32 {
        match NonZeroU32::new(v) {
            Some(n) => n,
            None => NonZeroU32::MIN,
        }
    }
    (at_least_one(width), at_least_one(height))
}
