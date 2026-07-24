//! Live WebGL2 smoke test for the GL pipeline (issue #370).
//!
//! The `compile-wasm-gl` CI job only *build-checks* the wasm32/WebGL2 path; nothing actually ran a
//! WebGL2 frame, so a backend that compiled but rendered nothing (for example the glow 0.16
//! `texImage3D` `depth`/`border` argument swap that made every glyph-atlas upload fail with
//! `INVALID_VALUE`, leaving all coverage zero and the screen blank) passed CI green. This module
//! closes that gap: it runs the *real* upload/draw pipeline against a browser WebGL2 context and
//! reads the pixels back, so an atlas that fails to upload is a hard test failure.
//!
//! It is the WebGL2 sibling of [`headless`](crate::headless) (the native EGL-surfaceless render
//! test). Both build the resources with the same [`GlRenderer::build_resources`] the windowed
//! `init_surface` uses, render into an offscreen framebuffer, and assert on the readback -- so a
//! break in shader compile/link, atlas upload, or the instanced draw shows up in exactly one of
//! the two on whichever platform regressed.
//!
//! # Running
//!
//! Runs in a real headless browser via `wasm-bindgen-test` (`run_in_browser`), driven by
//! `just test-wasm-gl` (`wasm-pack test --headless --chrome crates/gl`). CI runners have no GPU, so
//! `webdriver.json` launches Chrome with `--enable-unsafe-swiftshader` to get a software WebGL2
//! implementation. Gated to `default-font` so the test can build a renderer from the embedded
//! atlas, exactly like the native headless tests.
//!
//! # What is asserted
//!
//! The same driver-robust property checks the native headless module uses (never exact-pixel
//! snapshots, which are fragile across GL stacks): a full-block cell is entirely its foreground
//! color, and a blank (space) cell is entirely its background color. The full-block assertion is
//! the load-bearing one for the atlas-upload bug -- with a failed atlas upload the glyph coverage
//! is uniformly zero, so the full-block cell renders as its *background*, and this test fails.

// GL wants `i32` dimensions from `u32` pixel sizes; these casts are all bounded (the test grid is
// tiny) and pervasive, exactly as in `renderer.rs` and `headless.rs`. Allow the family
// module-wide rather than annotating every call.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]

use crate::GlBackendBuilder;
use crate::GlRenderer;
use crate::shaders::GlslFlavor;
use glow::HasContext as _;
use retroglyph_core::backend::Output;
use retroglyph_core::color::Color;
use retroglyph_core::grid::Pos;
use retroglyph_core::style::Style;
use retroglyph_core::tile::Tile;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

const RED: (u8, u8, u8) = (0xFF, 0x00, 0x00);
const GREEN: (u8, u8, u8) = (0x00, 0xFF, 0x00);
const BLUE: (u8, u8, u8) = (0x00, 0x00, 0xFF);

/// `(r, g, b)` -> a [`Color::Rgb`].
const fn rgb(c: (u8, u8, u8)) -> Color {
    Color::Rgb {
        r: c.0,
        g: c.1,
        b: c.2,
    }
}

/// Creates a detached `<canvas>` and acquires a WebGL2 context from it, wrapped in a `glow`
/// context.
///
/// The canvas is never added to the DOM: the test renders into an offscreen framebuffer and reads
/// it back, so no on-page compositing is involved. Returns the `glow` context (all rendering goes
/// through it).
fn webgl2_context(width: u32, height: u32) -> glow::Context {
    let document = web_sys::window()
        .expect("no window")
        .document()
        .expect("no document");
    let canvas: web_sys::HtmlCanvasElement = document
        .create_element("canvas")
        .expect("create <canvas>")
        .dyn_into()
        .expect("element is a canvas");
    canvas.set_width(width.max(1));
    canvas.set_height(height.max(1));

    let gl2 = canvas
        .get_context("webgl2")
        .expect("get_context(\"webgl2\") threw")
        .expect("WebGL2 not available (is SwiftShader enabled?)")
        .dyn_into::<web_sys::WebGl2RenderingContext>()
        .expect("context is WebGL2");
    glow::Context::from_webgl2_context(gl2)
}

/// A rendered frame read back from an offscreen framebuffer, stored bottom-left origin (WebGL2's
/// `readPixels` convention). The property assertions here are per whole cell, so orientation does
/// not matter; the flip that the native `headless` module does is unnecessary.
struct Frame {
    width: u32,
    rgba: Vec<u8>,
}

impl Frame {
    /// The `(r, g, b)` at `(x, y)`.
    fn rgb(&self, x: u32, y: u32) -> (u8, u8, u8) {
        let i = ((y * self.width + x) * 4) as usize;
        (self.rgba[i], self.rgba[i + 1], self.rgba[i + 2])
    }
}

/// Builds the renderer's resources, renders its current instance array into an RGBA8 offscreen
/// framebuffer via the real [`GlRenderer::build_resources`] + `draw`, and reads the pixels back.
fn render_to_frame(gl: &glow::Context, renderer: &GlRenderer) -> Frame {
    let (w, h) = renderer.surface_size;
    let res = renderer
        .build_resources(gl, GlslFlavor::Es300)
        .expect("build GL resources");

    // SAFETY: single-threaded wasm test, the WebGL2 context is always current; every object is
    // created, used, and deleted within this call.
    let rgba = unsafe {
        let renderbuffer = gl.create_renderbuffer().expect("create renderbuffer");
        gl.bind_renderbuffer(glow::RENDERBUFFER, Some(renderbuffer));
        gl.renderbuffer_storage(glow::RENDERBUFFER, glow::RGBA8, w as i32, h as i32);

        let framebuffer = gl.create_framebuffer().expect("create framebuffer");
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
        gl.framebuffer_renderbuffer(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::RENDERBUFFER,
            Some(renderbuffer),
        );
        let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
        assert_eq!(
            status,
            glow::FRAMEBUFFER_COMPLETE,
            "framebuffer incomplete: {status:#06x}"
        );

        // The renderer's own draw sets the viewport (via `build_resources`' `set_projection`),
        // clears, and issues the two instanced passes into the bound framebuffer.
        res.draw(gl, renderer.cell_count() as i32);
        gl.finish();

        let mut buf = vec![0u8; (w * h * 4) as usize];
        gl.read_pixels(
            0,
            0,
            w as i32,
            h as i32,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelPackData::Slice(Some(&mut buf)),
        );

        res.delete(gl);
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        gl.delete_framebuffer(framebuffer);
        gl.delete_renderbuffer(renderbuffer);
        buf
    };

    Frame { width: w, rgba }
}

/// Draws `cells` (single layer) into the renderer. Both backends' `draw` is infallible.
fn paint(out: &mut GlRenderer, cells: &[(Pos, Tile)]) {
    out.draw(cells.iter().map(|(p, t)| (*p, t, None))).ok();
}

#[wasm_bindgen_test]
fn full_block_cell_is_all_foreground_blank_cell_is_all_background() {
    // Cell 0: full block (every texel covered) with fg red over bg blue -> all red.
    // Cell 1: space (no coverage) with fg red over bg green -> all green.
    //
    // The full-block cell is the load-bearing assertion: if the glyph atlas failed to upload (the
    // glow 0.16 texImage3D bug this test exists to catch), its coverage is uniformly zero, so this
    // cell would render blue (its background) instead of red, and the test fails.
    let mut r = GlBackendBuilder::new()
        .grid_size(2, 1)
        .build()
        .expect("default-font builds a renderer");
    let cells = [
        (
            Pos::new(0, 0),
            Tile::new('\u{2588}', Style::new().fg(rgb(RED)).bg(rgb(BLUE))),
        ),
        (
            Pos::new(1, 0),
            Tile::new(' ', Style::new().fg(rgb(RED)).bg(rgb(GREEN))),
        ),
    ];
    paint(&mut r, &cells);

    let gl = webgl2_context(r.surface_size.0, r.surface_size.1);
    let frame = render_to_frame(&gl, &r);
    let (cw, ch) = r.geometry.cell_size();

    for y in 0..ch {
        for x in 0..cw {
            assert_eq!(frame.rgb(x, y), RED, "full-block pixel ({x},{y}) not fg");
            assert_eq!(frame.rgb(cw + x, y), GREEN, "blank pixel ({x},{y}) not bg");
        }
    }
}
