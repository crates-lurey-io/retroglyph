//! Headless offscreen render tests for the native GL pipeline (issue #376).
//!
//! The windowed [`GlContext`](crate::context) needs a real window handle, so it can't run in CI.
//! This module creates a *surfaceless* GL context off the windowed path -- an EGL display built
//! from an EGL device (`EGL_EXT_platform_device`) via glutin's `api::egl`, made current with no
//! surface -- then runs the exact same pipeline the windowed backend does (shader compile/link,
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
//! these there against an uncontrolled driver -- exactly the pixel-fragility the issue warns
//! about). The dedicated CI job (`gl-headless`) sets the flag *and* forces Mesa's llvmpipe software
//! rasterizer (`LIBGL_ALWAYS_SOFTWARE=1`, `GALLIUM_DRIVER=llvmpipe`), so rendering runs against one
//! known-good software stack. With the flag set, a missing/broken headless context is a hard
//! failure (not a silent skip), so the job can't pass without actually rendering.
//!
//! # What is asserted
//!
//! Exact-pixel snapshots are fragile across driver versions, so this uses the two robust
//! strategies from the issue instead:
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

use crate::GlBackendBuilder;
use crate::GlRenderer;
use crate::shaders::GlslFlavor;
use glow::HasContext as _;
use glutin::config::{ConfigSurfaceTypes, ConfigTemplateBuilder};
use glutin::context::{ContextApi, ContextAttributesBuilder, Version};
use glutin::prelude::*;
use retroglyph_core::backend::Output;
use retroglyph_core::color::Color;
use retroglyph_core::grid::Pos;
use retroglyph_core::style::Style;
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

    let res = renderer
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

        // The renderer's own draw sets the viewport, clears, and issues the two instanced passes
        // into the bound framebuffer.
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
    out.draw(cells.iter().map(|(p, t)| (*p, t, None))).ok();
}

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
            Tile::new('\u{2588}', Style::new().fg(rgb(RED)).bg(rgb(BLUE))),
        ),
        (
            Pos::new(1, 0),
            Tile::new(' ', Style::new().fg(rgb(RED)).bg(rgb(GREEN))),
        ),
    ];
    paint(&mut r, &cells);

    let frame = render_to_frame(&ctx, &r).expect("render");
    let (cw, ch) = (r.cell_w, r.cell_h);

    for y in 0..ch {
        for x in 0..cw {
            assert_eq!(frame.rgb(x, y), RED, "full-block pixel ({x},{y}) not fg");
            assert_eq!(frame.rgb(cw + x, y), GREEN, "blank pixel ({x},{y}) not bg");
        }
    }
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
            Tile::new('A', Style::new().fg(rgb(RED)).bg(rgb(BLUE))),
        )],
    );

    let frame = render_to_frame(&ctx, &r).expect("render");

    let gw = u32::from(r.font.glyph_width);
    let gh = u32::from(r.font.glyph_height);
    let idx = r.font.char_to_index('A');
    let rows = r.font.rows(idx);
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
    let mut sw = retroglyph_software::SoftwareBackendBuilder::new()
        .grid_size(cols, rows)
        .scale(scale as u8)
        .build()
        .expect("default-font builds")
        .run_headless()
        .expect("headless software renderer");
    paint(&mut sw, &cells);
    let sw_pixels = sw.pixels();

    assert_frames_match(&frame, sw_pixels);
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
            let fg = rgb((
                (i.wrapping_mul(37)) as u8,
                (i.wrapping_mul(91)) as u8,
                (i.wrapping_mul(13)) as u8,
            ));
            let bg = rgb((
                (i.wrapping_mul(17)) as u8,
                (i.wrapping_mul(53)) as u8,
                (i.wrapping_mul(200)) as u8,
            ));
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
