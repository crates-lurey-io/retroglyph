#![cfg(target_arch = "wasm32")]
//! Locates the single `<canvas>` element winit appends to the page.
//!
//! winit reports `RawWindowHandle::WebCanvas`, whose canvas can only be read through a
//! forbidden `unsafe` pointer cast, and winit appends exactly one canvas to the page. Both
//! graphical backends (`retroglyph-gl`, `retroglyph-software`) need this same DOM lookup, so it
//! lives here once instead of being duplicated in each.

use wasm_bindgen::JsCast as _;

/// Finds winit's `<canvas>` element via the DOM.
///
/// # Errors
///
/// Returns a message describing the failure if the global `Window`, `Document`, or canvas
/// element cannot be obtained. Each caller wraps the message in its own surface-error type.
pub fn winit_canvas() -> Result<web_sys::HtmlCanvasElement, String> {
    let document = web_sys::window()
        .ok_or_else(|| "no global `Window`".to_owned())?
        .document()
        .ok_or_else(|| "no `Document`".to_owned())?;
    let canvas: web_sys::HtmlCanvasElement = document
        .query_selector("canvas")
        .map_err(|_| "query_selector() threw".to_owned())?
        .ok_or_else(|| "no canvas element found".to_owned())?
        .dyn_into()
        .map_err(|_| "queried element is not a canvas".to_owned())?;
    Ok(canvas)
}
