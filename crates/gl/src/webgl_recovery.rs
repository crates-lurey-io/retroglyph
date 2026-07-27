//! Live WebGL2 context-loss recovery test (issue #373).
//!
//! [`webgl_smoke`](crate::webgl_smoke) proves a healthy WebGL2 context renders; this proves the
//! backend recovers when that context is *lost and restored* (a backgrounded tab reclaiming GPU
//! resources, a driver reset). It drives the real windowed path ([`Presenter::init_surface`]
//! then [`Presenter::present`]), so it exercises the actual listener registration in
//! `context_wasm.rs` and the rebuild branch in `GlRenderer::present`, not a stand-in.
//!
//! # The property under test
//!
//! A WebGL2 context is only restored by the browser if the page calls `preventDefault()` on the
//! `webglcontextlost` event; the `webglcontextrestored` event then fires and every GL object must
//! be recreated. The test forces the whole cycle with the `WEBGL_lose_context` extension and
//! asserts, end to end:
//!
//! 1. a healthy frame renders (full-block cell is its foreground color);
//! 2. while lost, `present()` reports the recoverable error;
//! 3. after restore, `present()` succeeds *and* the full-block cell is its foreground color again,
//!    which can only happen if the invalidated program/atlas/buffers were rebuilt on the live
//!    context.
//!
//! Step 3 is the load-bearing one: if recovery only cleared the error flag without rebuilding, the
//! draw would use dead GL objects and the cell would not come back red.
//!
//! # Running
//!
//! Same runner as `webgl_smoke`: `just test-wasm-gl` (`wasm-pack test --headless --chrome
//! crates/gl`), with `webdriver.json`'s `--enable-unsafe-swiftshader` for a GPU-less WebGL2. The
//! `WEBGL_lose_context` extension is implemented by the browser (not the GL driver), so it works
//! under `SwiftShader`.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]
// `clippy::future_not_send` fires on every `async fn` here. These are `wasm_bindgen_test`s: they
// run on the browser's single-threaded event loop, and the futures they await (`JsFuture` over a
// `Promise`) hold `JsValue`s, which are `!Send` by construction because a JS value cannot cross
// an agent boundary. There is no `Send` bound to satisfy -- `wasm_bindgen_test` never requires
// one -- so the lint is reporting a fact that has no consequence on this target.
#![allow(clippy::future_not_send)]

use crate::GlBackendBuilder;
use retroglyph_core::DrawCell;
use retroglyph_core::backend::Output as _;
use retroglyph_core::color::Color;
use retroglyph_core::grid::Pos;
use retroglyph_core::style::Style;
use retroglyph_core::tile::Tile;
use retroglyph_window::{Presenter as _, WindowHandle, raw_window_handle};
use std::sync::Arc;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

const RED: (u8, u8, u8) = (0xFF, 0x00, 0x00);
const BLUE: (u8, u8, u8) = (0x00, 0x00, 0xFF);

/// `(r, g, b)` -> a [`Color::Rgb`].
const fn rgb(c: (u8, u8, u8)) -> Color {
    Color::Rgb {
        r: c.0,
        g: c.1,
        b: c.2,
    }
}

/// A [`WindowHandle`] that never yields a real handle.
///
/// The wasm `GlContext::new` locates its canvas through the DOM and ignores the window argument
/// entirely (see `context_wasm.rs`), so these methods are never called on wasm: the renderer only
/// needs *a* value of the right type to pass to [`Presenter::init_surface`].
struct NoWindow;

impl raw_window_handle::HasWindowHandle for NoWindow {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        unreachable!("retroglyph-gl's wasm GlContext::new never reads the window handle")
    }
}

impl raw_window_handle::HasDisplayHandle for NoWindow {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        unreachable!("retroglyph-gl's wasm GlContext::new never reads the display handle")
    }
}

/// Yields to the browser event loop for `ms` milliseconds so a queued `webglcontextlost` /
/// `webglcontextrestored` event is delivered before the test inspects the result.
async fn yield_to_browser(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        web_sys::window()
            .expect("window")
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms)
            .expect("setTimeout");
    });
    wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .expect("timeout resolves");
}

/// Appends a `<canvas>` to the document so the renderer's `winit_canvas()` lookup finds it, and
/// returns it. The caller removes it when done.
fn dom_canvas() -> web_sys::HtmlCanvasElement {
    let document = web_sys::window()
        .expect("no window")
        .document()
        .expect("no document");
    let canvas: web_sys::HtmlCanvasElement = document
        .create_element("canvas")
        .expect("create <canvas>")
        .dyn_into()
        .expect("element is a canvas");
    document
        .body()
        .expect("no body")
        .append_child(&canvas)
        .expect("append canvas");
    canvas
}

/// Reads the default framebuffer back and returns whether every pixel of the leftmost cell
/// (`0..cell_w` across the full height) is `expected`.
///
/// The grid is one full-block cell (cell 0) followed by one space cell, so the left `cell_w`
/// columns are the glyph under test. `readPixels` is bottom-left origin, but that only flips Y and
/// the column range is uniform, so the check holds regardless of orientation (same reasoning as
/// `webgl_smoke`).
fn left_cell_is(
    gl2: &web_sys::WebGl2RenderingContext,
    width: u32,
    height: u32,
    cell_w: u32,
    expected: (u8, u8, u8),
) -> bool {
    gl2.bind_framebuffer(web_sys::WebGl2RenderingContext::FRAMEBUFFER, None);
    let mut buf = vec![0u8; (width * height * 4) as usize];
    gl2.read_pixels_with_opt_u8_array(
        0,
        0,
        width as i32,
        height as i32,
        web_sys::WebGl2RenderingContext::RGBA,
        web_sys::WebGl2RenderingContext::UNSIGNED_BYTE,
        Some(&mut buf),
    )
    .expect("readPixels");

    for y in 0..height {
        for x in 0..cell_w {
            let i = ((y * width + x) * 4) as usize;
            if (buf[i], buf[i + 1], buf[i + 2]) != expected {
                return false;
            }
        }
    }
    true
}

#[wasm_bindgen_test]
async fn context_loss_is_recovered_by_rebuilding_on_restore() {
    let canvas = dom_canvas();

    // One full-block cell (fg red over bg blue -> all red) and one space cell, matching
    // `webgl_smoke`. The full-block cell is the recovery probe: after a real loss it can only be
    // red again if the atlas/program/buffers were rebuilt.
    let mut renderer = GlBackendBuilder::new()
        .grid_size(2, 1)
        .build()
        .expect("default-font builds a renderer");
    let (width, height) = renderer.surface_size;
    let (cell_w, _cell_h) = renderer.geometry.cell_size();

    let handle: Arc<dyn WindowHandle> = Arc::new(NoWindow);
    renderer
        .init_surface(handle)
        .expect("init_surface acquires the WebGL2 context");

    let full_block = Tile::new('\u{2588}', Style::new().fg(rgb(RED)).bg(rgb(BLUE)));
    renderer
        .draw(core::iter::once(DrawCell::new(Pos::new(0, 0), &full_block)))
        .expect("draw is infallible");

    // The renderer got the context from the same canvas, so `getContext` hands back that very
    // context here, used to drive the loss extension and read pixels back.
    let gl2 = canvas
        .get_context("webgl2")
        .expect("get_context threw")
        .expect("WebGL2 available")
        .dyn_into::<web_sys::WebGl2RenderingContext>()
        .expect("is WebGL2");

    // 1. Healthy frame renders red.
    renderer.present().expect("first present succeeds");
    assert!(
        left_cell_is(&gl2, width, height, cell_w, RED),
        "baseline full-block cell should be its foreground color"
    );

    // `unchecked_into`, not `dyn_into`: a WebGL extension object's constructor isn't a named
    // global, so `instanceof` (what `dyn_into` uses) fails even though the object is the right
    // shape. getExtension already guarantees the type.
    let lose = gl2
        .get_extension("WEBGL_lose_context")
        .expect("get_extension threw")
        .expect("WEBGL_lose_context present")
        .unchecked_into::<web_sys::WebglLoseContext>();

    // 2. Force a loss and let the `webglcontextlost` event fire.
    lose.lose_context();
    yield_to_browser(50).await;
    assert!(gl2.is_context_lost(), "context should be lost");
    assert!(
        renderer.present().is_err(),
        "present() should report the recoverable error while the context is lost"
    );

    // 3. Restore; the `webglcontextrestored` event (only fired because `on_lost` called
    // preventDefault) flags a rebuild, which the next present() performs.
    lose.restore_context();
    yield_to_browser(50).await;
    assert!(!gl2.is_context_lost(), "context should be restored");

    renderer
        .present()
        .expect("present() succeeds after the context is restored");
    assert!(
        left_cell_is(&gl2, width, height, cell_w, RED),
        "after restore the full-block cell should be red again (resources were rebuilt)"
    );

    canvas.remove();
}
