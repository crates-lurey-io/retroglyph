//! Headless offscreen render tests for the native GL pipeline (issue #376).
//!
//! The windowed [`GlContext`](crate::context) needs a real window handle, so it can't run in CI.
//! This module creates a *surfaceless* GL context off the windowed path (an EGL display built
//! from an EGL device (`EGL_EXT_platform_device`) via glutin's `api::egl`, made current with no
//! surface), then runs the exact same pipeline the windowed backend does (shader compile/link,
//! atlas upload, instanced draw) into an offscreen framebuffer and reads the pixels back with
//! `glReadPixels`. That is the whole point: exercise real GPU rendering, not just the CPU-side
//! atlas/shader-string units the crate already tests.
//!
//! # Platform gate
//!
//! Compiled only on Linux (`cfg(target_os = "linux")`): the EGL device platform is the portable
//! CI-able headless path (macOS's CGL pbuffer is deprecated, Windows differs), and render-
//! correctness only needs to be asserted on one platform. The whole module is `cfg(test)` too, so
//! it never ships in the crate.
//!
//! # Opt-in run (`RETROGLYPH_REQUIRE_GL`)
//!
//! These tests render only when `RETROGLYPH_REQUIRE_GL` is set; otherwise they skip. That keeps
//! the ordinary `test`/`coverage` jobs from depending on whatever GL a runner happens to expose
//! (GitHub's stock `ubuntu-latest` ships llvmpipe, so "try if a context is available" would run
//! these there against an uncontrolled driver: exactly the pixel-fragility the issue warns
//! about). The dedicated CI job (`gl-headless`) sets the flag *and* forces Mesa's llvmpipe software
//! rasterizer (`LIBGL_ALWAYS_SOFTWARE=1`, `GALLIUM_DRIVER=llvmpipe`), so rendering runs against one
//! known-good software stack. With the flag set, a missing/broken headless context is a hard
//! failure (not a silent skip), so the job can't pass without actually rendering.
//!
//! # What is asserted
//!
//! Exact-pixel snapshots are fragile across driver versions, so this uses the two strategies
//! from the issue instead, chosen to hold up across drivers:
//!
//! - Property assertions: a full-block cell is entirely its foreground; a blank cell is entirely
//!   its background; a real glyph matches the font's own coverage bits fg-vs-bg.
//! - Cross-backend parity: the same grid rendered through `retroglyph-software`'s deterministic
//!   CPU rasterizer must match the GL readback pixel-for-pixel. Both backends share
//!   `retroglyph-window`'s font, so this directly verifies the shared-font/color pixel-identity
//!   goal.

// GL wants `i32` dimensions from `u32` pixel sizes and `f32` from integer sizes; these casts are
// all bounded (test grids are tiny) and pervasive, exactly as in `renderer.rs`. Allow the cast
// family module-wide rather than dusting every call with the same attribute.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

use crate::GlRenderer;
use crate::config::GlBackendBuilder;
use crate::shaders::GlslFlavor;
use glow::HasContext as _;
use glutin::config::{ConfigSurfaceTypes, ConfigTemplateBuilder};
use glutin::context::{ContextApi, ContextAttributesBuilder, Version};
use glutin::prelude::*;
use retroglyph_core::backend::DrawCell;
use retroglyph_core::backend::Output;
use retroglyph_core::color::Color;
use retroglyph_core::color::Style;
use retroglyph_core::grid::Pos;
use retroglyph_core::tile::Tile;
use std::ffi::CString;

/// A surfaceless native GL context for offscreen rendering, created without any window.
///
/// Kept minimal: it exposes the `glow` handle and the GLSL flavor the created context needs, and
/// keeps the glutin display/context alive (dropping either would tear the GL context down while
/// `gl` still references it).
struct HeadlessContext {
    gl: glow::Context,
    flavor: GlslFlavor,
    // Order matters only for clarity; both must outlive `gl`. Held, never read.
    _context: glutin::api::egl::context::PossiblyCurrentContext,
    _display: glutin::api::egl::display::Display,
}

impl HeadlessContext {
    /// Creates a GL 3.3 core (or GLES 3.0 fallback) surfaceless context from the first EGL device.
    ///
    /// Returns `Err` with a human-readable reason if no EGL device/display/config/context is
    /// available, so the caller can decide between skipping and hard-failing.
    fn new() -> Result<Self, String> {
        use glutin::api::egl::device::Device;
        use glutin::api::egl::display::Display;

        let device = Device::query_devices()
            .map_err(|e| format!("query EGL devices: {e}"))?
            .next()
            .ok_or_else(|| "no EGL devices available".to_owned())?;

        // SAFETY: `device` came from `query_devices` above and stays valid; no raw display handle
        // is passed (offscreen, no DRM node).
        let display = unsafe { Display::with_device(&device, None) }
            .map_err(|e| format!("create EGL display from device: {e}"))?;

        // Offscreen: no window surface support, fewest samples (crisp text, no AA).
        let template = ConfigTemplateBuilder::new()
            .with_alpha_size(8)
            .with_surface_type(ConfigSurfaceTypes::empty())
            .build();
        // SAFETY: the template was built for this display.
        let config = unsafe { display.find_configs(template) }
            .map_err(|e| format!("find EGL configs: {e}"))?
            .reduce(|acc, cfg| {
                if cfg.num_samples() < acc.num_samples() {
                    cfg
                } else {
                    acc
                }
            })
            .ok_or_else(|| "no suitable EGL config".to_owned())?;

        // Prefer desktop GL 3.3 core (llvmpipe provides it), fall back to GLES 3.0. Offscreen, so
        // no raw window handle. Matches the windowed context's version/fallback choice.
        let core_attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 3))))
            .build(None);
        let gles_attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::Gles(Some(Version::new(3, 0))))
            .build(None);
        // SAFETY: `config` comes from this display.
        let not_current = unsafe {
            display
                .create_context(&config, &core_attrs)
                .or_else(|_| display.create_context(&config, &gles_attrs))
        }
        .map_err(|e| format!("create GL context: {e}"))?;

        let context = not_current
            .make_current_surfaceless()
            .map_err(|e| format!("make GL context current (surfaceless): {e}"))?;

        let flavor = match context.context_api() {
            ContextApi::Gles(_) => GlslFlavor::Es300,
            ContextApi::OpenGl(_) => GlslFlavor::Desktop330,
        };

        // SAFETY: the loader runs synchronously while `display` is current.
        let gl = unsafe {
            glow::Context::from_loader_function(|symbol| {
                CString::new(symbol).map_or(core::ptr::null(), |cname| {
                    display.get_proc_address(cname.as_c_str()).cast()
                })
            })
        };

        Ok(Self {
            gl,
            flavor,
            _context: context,
            _display: display,
        })
    }
}

/// A rendered frame read back from the GPU, stored top-left-origin (row 0 is the top), matching
/// `retroglyph-software`'s pixel buffer orientation.
struct Frame {
    width: u32,
    height: u32,
    /// RGBA bytes, `width * height * 4`, row-major from the top.
    rgba: Vec<u8>,
}

impl Frame {
    /// The `(r, g, b)` at `(x, y)` (top-left origin).
    fn rgb(&self, x: u32, y: u32) -> (u8, u8, u8) {
        let i = ((y * self.width + x) * 4) as usize;
        (self.rgba[i], self.rgba[i + 1], self.rgba[i + 2])
    }

    /// The alpha at `(x, y)` (top-left origin).
    fn alpha(&self, x: u32, y: u32) -> u8 {
        let i = ((y * self.width + x) * 4) as usize;
        self.rgba[i + 3]
    }
}

/// Renders `renderer`'s current instance array through the real pipeline into an offscreen FBO and
/// reads it back.
///
/// Builds the GL resources with the same [`GlRenderer::build_resources`] the windowed
/// `init_surface` uses, so a break in shader compile/link, atlas upload, or the instanced draw
/// shows up here. The readback is flipped to top-left origin.
fn render_to_frame(ctx: &HeadlessContext, renderer: &GlRenderer) -> Result<Frame, String> {
    let gl = &ctx.gl;
    let (w, h) = renderer.surface_size;

    #[cfg_attr(not(feature = "tilesets"), allow(unused_mut))]
    let mut res = renderer
        .build_resources(gl, ctx.flavor)
        .map_err(|e| format!("build GL resources: {e}"))?;

    // SAFETY: single-threaded test, context is current; every object is created, used, and deleted
    // within this call.
    let rgba = unsafe {
        let renderbuffer = gl
            .create_renderbuffer()
            .map_err(|e| format!("create renderbuffer: {e}"))?;
        gl.bind_renderbuffer(glow::RENDERBUFFER, Some(renderbuffer));
        gl.renderbuffer_storage(glow::RENDERBUFFER, glow::RGBA8, w as i32, h as i32);

        let framebuffer = gl
            .create_framebuffer()
            .map_err(|e| format!("create framebuffer: {e}"))?;
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
        gl.framebuffer_renderbuffer(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::RENDERBUFFER,
            Some(renderbuffer),
        );
        let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
        if status != glow::FRAMEBUFFER_COMPLETE {
            return Err(format!("framebuffer incomplete: {status:#06x}"));
        }

        // `build_resources` set the viewport/projection; clear once, then composite every layer
        // back-to-front (upload + two instanced passes each) into the bound framebuffer, the same
        // loop the windowed `present` runs.
        res.clear(gl);
        crate::draw_all_layers(&renderer.layers, gl, &mut res, renderer.cell_count() as i32);
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

    // `glReadPixels` is bottom-left origin: buffer row 0 is the image's bottom. Flip to top-left so
    // `Frame::rgb` and the software backend agree on `(x, y)`.
    let row_bytes = (w * 4) as usize;
    let mut flipped = vec![0u8; rgba.len()];
    for y in 0..h as usize {
        let src = (h as usize - 1 - y) * row_bytes;
        let dst = y * row_bytes;
        flipped[dst..dst + row_bytes].copy_from_slice(&rgba[src..src + row_bytes]);
    }

    Ok(Frame {
        width: w,
        height: h,
        rgba: flipped,
    })
}

/// Returns a headless context, or `None` when the render tests should skip.
///
/// Gated on `RETROGLYPH_REQUIRE_GL` so rendering runs only where explicitly requested (CI's
/// `gl-headless` job, or a dev opting in), never in the ordinary jobs that just happen to have a GL
/// stack. When the flag is set, failing to create a context is a hard failure rather than a skip,
/// so the dedicated job can't pass without rendering.
fn context_or_skip(test: &str) -> Option<HeadlessContext> {
    if std::env::var_os("RETROGLYPH_REQUIRE_GL").is_none() {
        eprintln!(
            "skipping {test}: set RETROGLYPH_REQUIRE_GL=1 (with a headless GL/EGL stack, e.g. Mesa \
             llvmpipe) to run"
        );
        return None;
    }
    match HeadlessContext::new() {
        Ok(ctx) => Some(ctx),
        Err(reason) => {
            panic!("{test}: RETROGLYPH_REQUIRE_GL is set but no headless GL context: {reason}")
        }
    }
}

/// Builds a GL renderer from the embedded default font.
fn gl_renderer(cols: u16, rows: u16, scale: u16) -> GlRenderer {
    GlBackendBuilder::new()
        .grid_size(cols, rows)
        .scale(scale)
        .build()
        .expect("default-font builds a renderer")
}

/// Draws `cells` (single layer) into any [`Output`]. Both backends' `draw` is infallible.
fn paint(out: &mut impl Output, cells: &[(Pos, Tile)]) {
    out.draw(cells.iter().map(|(p, t)| DrawCell::new(*p, t)))
        .ok();
}

/// Feeds a full layered frame `(layer, pos, tile)` into any [`Output`] via `draw_layers`, the way
/// the core `Terminal` drives a `PixelLayered` backend. Both GL and software composite this
/// stream themselves, so the two must agree pixel-for-pixel.
fn paint_layers(out: &mut impl Output, cells: &[(u8, Pos, Tile)]) {
    out.draw_layers(cells.iter().map(|(l, p, t)| DrawCell::on_layer(*l, *p, t)))
        .ok();
}

/// [`paint_layers`] with `tint` applied to every cell.
#[cfg(feature = "tilesets")]
fn paint_layers_tinted(
    out: &mut impl Output,
    cells: &[(u8, Pos, Tile)],
    tint: retroglyph_core::color::Tint,
) {
    out.draw_layers(
        cells
            .iter()
            .map(|(l, p, t)| DrawCell::on_layer(*l, *p, t).with_tint(tint)),
    )
    .ok();
}

const RED: (u8, u8, u8) = (0xFF, 0x00, 0x00);
const GREEN: (u8, u8, u8) = (0x00, 0xFF, 0x00);
const BLUE: (u8, u8, u8) = (0x00, 0x00, 0xFF);

#[test]
fn full_block_cell_is_all_foreground_blank_cell_is_all_background() {
    let Some(ctx) = context_or_skip("full_block_cell_is_all_foreground") else {
        return;
    };

    // Cell 0: full block (every texel covered) with fg red over bg blue -> all red.
    // Cell 1: space (no coverage) with fg red over bg green -> all green.
    let mut r = gl_renderer(2, 1, 1);
    let cells = [
        (
            Pos::new(0, 0),
            Tile::new(
                '\u{2588}',
                Style::new()
                    .fg(Color::rgb(RED.0, RED.1, RED.2))
                    .bg(Color::rgb(BLUE.0, BLUE.1, BLUE.2)),
            ),
        ),
        (
            Pos::new(1, 0),
            Tile::new(
                ' ',
                Style::new()
                    .fg(Color::rgb(RED.0, RED.1, RED.2))
                    .bg(Color::rgb(GREEN.0, GREEN.1, GREEN.2)),
            ),
        ),
    ];
    paint(&mut r, &cells);

    let frame = render_to_frame(&ctx, &r).expect("render");
    let (cw, ch) = r.geometry.cell_size();

    for y in 0..ch {
        for x in 0..cw {
            assert_eq!(frame.rgb(x, y), RED, "full-block pixel ({x},{y}) not fg");
            assert_eq!(frame.rgb(cw + x, y), GREEN, "blank pixel ({x},{y}) not bg");
        }
    }
}

/// The rendered surface is opaque everywhere: glyph coverage is a mask for the color channels, so
/// the glyph pass must leave the destination alpha alone: see the blend factors in
/// `GlResources::draw_layer`. Only a cell mixing covered and uncovered texels can tell a
/// coverage-into-alpha write apart from a correct one, since covered texels carry alpha 1 either
/// way.
///
/// Native GL presents through an opaque default framebuffer, so this is invisible on the windowed
/// desktop path; it is asserted here because this offscreen FBO is RGBA8, exactly like the WebGL2
/// canvas the page composites.
#[test]
fn glyph_coverage_leaves_the_surface_opaque() {
    let Some(ctx) = context_or_skip("glyph_coverage_leaves_the_surface_opaque") else {
        return;
    };

    let mut r = gl_renderer(1, 1, 1);
    paint(
        &mut r,
        &[(
            Pos::new(0, 0),
            Tile::new(
                'A',
                Style::new()
                    .fg(Color::rgb(RED.0, RED.1, RED.2))
                    .bg(Color::rgb(BLUE.0, BLUE.1, BLUE.2)),
            ),
        )],
    );

    let frame = render_to_frame(&ctx, &r).expect("render");
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

#[test]
fn glyph_matches_font_coverage_fg_vs_bg() {
    let Some(ctx) = context_or_skip("glyph_matches_font_coverage") else {
        return;
    };

    // A real glyph at scale 1: each set bit must be fg, each clear bit bg. This also pins the atlas
    // Y orientation (row 0 = glyph top) and the shader's y-flip.
    let mut r = gl_renderer(1, 1, 1);
    paint(
        &mut r,
        &[(
            Pos::new(0, 0),
            Tile::new(
                'A',
                Style::new()
                    .fg(Color::rgb(RED.0, RED.1, RED.2))
                    .bg(Color::rgb(BLUE.0, BLUE.1, BLUE.2)),
            ),
        )],
    );

    let frame = render_to_frame(&ctx, &r).expect("render");

    let glyph = r.glyphs.fonts().resolve('A').expect("'A' is in CP437");
    let gw = u32::from(glyph.font().glyph_width());
    let gh = u32::from(glyph.font().glyph_height());
    let rows = glyph.rows();
    for y in 0..gh {
        let mask = rows[y as usize];
        for x in 0..gw {
            // Bit 7 (MSB) is the leftmost pixel, matching the atlas builder.
            let set = (mask >> (7 - x)) & 1 == 1;
            let expected = if set { RED } else { BLUE };
            assert_eq!(frame.rgb(x, y), expected, "glyph pixel ({x},{y})");
        }
    }
}

#[test]
fn matches_software_backend_pixel_for_pixel() {
    let Some(ctx) = context_or_skip("matches_software_backend") else {
        return;
    };

    let (cols, rows, scale) = (8u16, 5u16, 2u16);
    let cells = sample_grid(cols, rows);

    // GL readback.
    let mut gl = gl_renderer(cols, rows, scale);
    paint(&mut gl, &cells);
    let frame = render_to_frame(&ctx, &gl).expect("render");

    // Reference CPU rasterization, same font, same grid.
    let mut sw = retroglyph_software::config::SoftwareBackendBuilder::new()
        .grid_size(cols, rows)
        .scale(scale)
        .build()
        .expect("default-font builds")
        .into_renderer()
        .expect("headless software renderer");
    paint(&mut sw, &cells);
    let sw_pixels = sw.pixels();

    assert_frames_match(&frame, sw_pixels);
}

#[test]
fn matches_software_backend_with_layers_pixel_for_pixel() {
    let Some(ctx) = context_or_skip("matches_software_backend_with_layers") else {
        return;
    };

    let (cols, rows, scale) = (8u16, 5u16, 2u16);
    let layered = sample_layered(cols, rows);

    // GL composites the layered stream on the GPU.
    let mut gl = gl_renderer(cols, rows, scale);
    paint_layers(&mut gl, &layered);
    let frame = render_to_frame(&ctx, &gl).expect("render");

    // Software composites the same stream on the CPU (the parity reference).
    let mut sw = retroglyph_software::config::SoftwareBackendBuilder::new()
        .grid_size(cols, rows)
        .scale(scale)
        .build()
        .expect("default-font builds")
        .into_renderer()
        .expect("headless software renderer");
    paint_layers(&mut sw, &layered);
    let sw_pixels = sw.pixels();

    assert_frames_match(&frame, sw_pixels);
}

/// A deterministic two-layer frame in the layer-major, all-cells order `Grid::layers` produces: a
/// full base layer plus a higher layer mixing empty (transparent) cells, occupied cells with a
/// `Color::Default` background (opaque, inheriting the base background), and occupied cells with
/// their own background. Exercises every branch of the occlusion rule both backends must share.
fn sample_layered(cols: u16, rows: u16) -> Vec<(u8, Pos, Tile)> {
    let base = sample_grid(cols, rows);
    let mut cells: Vec<(u8, Pos, Tile)> = base.into_iter().map(|(p, t)| (0u8, p, t)).collect();

    // Layer 1: every cell (layer-major full-frame order), mostly empty with a few overlays.
    for y in 0..rows {
        for x in 0..cols {
            let i = usize::from(y) * usize::from(cols) + usize::from(x);
            let tile = match i % 4 {
                // Empty: transparent, the base shows through.
                0 => Tile::default(),
                // Occupied, default background: opaque, inherits the base background, erases the
                // base glyph, draws its own.
                1 => Tile::new('*', Style::new().fg(Color::rgb(GREEN.0, GREEN.1, GREEN.2))),
                // Occupied space, default background: opaque erase with no visible glyph.
                2 => Tile::new(' ', Style::new().fg(Color::rgb(RED.0, RED.1, RED.2))),
                // Occupied, own background.
                _ => Tile::new(
                    '=',
                    Style::new()
                        .fg(Color::rgb(BLUE.0, BLUE.1, BLUE.2))
                        .bg(Color::rgb(GREEN.0, GREEN.1, GREEN.2)),
                ),
            };
            cells.push((1u8, Pos::new(x, y), tile));
        }
    }
    cells
}

/// A deterministic mixed grid: varied glyphs (letters, digits, punctuation, full and partial block
/// shades, spaces) and per-cell fg/bg, to exercise many atlas layers and colors at once.
fn sample_grid(cols: u16, rows: u16) -> Vec<(Pos, Tile)> {
    const GLYPHS: [char; 12] = [
        'A', 'Z', '0', '9', '#', '@', '\u{2588}', '\u{2591}', ' ', 'k', 'W', '.',
    ];
    let mut cells = Vec::with_capacity(usize::from(cols) * usize::from(rows));
    for y in 0..rows {
        for x in 0..cols {
            let i = usize::from(y) * usize::from(cols) + usize::from(x);
            let glyph = GLYPHS[i % GLYPHS.len()];
            let fg = Color::rgb(
                (i.wrapping_mul(37)) as u8,
                (i.wrapping_mul(91)) as u8,
                (i.wrapping_mul(13)) as u8,
            );
            let bg = Color::rgb(
                (i.wrapping_mul(17)) as u8,
                (i.wrapping_mul(53)) as u8,
                (i.wrapping_mul(200)) as u8,
            );
            cells.push((Pos::new(x, y), Tile::new(glyph, Style::new().fg(fg).bg(bg))));
        }
    }
    cells
}

/// Asserts every pixel of `frame` equals the matching `0x00RRGGBB` pixel in `software`.
///
/// Coverage is binary (atlas texels are `0x00`/`0xFF`) and the glyph pass blends fg over bg with
/// alpha 0 or 1, so no rounding is involved and the two backends must agree exactly.
fn assert_frames_match(frame: &Frame, software: &[u32]) {
    assert_eq!(
        (frame.width * frame.height) as usize,
        software.len(),
        "frame size vs software buffer"
    );
    let mut mismatches = Vec::new();
    for y in 0..frame.height {
        for x in 0..frame.width {
            let got = frame.rgb(x, y);
            let px = software[(y * frame.width + x) as usize];
            let want = ((px >> 16) as u8, (px >> 8) as u8, px as u8);
            if got != want && mismatches.len() < 8 {
                mismatches.push(format!("({x},{y}): gl={got:?} sw={want:?}"));
            }
            assert!(
                mismatches.len() < 8 || got == want,
                "GL vs software pixel mismatch(es): {}",
                mismatches.join(", ")
            );
        }
    }
    assert!(
        mismatches.is_empty(),
        "GL vs software pixel mismatch(es): {}",
        mismatches.join(", ")
    );
}

/// A 2-tile PNG tileset (issue #366): tile 0 solid red, tile 1 solid green, each `8x16` (the
/// Unscii cell), laid out in two columns.
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
#[test]
fn sprite_cells_render_their_tileset_colors() {
    use retroglyph_window::tileset::{Codepage, TilesetOptions};
    let Some(ctx) = context_or_skip("sprite_cells_render_their_tileset_colors") else {
        return;
    };

    // 'A' -> tile 0 (red), 'B' -> tile 1 (green). Each 8x16 sprite exactly fills one cell at
    // scale 1, so cell 0 must be all red and cell 1 all green: proving tileset decode, the RGBA
    // atlas upload, the sprite pass, and per-cell glyph -> sprite dispatch all work on real GL.
    let opts = TilesetOptions::builder(two_tile_png())
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
                Tile::new('A', Style::new().bg(Color::rgb(BLUE.0, BLUE.1, BLUE.2))),
            ),
            (
                0,
                Pos::new(1, 0),
                Tile::new('B', Style::new().bg(Color::rgb(BLUE.0, BLUE.1, BLUE.2))),
            ),
        ],
    );

    let frame = render_to_frame(&ctx, &r).expect("render");
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

#[cfg(feature = "tilesets")]
#[test]
fn a_tinted_sprite_matches_what_sprite_tint_apply_computes() {
    use retroglyph_core::color::Tint;
    use retroglyph_window::palette::DEFAULT_FG;
    use retroglyph_window::sprite_cache::SpriteTint;
    use retroglyph_window::tileset::{Codepage, SheetColor, TilesetOptions};

    let Some(ctx) = context_or_skip("a_tinted_sprite_matches_what_sprite_tint_apply_computes")
    else {
        return;
    };

    // The cross-backend contract from retroglyph#537. The CPU rasteriser calls
    // `SpriteTint::apply` directly and the fragment shader reimplements its arithmetic from
    // instance attributes, so the two can drift silently. This pins the GPU half to the same
    // function the software half is pinned to, on real GL, per tint operation.
    //
    // Rendering one tint per pass rather than one grid of many keeps each assertion pointed at a
    // single instance's attributes.
    //
    // The value set below is a boundary-focused sweep (retroglyph#555), not just a handful of
    // round numbers: 0/1/127/128/129/254/255 are exactly the inputs where `multiply_u8`'s
    // `(a*b+127)/255` and `mix_u8`'s `(delta +/- 127)/255` rounding can disagree with a naive
    // float `/255.0` normalize -> GPU multiply/mix -> `*255.0` round-trip (the bug this shader
    // rework fixes: the old `vec4`/`mix()` float path used the GPU's own float rounding, which
    // does not reproduce `(a*b+127)/255` integer rounding at these boundary values). Keeping the
    // sweep here (rather than deleting it once the fix lands) documents exactly which inputs the
    // float path used to get wrong.
    let boundary = [0u8, 1, 63, 64, 127, 128, 129, 191, 192, 254, 255];
    let mut tints: Vec<Tint> = vec![Tint::None];
    for &v in &boundary {
        tints.push(Tint::multiply(v, v, v));
        tints.push(Tint::multiply(v, 255 - v, v / 2));
    }
    for &amount in &boundary {
        tints.push(Tint::mix(0, 0, 255, amount));
        tints.push(Tint::mix(255, 255, 255, amount));
        tints.push(Tint::mix(64, 200, 32, amount));
    }
    for tint in tints {
        let opts = TilesetOptions::builder(two_tile_png())
            .tile_size(8, 16)
            .columns(2)
            .codepage(Codepage::Custom(vec!['A', 'B']))
            .build()
            .expect("valid 2-tile tileset");
        let mut r = GlBackendBuilder::new()
            .grid_size(1, 1)
            .scale(1)
            .tileset(opts)
            .build()
            .expect("gl renderer with tileset");
        paint_layers_tinted(
            &mut r,
            &[(
                0,
                Pos::new(0, 0),
                Tile::new('A', Style::new().bg(Color::rgb(BLUE.0, BLUE.1, BLUE.2))),
            )],
            tint,
        );

        let frame = render_to_frame(&ctx, &r).expect("render");
        // Tile 0 of `two_tile_png` is opaque red.
        let want =
            SpriteTint::resolve(SheetColor::Art, Color::Default, tint, DEFAULT_FG).apply(RED);
        let (cw, ch) = r.geometry.cell_size();
        for y in 0..ch {
            for x in 0..cw {
                assert_eq!(frame.rgb(x, y), want, "tint {tint:?} at ({x},{y})");
            }
        }
    }
}

/// A one-tile PNG of `w` x `h` solid opaque red pixels.
#[cfg(feature = "tilesets")]
fn wide_tile_png(w: u32, h: u32) -> Vec<u8> {
    use image::ImageEncoder as _;
    let mut img = image::RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.put_pixel(x, y, image::Rgba([0xFF, 0x00, 0x00, 0xFF]));
        }
    }
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)
        .expect("encode test tileset PNG");
    png
}

/// A `cols` x `rows` grid painted from `grid`'s layer 0, in the all-cells layer-major order a
/// `PixelLayered` backend receives.
#[cfg(feature = "tilesets")]
fn span_scene(grid: &retroglyph_core::grid::Grid) -> Vec<(u8, Pos, Tile)> {
    (0..grid.height())
        .flat_map(|y| (0..grid.width()).map(move |x| (x, y)))
        .map(|(x, y)| (0u8, Pos::new(x, y), *grid.tile(0, (x, y)).unwrap()))
        .collect()
}

#[cfg(feature = "tilesets")]
#[test]
fn multicell_span_covers_every_cell_of_its_footprint() {
    use retroglyph_core::grid::Grid;
    use retroglyph_window::tileset::{Codepage, TilesetOptions};

    let Some(ctx) = context_or_skip("multicell_span_covers_every_cell_of_its_footprint") else {
        return;
    };

    // A 16x16 sprite is two 8x16 cells wide, declared as a 2x1 span. Every pixel of both cells
    // must be the sprite, with no trace of the covered cell's fallback glyph or background.
    let opts = TilesetOptions::builder(wide_tile_png(16, 16))
        .tile_size(16, 16)
        .columns(1)
        .codepage(Codepage::Custom(vec!['S']))
        .build()
        .expect("valid 1-tile tileset");
    let mut r = GlBackendBuilder::new()
        .grid_size(2, 1)
        .scale(1)
        .tileset(opts)
        .build()
        .expect("gl renderer with tileset");

    let mut grid = Grid::new(2, 1);
    grid.write_span(
        0,
        0,
        0,
        &["S#"],
        Style::new().bg(Color::rgb(BLUE.0, BLUE.1, BLUE.2)),
    )
    .expect("2x1 span fits");
    paint_layers(&mut r, &span_scene(&grid));

    let frame = render_to_frame(&ctx, &r).expect("render");
    let (cw, ch) = r.geometry.cell_size();
    for y in 0..ch {
        for x in 0..cw * 2 {
            assert_eq!(frame.rgb(x, y), RED, "span pixel ({x},{y})");
        }
    }
}

/// Regression guard for retroglyph#762: a span's covered cell holds the text-fallback glyph as
/// its own `Tile::glyph`, and that glyph can independently have its own registered sprite (the
/// default `Codepage::Cp437` registers one for every codepoint, so this is the common case, not
/// an edge case). The covered cell must draw nothing of its own, not that sprite.
///
/// Unlike `multicell_span_covers_every_cell_of_its_footprint` (whose one-tile `Custom(['S'])`
/// sheet leaves the fallback character unregistered, so it can never collide), this registers
/// both the anchor glyph and the fallback glyph as distinct, same-size single-cell sprites, so
/// the covered cell painting its own sprite instead of staying reserved is unambiguous.
#[cfg(feature = "tilesets")]
#[test]
fn multicell_span_covered_cells_fallback_sprite_does_not_paint_over_the_anchor() {
    use retroglyph_core::grid::Grid;
    use retroglyph_window::tileset::{Codepage, TilesetOptions};

    let Some(ctx) = context_or_skip(
        "multicell_span_covered_cells_fallback_sprite_does_not_paint_over_the_anchor",
    ) else {
        return;
    };

    // 'S' -> tile 0 (red), the anchor's own sprite, exactly one cell (so it reserves but does not
    // fill cell 1). '#' -> tile 1 (green) is the covered cell's own text-fallback glyph, also
    // registered as a sprite.
    let opts = TilesetOptions::builder(two_tile_png())
        .tile_size(8, 16)
        .columns(2)
        .codepage(Codepage::Custom(vec!['S', '#']))
        .build()
        .expect("valid 2-tile tileset");
    let mut r = GlBackendBuilder::new()
        .grid_size(2, 1)
        .scale(1)
        .tileset(opts)
        .build()
        .expect("gl renderer with tileset");

    let mut grid = Grid::new(2, 1);
    grid.write_span(0, 0, 0, &["S#"], Style::default())
        .expect("2x1 span fits");
    paint_layers(&mut r, &span_scene(&grid));

    let frame = render_to_frame(&ctx, &r).expect("render");
    let (cw, ch) = r.geometry.cell_size();
    for y in 0..ch {
        for x in 0..cw {
            assert_eq!(frame.rgb(x, y), RED, "anchor cell pixel ({x},{y})");
        }
        for x in cw..cw * 2 {
            assert_ne!(
                frame.rgb(x, y),
                GREEN,
                "covered cell pixel ({x},{y}) must not draw '#''s own sprite"
            );
        }
    }
}

/// The strongest guarantee available here: multi-cell spans must render pixel-for-pixel
/// identically on the GPU and on `retroglyph-software`'s CPU rasterizer. Covers the covered-cell
/// background, the suppressed fallback glyph, and a `Center`-aligned sprite in a span box larger
/// than its art (the three things retroglyph#412 adds) in one scene.
#[cfg(feature = "tilesets")]
#[test]
fn matches_software_backend_for_multicell_spans() {
    use retroglyph_core::grid::Grid;
    use retroglyph_window::tileset::{Codepage, SpriteAlign, TilesetOptions};

    let Some(ctx) = context_or_skip("matches_software_backend_for_multicell_spans") else {
        return;
    };

    let (cols, rows, scale) = (6u16, 2u16, 2u16);
    // An 8x16 sprite is exactly one cell, so every span below reserves more cells than the art
    // fills and alignment is observable.
    let tileset = || {
        TilesetOptions::builder(wide_tile_png(8, 16))
            .tile_size(8, 16)
            .columns(1)
            .codepage(Codepage::Custom(vec!['S']))
            .align(SpriteAlign::Center)
            .build()
            .expect("valid 1-tile tileset")
    };

    let mut grid = Grid::new(cols, rows);
    for y in 0..rows {
        for x in 0..cols {
            grid.put_tile(
                0,
                (x, y),
                Tile::new('.', Style::new().bg(Color::rgb(BLUE.0, BLUE.1, BLUE.2))),
            );
        }
    }
    // A 3x1 span (art centred in it) and a 2x2 span, both with fallback glyphs a cell backend
    // would print and a pixel backend must not.
    grid.write_span(
        0,
        0,
        0,
        &["S=-"],
        Style::new().bg(Color::rgb(GREEN.0, GREEN.1, GREEN.2)),
    )
    .expect("3x1 span fits");
    grid.write_span(0, 4, 0, &["S=", "[]"], Style::new())
        .expect("2x2 span fits");
    let scene = span_scene(&grid);

    let mut gl = GlBackendBuilder::new()
        .grid_size(cols, rows)
        .scale(scale)
        .tileset(tileset())
        .build()
        .expect("gl renderer with tileset");
    paint_layers(&mut gl, &scene);
    let frame = render_to_frame(&ctx, &gl).expect("render");

    let mut sw = retroglyph_software::config::SoftwareBackendBuilder::new()
        .grid_size(cols, rows)
        .scale(scale)
        .tileset(tileset())
        .build()
        .expect("default-font builds")
        .into_renderer()
        .expect("headless software renderer");
    paint_layers(&mut sw, &scene);

    assert_frames_match(&frame, sw.pixels());
}

/// retroglyph#726: a `Color::Default`-background span on a higher layer resolves each covered
/// cell's inherited background at *that cell's own column*, not the anchor's. Layer 0 alternates
/// red/green per column so the two backends would visibly disagree if either smeared the anchor's
/// column across the span's footprint; a uniform layer 0 (as in
/// `matches_software_backend_for_multicell_spans`) can't catch that, since every column would
/// already agree.
#[test]
fn matches_software_backend_for_a_span_covered_cells_default_background() {
    use retroglyph_core::grid::Grid;

    let Some(ctx) =
        context_or_skip("matches_software_backend_for_a_span_covered_cells_default_background")
    else {
        return;
    };

    let (cols, rows, scale) = (4u16, 1u16, 4u16);
    let mut grid = Grid::new(cols, rows);
    for x in 0..cols {
        let bg = if x % 2 == 0 { RED } else { GREEN };
        grid.put_tile(
            0,
            (x, 0),
            Tile::new('.', Style::new().bg(Color::rgb(bg.0, bg.1, bg.2))),
        );
    }
    // A 2-wide span with a `Default` background over columns 0 (red) and 1 (green).
    grid.write_span(1, 0, 0, &["C="], Style::new())
        .expect("2x1 span fits");
    let scene: Vec<(u8, Pos, Tile)> = grid
        .layers()
        .map(|cell| (cell.layer, cell.pos, *cell.tile))
        .collect();

    let mut gl = GlBackendBuilder::new()
        .grid_size(cols, rows)
        .scale(scale)
        .build()
        .expect("default-font builds");
    paint_layers(&mut gl, &scene);
    let frame = render_to_frame(&ctx, &gl).expect("render");

    let mut sw = retroglyph_software::config::SoftwareBackendBuilder::new()
        .grid_size(cols, rows)
        .scale(scale)
        .build()
        .expect("default-font builds")
        .into_renderer()
        .expect("headless software renderer");
    paint_layers(&mut sw, &scene);

    assert_frames_match(&frame, sw.pixels());
}
