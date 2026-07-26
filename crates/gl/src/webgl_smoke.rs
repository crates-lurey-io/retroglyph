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
//! `init_surface` uses, render into an offscreen framebuffer, and assert on the readback, so a
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
//! the load-bearing one for the atlas-upload bug: with a failed atlas upload the glyph coverage
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
use retroglyph_core::DrawCell;
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

    /// The alpha at `(x, y)`.
    fn alpha(&self, x: u32, y: u32) -> u8 {
        let i = ((y * self.width + x) * 4) as usize;
        self.rgba[i + 3]
    }
}

/// Builds the renderer's resources, renders its current instance array into an RGBA8 offscreen
/// framebuffer via the real [`GlRenderer::build_resources`] + `draw`, and reads the pixels back.
fn render_to_frame(gl: &glow::Context, renderer: &GlRenderer) -> Frame {
    let (w, h) = renderer.surface_size;
    #[cfg_attr(not(feature = "tilesets"), allow(unused_mut))]
    let mut res = renderer
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

        // `build_resources` set the viewport/projection; clear once, then composite every layer
        // back-to-front (upload + two instanced passes each), the same loop the windowed
        // `present` runs, so a single-layer frame and a multi-layer one both go through it.
        res.clear(gl);
        for l in 0..renderer.layers.len() {
            res.upload(gl, &renderer.layers[l]);
            res.draw_layer(gl, renderer.cell_count() as i32);
            #[cfg(feature = "tilesets")]
            if let Some(sprites) = renderer.sprite_layers.get(l) {
                res.draw_sprites(gl, sprites);
            }
        }
        // Fail loudly on any GL error from the draw passes (e.g. an attribute type mismatch that
        // silently drops a draw) rather than only on the pixel assertions downstream.
        let err = gl.get_error();
        assert_eq!(err, glow::NO_ERROR, "GL error after draw loop: {err:#06x}");
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
    out.draw(cells.iter().map(|(p, t)| DrawCell::new(*p, t)))
        .ok();
}

/// Feeds a full layered frame `(layer, pos, tile)` into the renderer via `draw_layers`, the way the
/// core `Terminal` drives this `composites_layers` backend.
fn paint_layers(out: &mut GlRenderer, cells: &[(u8, Pos, Tile)]) {
    out.draw_layers(cells.iter().map(|(l, p, t)| DrawCell::on_layer(*l, *p, t)))
        .ok();
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

/// A WebGL2 canvas is composited by the page (winit requests `alpha: true`), so the surface's
/// alpha channel is load-bearing: a texel left at alpha 0 shows the document background rather
/// than the cell background painted by the background pass. Glyph coverage is a mask for the color
/// channels only, so the glyph pass must leave the destination alpha alone: see the blend factors
/// in `GlResources::draw_layer`.
///
/// A partially-covered glyph is the case that matters: the covered texels carry alpha 1 anyway, so
/// only a cell mixing covered and uncovered texels can tell a coverage-into-alpha write apart from
/// a correct one.
#[wasm_bindgen_test]
fn glyph_coverage_leaves_the_surface_opaque() {
    let mut r = GlBackendBuilder::new()
        .grid_size(1, 1)
        .build()
        .expect("default-font builds a renderer");
    paint(
        &mut r,
        &[(
            Pos::new(0, 0),
            Tile::new('A', Style::new().fg(rgb(RED)).bg(rgb(BLUE))),
        )],
    );

    let gl = webgl2_context(r.surface_size.0, r.surface_size.1);
    let frame = render_to_frame(&gl, &r);
    let (cw, ch) = r.geometry.cell_size();

    let mut covered = 0_u32;
    let mut uncovered = 0_u32;
    for y in 0..ch {
        for x in 0..cw {
            match frame.rgb(x, y) {
                RED => covered += 1,
                BLUE => uncovered += 1,
                other => panic!("pixel ({x},{y}) is neither fg nor bg: {other:?}"),
            }
            assert_eq!(
                frame.alpha(x, y),
                0xFF,
                "pixel ({x},{y}) is transparent; the glyph pass wrote coverage into alpha"
            );
        }
    }
    assert!(covered > 0, "'A' rendered no foreground texels");
    assert!(
        uncovered > 0,
        "'A' covered the whole cell; pick a sparser glyph"
    );
}

#[wasm_bindgen_test]
fn composites_two_layers_back_to_front() {
    // A 3x1 grid exercising every branch of the GPU occlusion rule (issue #368):
    //
    //   cell 0: base space on BLUE; layer 1 EMPTY        -> transparent, base shows -> BLUE
    //   cell 1: base space on BLUE; layer 1 full-block GREEN (default bg, occupied) -> GREEN
    //   cell 2: base full-block RED on BLUE; layer 1 space (default bg, occupied)
    //           -> opaque erase inheriting BLUE, space draws nothing -> BLUE (the base RED is gone)
    //
    // If higher-layer occupied cells were treated as transparent-background (the wrong model),
    // cell 2 would still show the base's RED block bleeding around the space, and cell 1's GREEN
    // block would composite over BLUE rather than replacing it.
    let block = '\u{2588}';
    let mut r = GlBackendBuilder::new()
        .grid_size(3, 1)
        .build()
        .expect("default-font builds a renderer");
    let layered = [
        (
            0u8,
            Pos::new(0, 0),
            Tile::new(' ', Style::new().bg(rgb(BLUE))),
        ),
        (
            0u8,
            Pos::new(1, 0),
            Tile::new(' ', Style::new().bg(rgb(BLUE))),
        ),
        (
            0u8,
            Pos::new(2, 0),
            Tile::new(block, Style::new().fg(rgb(RED)).bg(rgb(BLUE))),
        ),
        (1u8, Pos::new(0, 0), Tile::default()),
        (
            1u8,
            Pos::new(1, 0),
            Tile::new(block, Style::new().fg(rgb(GREEN))),
        ),
        (
            1u8,
            Pos::new(2, 0),
            Tile::new(' ', Style::new().fg(rgb(RED))),
        ),
    ];
    paint_layers(&mut r, &layered);

    let gl = webgl2_context(r.surface_size.0, r.surface_size.1);
    let frame = render_to_frame(&gl, &r);
    let (cw, ch) = r.geometry.cell_size();

    for y in 0..ch {
        for x in 0..cw {
            assert_eq!(
                frame.rgb(x, y),
                BLUE,
                "cell 0 (empty overlay) pixel ({x},{y})"
            );
            assert_eq!(
                frame.rgb(cw + x, y),
                GREEN,
                "cell 1 (opaque green block) pixel ({x},{y})"
            );
            assert_eq!(
                frame.rgb(2 * cw + x, y),
                BLUE,
                "cell 2 (opaque space erases base glyph) pixel ({x},{y})"
            );
        }
    }
}

/// A 2-tile PNG tileset (issue #366): tile 0 solid red, tile 1 solid green, each 8x16, two columns.
#[cfg(feature = "tilesets")]
fn two_tile_png() -> Vec<u8> {
    use image::ImageEncoder as _;
    let (tile_w, tile_h) = (8u32, 16u32);
    let img_w = tile_w * 2;
    let mut img = image::RgbaImage::new(img_w, tile_h);
    for y in 0..tile_h {
        for x in 0..img_w {
            let px = if x < tile_w {
                [0xFF, 0x00, 0x00, 0xFF]
            } else {
                [0x00, 0xFF, 0x00, 0xFF]
            };
            img.put_pixel(x, y, image::Rgba(px));
        }
    }
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(img.as_raw(), img_w, tile_h, image::ExtendedColorType::Rgba8)
        .expect("encode test tileset PNG");
    png
}

#[cfg(feature = "tilesets")]
#[wasm_bindgen_test]
fn sprite_cells_render_their_tileset_colors() {
    // The full sprite path on real WebGL2 (issue #366): 'A' -> tile 0 (red), 'B' -> tile 1 (green),
    // each 8x16 sprite filling one cell. Proves tileset decode, the RGBA atlas upload, the sprite
    // pass, and glyph -> sprite dispatch all work in the browser.
    use retroglyph_window::tileset::{Codepage, TilesetOptions};
    let opts = TilesetOptions::from_bytes(two_tile_png())
        .tile_size(8, 16)
        .columns(2)
        .codepage(Codepage::Custom(vec!['A', 'B']))
        .build()
        .expect("valid 2-tile tileset");
    let mut r = GlBackendBuilder::new()
        .grid_size(2, 1)
        .scale(1)
        .tileset(opts)
        .build()
        .expect("gl renderer with tileset");
    // Route through `draw_layers` (layer 0), the path the compositing GL backend actually uses --
    // sprite dispatch lives there, not in the single-layer `draw`.
    paint_layers(
        &mut r,
        &[
            (
                0,
                Pos::new(0, 0),
                Tile::new('A', Style::new().bg(rgb(BLUE))),
            ),
            (
                0,
                Pos::new(1, 0),
                Tile::new('B', Style::new().bg(rgb(BLUE))),
            ),
        ],
    );

    // draw_layers must have collected one sprite instance per cell on layer 0.
    assert_eq!(
        r.sprite_layers.first().map(Vec::len),
        Some(2),
        "sprite dispatch did not collect instances"
    );

    let gl = webgl2_context(r.surface_size.0, r.surface_size.1);
    let frame = render_to_frame(&gl, &r);
    let (cw, ch) = r.geometry.cell_size();
    for y in 0..ch {
        for x in 0..cw {
            assert_eq!(frame.rgb(x, y), RED, "sprite 'A' cell pixel ({x},{y})");
            assert_eq!(
                frame.rgb(cw + x, y),
                GREEN,
                "sprite 'B' cell pixel ({x},{y})"
            );
        }
    }
}
