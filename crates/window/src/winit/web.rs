//! wasm32-only DPR-capping, browser-viewport-fill, and pointer-rescaling helpers used by
//! `super::run` on `wasm32`.

#[cfg(target_arch = "wasm32")]
use std::sync::Arc;
#[cfg(target_arch = "wasm32")]
use winit::window::Window;

/// Upper bound on the device pixel ratio used to size the canvas backing
/// store. Present cost is O(pixels), so an uncapped DPR (3 on many phones,
/// 2 on most laptops) quadruples or worse the per-frame rasterize/present
/// work for marginal crispness on a pseudo-graphic UI.
#[cfg(target_arch = "wasm32")]
const MAX_DEVICE_PIXEL_RATIO: f64 = 1.5;

/// The browser viewport's CSS width/height, or `None` if running outside a
/// browser `window` context. Shared by the two physical-size helpers below.
#[cfg(target_arch = "wasm32")]
fn web_viewport_css_size() -> Option<(f64, f64)> {
    let window = web_sys::window()?;
    let width = window.inner_width().ok()?.as_f64()?;
    let height = window.inner_height().ok()?.as_f64()?;
    Some((width, height))
}

/// The browser viewport size in true physical (device) pixels -- i.e. at the
/// real, uncapped `devicePixelRatio`.
///
/// Pass this to winit's `with_inner_size`/`request_inner_size` (and *only*
/// this -- never [`web_viewport_surface_physical_size`]). winit's wasm
/// backend always converts the `PhysicalSize` it's given back to a logical
/// (CSS pixel) size by dividing by the real `devicePixelRatio` to set the
/// canvas's inline style; handing it anything scaled by a different ratio
/// (like our DPR-capped surface size) makes the canvas's CSS size come out
/// smaller than the viewport.
#[cfg(target_arch = "wasm32")]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(super) fn web_viewport_layout_physical_size() -> Option<winit::dpi::PhysicalSize<u32>> {
    let (width, height) = web_viewport_css_size()?;
    let dpr = web_sys::window()?.device_pixel_ratio();
    Some(winit::dpi::PhysicalSize::new(
        (width * dpr).round() as u32,
        (height * dpr).round() as u32,
    ))
}

/// Ratio to convert a pointer position winit reports (always in *real*,
/// uncapped-DPR physical pixels -- see `to_physical(super::scale_factor)` in
/// `winit`'s wasm `pointer.rs`) into the raster-backing-store pixel space
/// that [`Presenter::cell_size`](crate::presenter::Presenter::cell_size),
/// and therefore [`pixel_to_cell`], are expressed in.
///
/// `1.0` whenever `real_dpr` is already at or below `capped_dpr` (desktop,
/// non-Retina): no correction needed. Below that, taps/clicks land scaled
/// past their true position -- south-east of the intended cell, growing
/// with how far `real_dpr` exceeds the cap (2x at DPR 3 against a 1.5 cap).
/// Pure math, kept separate from [`wasm_pointer_scale`] so it's unit
/// -testable without a wasm window (hence `cfg(any(.., test))`: unused on a
/// native non-test build, whose `on_cursor_moved` hardcodes `scale = 1.0`
/// instead of calling this).
#[cfg(any(target_arch = "wasm32", test))]
fn dpr_pointer_scale(real_dpr: f64, capped_dpr: f64) -> f64 {
    (capped_dpr / real_dpr).min(1.0)
}

/// [`dpr_pointer_scale`] using the page's actual `devicePixelRatio` and
/// [`MAX_DEVICE_PIXEL_RATIO`]. `1.0` if no browser `window` is available.
#[cfg(target_arch = "wasm32")]
pub(super) fn wasm_pointer_scale() -> f64 {
    web_sys::window().map_or(1.0, |w| {
        dpr_pointer_scale(w.device_pixel_ratio(), MAX_DEVICE_PIXEL_RATIO)
    })
}

/// The physical pixel size of the software renderer's raster backing store,
/// capped at [`MAX_DEVICE_PIXEL_RATIO`] for `present()` cost.
///
/// Deliberately *not* the size passed to winit (see
/// [`web_viewport_layout_physical_size`]): winit's `Resized` event always
/// reports back whatever physical size we last requested, so if this capped
/// size were also used for `request_inner_size`, the canvas's CSS size would
/// shrink below the viewport on any device whose real DPR exceeds the cap
/// (i.e. almost every phone).
#[cfg(target_arch = "wasm32")]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub(super) fn web_viewport_surface_physical_size() -> Option<winit::dpi::PhysicalSize<u32>> {
    let (width, height) = web_viewport_css_size()?;
    let dpr = web_sys::window()?
        .device_pixel_ratio()
        .min(MAX_DEVICE_PIXEL_RATIO);
    Some(winit::dpi::PhysicalSize::new(
        (width * dpr).round() as u32,
        (height * dpr).round() as u32,
    ))
}

/// Re-requests the window's inner size to match the browser viewport on
/// every `resize` event, so the canvas keeps filling the screen instead of
/// staying pinned to its size at first paint.
#[cfg(target_arch = "wasm32")]
pub(super) fn install_viewport_resize_listener(window: &Arc<Window>) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::Closure;

    let Some(web_window) = web_sys::window() else {
        return;
    };
    let window = window.clone();
    let closure = Closure::<dyn FnMut()>::new(move || {
        // Only the uncapped layout size goes to winit; `on_resized` (fired
        // by the `Resized` event this triggers) independently recomputes
        // the DPR-capped surface size for the backing store.
        if let Some(size) = web_viewport_layout_physical_size() {
            let _ = window.request_inner_size(size);
        }
    });
    if web_window
        .add_event_listener_with_callback("resize", closure.as_ref().unchecked_ref())
        .is_ok()
    {
        // Leaked deliberately: the listener, and the closure it wraps, need
        // to live as long as the page does -- there's no window-teardown
        // hook on wasm to drop it from.
        closure.forget();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── dpr_pointer_scale ─────────────────────────────────────────────────────

    #[test]
    fn dpr_pointer_scale_no_correction_below_cap() {
        // Real DPR at or below the cap: pointer positions already match the
        // (uncapped) backing store, no rescale needed.
        assert!((dpr_pointer_scale(1.0, 1.5) - 1.0).abs() < 1e-9);
        assert!((dpr_pointer_scale(1.5, 1.5) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn dpr_pointer_scale_corrects_above_cap() {
        // Real DPR 3 against a 1.5 cap: the backing store is half the real
        // resolution, so pointer positions must be halved to land on the
        // right cell instead of drifting south-east of it.
        assert!((dpr_pointer_scale(3.0, 1.5) - 0.5).abs() < 1e-9);
        assert!((dpr_pointer_scale(2.0, 1.5) - 0.75).abs() < 1e-9);
    }
}
