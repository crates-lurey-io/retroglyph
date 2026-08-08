//! Offscreen render tests for the real wgpu pipeline.
//!
//! The windowed path needs a window handle, so it can't run in CI. These tests take the other
//! branch `wgpu` already offers: a device requested with no compatible surface at all, rendering
//! into a texture this module owns and reads back. Everything else is the production path, built by
//! the same [`WgpuRenderer::build_resources`] and encoded by the same
//! [`GpuResources::render`](crate::renderer::GpuResources::render) that
//! [`Presenter::present`](retroglyph_window::presenter::Presenter::present) drives, so a break in pipeline
//! creation, atlas upload, the instance layout, or either shader shows up here.
//!
//! # No platform gate
//!
//! `retroglyph-gl`'s equivalent is Linux-only, because a surfaceless GL context means EGL device
//! platform there and nothing portable elsewhere. `wgpu` has no such split: "no surface" is a
//! first-class option on Vulkan, Metal, and D3D12 alike, so these run wherever an adapter exists,
//! including a developer's laptop.
//!
//! # Opt-in enforcement (`RETROGLYPH_REQUIRE_WGPU`)
//!
//! A machine with no adapter at all (a container without a GPU or a software rasterizer) skips
//! these with a message rather than failing, so `cargo test` stays useful there. Setting
//! `RETROGLYPH_REQUIRE_WGPU` turns a missing adapter into a hard failure, which is what the
//! dedicated CI job does so it cannot pass without actually rendering.
//!
//! # What is asserted
//!
//! Exact-pixel snapshots are fragile across driver versions, so this uses two strategies that
//! aren't:
//!
//! - Property assertions: a full-block cell is entirely its foreground, a blank cell is entirely
//!   its background, a real glyph matches the font's own coverage bits fg-vs-bg.
//! - Cross-backend parity: the same grid rendered through `retroglyph-software`'s deterministic CPU
//!   rasterizer must match the readback pixel for pixel. Both backends share
//!   `retroglyph-window`'s font, so this directly verifies pixel identity between them.
//!
//! Parity is exact rather than approximate by construction: the atlas is sampled `Nearest` from a
//! texture whose texels are only `0x00` or `0xFF`, so the glyph pass's alpha is only ever 0 or 1
//! and its blend resolves to exactly the source or exactly the destination. No intermediate value,
//! and so no rounding, ever reaches the framebuffer.

use crate::WgpuRenderer;
use crate::config::WgpuBackendBuilder;
use crate::gpu::GpuContext;
use retroglyph_core::backend::{DrawCell, Output};
use retroglyph_core::color::{Color, Style};
use retroglyph_core::grid::Pos;
use retroglyph_core::tile::Tile;

/// A read-back frame, RGBA8, row-major from the top-left.
struct Frame {
    width: u32,
    height: u32,
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

/// The format the offscreen target uses.
///
/// Deliberately not an `*Srgb` format, for the same reason the windowed path views its surface
/// through a non-sRGB format: the shader emits `u8 / 255.0`, and an sRGB target would re-encode
/// those on write and put every channel out of step with the CPU rasterizer.
const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Renders `renderer`'s current instance arrays through the real pipeline into an offscreen
/// texture and reads the pixels back.
fn render_to_frame(renderer: &mut WgpuRenderer, context: GpuContext) -> Frame {
    let (width, height) = renderer.surface_size;
    let resources = renderer
        .build_resources(&context, TARGET_FORMAT)
        .expect("build GPU resources");

    let target = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: TARGET_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());

    // Install the device and resources so `encode_frame` runs the same body `present` does, then
    // point it at this texture instead of a swap-chain frame.
    renderer.install_offscreen(context, resources);
    renderer.encode_frame(&view);

    let context = renderer.offscreen_context().expect("installed above");
    read_back(context, &target, width, height)
}

/// Copies `texture` into a mappable buffer and returns its pixels, undoing the 256-byte row
/// padding a texture-to-buffer copy requires.
fn read_back(context: &GpuContext, texture: &wgpu::Texture, width: u32, height: u32) -> Frame {
    let unpadded = width * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded = unpadded.div_ceil(align) * align;

    let buffer = context.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("offscreen readback"),
        size: u64::from(padded) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = context
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("offscreen readback"),
        });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    context.queue.submit(Some(encoder.finish()));

    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| {
        result.expect("map readback buffer");
    });
    context
        .device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .expect("poll to completion");

    let mapped = slice.get_mapped_range().expect("mapped readback buffer");
    let mut rgba = Vec::with_capacity((unpadded * height) as usize);
    for row in 0..height as usize {
        let start = row * padded as usize;
        rgba.extend_from_slice(&mapped[start..start + unpadded as usize]);
    }
    drop(mapped);
    buffer.unmap();

    // No flip: unlike `glReadPixels`, a wgpu render target's row 0 is the image's top row, which is
    // already what `Frame::rgb` and the software backend agree on.
    Frame {
        width,
        height,
        rgba,
    }
}

/// A device to render with, or `None` when these tests should skip.
///
/// See the module docs for `RETROGLYPH_REQUIRE_WGPU`.
fn device_or_skip(test: &str) -> Option<GpuContext> {
    match GpuContext::offscreen() {
        Ok(context) => Some(context),
        Err(reason) => {
            assert!(
                std::env::var_os("RETROGLYPH_REQUIRE_WGPU").is_none(),
                "{test}: RETROGLYPH_REQUIRE_WGPU is set but no adapter is available: {reason}"
            );
            eprintln!("skipping {test}: no wgpu adapter available ({reason})");
            None
        }
    }
}

/// A renderer built from the embedded default font.
fn renderer(cols: u16, rows: u16, scale: u16) -> WgpuRenderer {
    WgpuBackendBuilder::new()
        .grid_size(cols, rows)
        .scale(scale)
        .build()
        .expect("default-font builds a renderer")
}

/// A software renderer over the same grid, the CPU parity reference.
fn software(cols: u16, rows: u16, scale: u16) -> retroglyph_software::SoftwareRenderer {
    retroglyph_software::config::SoftwareBackendBuilder::new()
        .grid_size(cols, rows)
        .scale(scale)
        .build()
        .expect("default-font builds")
        .into_renderer()
        .expect("headless software renderer")
}

/// Draws `cells` (single layer) into any [`Output`]. Every backend's `draw` here is infallible.
fn paint(out: &mut impl Output, cells: &[(Pos, Tile)]) {
    out.draw(cells.iter().map(|(p, t)| DrawCell::new(*p, t)))
        .ok();
}

/// Feeds a full layered frame into any [`Output`] via `draw_layers`, the way the core `Terminal`
/// drives a `composites_layers` backend.
fn paint_layers(out: &mut impl Output, cells: &[(u8, Pos, Tile)]) {
    out.draw_layers(cells.iter().map(|(l, p, t)| DrawCell::on_layer(*l, *p, t)))
        .ok();
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

/// Asserts a readback frame equals the software backend's pixel buffer, reporting the first
/// mismatch with both colors rather than dumping the whole frame.
fn assert_frames_match(frame: &Frame, software: &[u32]) {
    assert_eq!(
        software.len(),
        (frame.width * frame.height) as usize,
        "the two backends disagree on the surface size"
    );
    for y in 0..frame.height {
        for x in 0..frame.width {
            let packed = software[(y * frame.width + x) as usize];
            #[allow(clippy::cast_possible_truncation)]
            let expected = ((packed >> 16) as u8, (packed >> 8) as u8, packed as u8);
            assert_eq!(
                frame.rgb(x, y),
                expected,
                "pixel ({x},{y}) differs from the software rasterizer"
            );
        }
    }
}

/// A deterministic single-layer grid covering blanks, glyphs, colors, and sub-cell offsets.
fn sample_grid(cols: u16, rows: u16) -> Vec<(Pos, Tile)> {
    let mut cells = Vec::new();
    for y in 0..rows {
        for x in 0..cols {
            let i = usize::from(y) * usize::from(cols) + usize::from(x);
            let glyph = [' ', 'A', '\u{2588}', '#', 'w', '.'][i % 6];
            let style = Style::new()
                .fg(rgb([RED, GREEN, BLUE][i % 3]))
                .bg(rgb([BLUE, RED, GREEN][i % 3]));
            let tile = match i % 5 {
                // A sub-cell offset, so the two backends must agree on spill as well as placement.
                3 => Tile::new(glyph, style).with_offset(-2, 1),
                4 => Tile::new(glyph, style).with_offset(3, -1),
                _ => Tile::new(glyph, style),
            };
            cells.push((Pos::new(x, y), tile));
        }
    }
    cells
}

/// A deterministic two-layer frame in the layer-major, all-cells order `Grid::layers` produces:
/// a full base layer plus a higher layer mixing empty (transparent) cells, occupied cells with a
/// `Color::Default` background (opaque, inheriting the base background), and occupied cells with
/// their own background. Exercises every branch of the occlusion rule both backends share.
fn sample_layered(cols: u16, rows: u16) -> Vec<(u8, Pos, Tile)> {
    let base = sample_grid(cols, rows);
    let mut cells: Vec<(u8, Pos, Tile)> = base.into_iter().map(|(p, t)| (0u8, p, t)).collect();
    for y in 0..rows {
        for x in 0..cols {
            let i = usize::from(y) * usize::from(cols) + usize::from(x);
            let tile = match i % 4 {
                // Empty: transparent, the base shows through.
                0 => Tile::default(),
                // Occupied, default background: opaque, inherits the base background, erases the
                // base glyph, draws its own.
                1 => Tile::new('*', Style::new().fg(rgb(GREEN))),
                // Occupied space, default background: opaque erase with no visible glyph.
                2 => Tile::new(' ', Style::new().fg(rgb(RED))),
                // Occupied with its own background: fully opaque, own colors.
                _ => Tile::new('o', Style::new().fg(rgb(BLUE)).bg(rgb(GREEN))),
            };
            cells.push((1, Pos::new(x, y), tile));
        }
    }
    cells
}

/// `Presenter::geometry`/`cell_size` delegate to the renderer's internal `CellGeometry` with no
/// device required, so unlike the render tests below this always runs.
#[test]
fn presenter_geometry_and_cell_size_match_the_internal_geometry() {
    use retroglyph_window::presenter::Presenter as _;

    let r = renderer(2, 1, 3);
    assert_eq!(r.geometry(), r.geometry);
    assert_eq!(r.cell_size(), r.geometry.cell_size());
}

#[test]
fn full_block_cell_is_all_foreground_blank_cell_is_all_background() {
    let Some(device) = device_or_skip("full_block_cell_is_all_foreground") else {
        return;
    };

    // Cell 0: full block (every texel covered) with fg red over bg blue -> all red.
    // Cell 1: space (no coverage) with fg red over bg green -> all green.
    let mut r = renderer(2, 1, 1);
    paint(
        &mut r,
        &[
            (
                Pos::new(0, 0),
                Tile::new('\u{2588}', Style::new().fg(rgb(RED)).bg(rgb(BLUE))),
            ),
            (
                Pos::new(1, 0),
                Tile::new(' ', Style::new().fg(rgb(RED)).bg(rgb(GREEN))),
            ),
        ],
    );

    let (cw, ch) = r.geometry.cell_size();
    let frame = render_to_frame(&mut r, device);

    for y in 0..ch {
        for x in 0..cw {
            assert_eq!(frame.rgb(x, y), RED, "full-block pixel ({x},{y}) not fg");
            assert_eq!(frame.rgb(cw + x, y), GREEN, "blank pixel ({x},{y}) not bg");
        }
    }
}

/// The rendered surface is opaque everywhere: glyph coverage is a mask for the color channels, so
/// the glyph pass must leave the destination alpha alone (see `GLYPH_BLEND`). Only a cell mixing
/// covered and uncovered texels can tell a coverage-into-alpha write apart from a correct one,
/// since covered texels carry alpha 1 either way.
#[test]
fn glyph_coverage_leaves_the_surface_opaque() {
    let Some(device) = device_or_skip("glyph_coverage_leaves_the_surface_opaque") else {
        return;
    };

    let mut r = renderer(1, 1, 1);
    paint(
        &mut r,
        &[(
            Pos::new(0, 0),
            Tile::new('A', Style::new().fg(rgb(RED)).bg(rgb(BLUE))),
        )],
    );

    let (cw, ch) = r.geometry.cell_size();
    let frame = render_to_frame(&mut r, device);

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
    let Some(device) = device_or_skip("glyph_matches_font_coverage") else {
        return;
    };

    // A real glyph at scale 1: each set bit must be fg, each clear bit bg. This also pins the atlas
    // Y orientation (row 0 = glyph top) and the shader's y-flip.
    let mut r = renderer(1, 1, 1);
    paint(
        &mut r,
        &[(
            Pos::new(0, 0),
            Tile::new('A', Style::new().fg(rgb(RED)).bg(rgb(BLUE))),
        )],
    );

    let glyph = r.glyphs.fonts().resolve('A').expect("'A' is in CP437");
    let gw = u32::from(glyph.font().glyph_width());
    let gh = u32::from(glyph.font().glyph_height());
    let rows: Vec<u8> = glyph.rows().to_vec();
    let frame = render_to_frame(&mut r, device);

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
    let Some(device) = device_or_skip("matches_software_backend") else {
        return;
    };

    let (cols, rows, scale) = (8u16, 5u16, 2u16);
    let cells = sample_grid(cols, rows);

    let mut gpu = renderer(cols, rows, scale);
    paint(&mut gpu, &cells);
    let frame = render_to_frame(&mut gpu, device);

    let mut cpu = software(cols, rows, scale);
    paint(&mut cpu, &cells);

    assert_frames_match(&frame, cpu.pixels());
}

#[test]
fn matches_software_backend_with_layers_pixel_for_pixel() {
    let Some(device) = device_or_skip("matches_software_backend_with_layers") else {
        return;
    };

    let (cols, rows, scale) = (8u16, 5u16, 2u16);
    let layered = sample_layered(cols, rows);

    // The GPU composites the layered stream in one render pass.
    let mut gpu = renderer(cols, rows, scale);
    paint_layers(&mut gpu, &layered);
    let frame = render_to_frame(&mut gpu, device);

    // Software composites the same stream on the CPU (the parity reference).
    let mut cpu = software(cols, rows, scale);
    paint_layers(&mut cpu, &layered);

    assert_frames_match(&frame, cpu.pixels());
}

/// A sub-cell offset must move only the glyph, never the cell's background fill, and the spill must
/// reach a neighbour identically in every direction. Checking that against the CPU rasterizer at a
/// scale above 1 is the strongest available statement of the shared spill contract: the shader
/// shifts a quad's vertices, the CPU blit shifts a blit origin, and the two must still agree
/// texel for texel.
#[test]
fn sub_cell_offsets_spill_the_same_way_as_the_cpu_rasterizer() {
    let Some(device) = device_or_skip("sub_cell_offsets_spill_the_same_way") else {
        return;
    };

    let (cols, rows, scale) = (3u16, 3u16, 3u16);
    // A ring of offset full blocks around a blank centre: every direction of spill lands on a
    // neighbour whose own background must survive underneath it.
    let mut cells = Vec::new();
    for y in 0..rows {
        for x in 0..cols {
            #[allow(clippy::cast_possible_wrap)]
            let (dx, dy) = (x as i16 - 1, y as i16 - 1);
            let tile = Tile::new('\u{2588}', Style::new().fg(rgb(GREEN)).bg(rgb(BLUE)))
                .with_offset(dx * 3, dy * 3);
            cells.push((Pos::new(x, y), tile));
        }
    }

    let mut gpu = renderer(cols, rows, scale);
    paint(&mut gpu, &cells);
    let frame = render_to_frame(&mut gpu, device);

    let mut cpu = software(cols, rows, scale);
    paint(&mut cpu, &cells);

    assert_frames_match(&frame, cpu.pixels());
}

/// retroglyph#726: a `Color::Default`-background span on a higher layer resolves each covered
/// cell's inherited background at *that cell's own column*, not the anchor's. Layer 0 alternates
/// red/green per column, so the two backends would visibly disagree if either smeared the anchor's
/// column across the span's footprint; a uniform layer 0 could not catch that, since every column
/// would already agree.
#[test]
fn matches_software_backend_for_a_span_covered_cells_default_background() {
    use retroglyph_core::grid::Grid;

    let Some(device) = device_or_skip("matches_software_backend_for_a_span_covered_cell") else {
        return;
    };

    let (cols, rows, scale) = (4u16, 1u16, 4u16);
    let mut grid = Grid::new(cols, rows);
    for x in 0..cols {
        let bg = if x % 2 == 0 { RED } else { GREEN };
        grid.put_tile(0, (x, 0), Tile::new('.', Style::new().bg(rgb(bg))));
    }
    // A 2-wide span with a `Default` background over columns 0 (red) and 1 (green).
    grid.write_span(1, 0, 0, &["C="], Style::new())
        .expect("2x1 span fits");
    let scene: Vec<(u8, Pos, Tile)> = grid
        .layers()
        .map(|cell| (cell.layer, cell.pos, *cell.tile))
        .collect();

    let mut gpu = renderer(cols, rows, scale);
    paint_layers(&mut gpu, &scene);
    let frame = render_to_frame(&mut gpu, device);

    let mut cpu = software(cols, rows, scale);
    paint_layers(&mut cpu, &scene);

    assert_frames_match(&frame, cpu.pixels());
}

/// A multi-cell span draws one piece of artwork across its whole footprint: the anchor's glyph is
/// drawn once and the covered cells draw none of their own, on one uniform backdrop
/// (retroglyph#412). Checked against the CPU rasterizer, so a covered cell that wrongly drew its
/// text-fallback glyph shows up as a pixel difference rather than passing silently.
#[test]
fn matches_software_backend_for_multicell_spans() {
    use retroglyph_core::grid::Grid;

    let Some(device) = device_or_skip("matches_software_backend_for_multicell_spans") else {
        return;
    };

    let (cols, rows, scale) = (4u16, 2u16, 3u16);
    let mut grid = Grid::new(cols, rows);
    for y in 0..rows {
        for x in 0..cols {
            grid.put_tile(0, (x, y), Tile::new('.', Style::new().fg(rgb(BLUE))));
        }
    }
    // A 2x2 span with its own background, so both the footprint and the backdrop are exercised.
    grid.write_span(
        0,
        0,
        0,
        &["AB", "CD"],
        Style::new().fg(rgb(GREEN)).bg(rgb(RED)),
    )
    .expect("2x2 span fits");
    let scene: Vec<(u8, Pos, Tile)> = grid
        .layers()
        .map(|cell| (cell.layer, cell.pos, *cell.tile))
        .collect();

    let mut gpu = renderer(cols, rows, scale);
    paint_layers(&mut gpu, &scene);
    let frame = render_to_frame(&mut gpu, device);

    let mut cpu = software(cols, rows, scale);
    paint_layers(&mut cpu, &scene);

    assert_frames_match(&frame, cpu.pixels());
}

#[cfg(feature = "tilesets")]
mod sprites {
    use super::{
        BLUE, Frame, GREEN, RED, assert_frames_match, device_or_skip, paint_layers,
        render_to_frame, rgb,
    };
    use crate::WgpuRenderer;
    use crate::config::WgpuBackendBuilder;
    use retroglyph_core::color::Style;
    use retroglyph_core::grid::Pos;
    use retroglyph_core::tile::Tile;
    use retroglyph_window::tileset::{Codepage, SheetColor, TilesetOptions};

    /// A one-tile PNG sheet of solid `color`, sized `tile` x `tile`.
    fn sheet(color: (u8, u8, u8), tile: u32) -> Vec<u8> {
        let mut img = image::RgbaImage::new(tile, tile);
        for px in img.pixels_mut() {
            *px = image::Rgba([color.0, color.1, color.2, 0xFF]);
        }
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encode png");
        png
    }

    /// A renderer and a software reference sharing one art tileset, so a sprite cell can be
    /// compared across the two.
    fn pair(
        cols: u16,
        rows: u16,
        scale: u16,
        png: &[u8],
        tile: u16,
        color: SheetColor,
    ) -> (WgpuRenderer, retroglyph_software::SoftwareRenderer) {
        let opts = || {
            TilesetOptions::builder(png.to_vec())
                .tile_size(tile, tile)
                .columns(1)
                .codepage(Codepage::Custom(vec!['@']))
                .color(color)
                .build()
                .expect("valid one-tile tileset")
        };
        let gpu = WgpuBackendBuilder::new()
            .grid_size(cols, rows)
            .scale(scale)
            .tileset(opts())
            .build()
            .expect("tileset builds");
        let cpu = retroglyph_software::config::SoftwareBackendBuilder::new()
            .grid_size(cols, rows)
            .scale(scale)
            .tileset(opts())
            .build()
            .expect("tileset builds")
            .into_renderer()
            .expect("headless software renderer");
        (gpu, cpu)
    }

    /// An art sheet's sprite renders its own colors, not the cell's foreground, and matches the CPU
    /// blit exactly.
    #[test]
    fn art_sprites_match_the_cpu_blit() {
        let Some(device) = device_or_skip("art_sprites_match_the_cpu_blit") else {
            return;
        };
        let png = sheet(GREEN, 8);
        let (mut gpu, mut cpu) = pair(2, 1, 2, &png, 8, SheetColor::Art);

        // '@' is the sheet's only tile; the cell's contrasting foreground makes a sprite that was
        // wrongly drawn as a font glyph obvious.
        let sprite = Tile::new('@', Style::new().fg(rgb(RED)).bg(rgb(BLUE)));
        let plain = Tile::new('A', Style::new().fg(rgb(RED)).bg(rgb(BLUE)));
        let cells = [(0u8, Pos::new(0, 0), sprite), (0u8, Pos::new(1, 0), plain)];
        paint_layers(&mut gpu, &cells);
        paint_layers(&mut cpu, &cells);

        let frame: Frame = render_to_frame(&mut gpu, device);
        assert_frames_match(&frame, cpu.pixels());
        // And the sprite really did paint its own color, so the comparison above isn't two
        // backends agreeing on having drawn nothing.
        assert_eq!(frame.rgb(0, 0), GREEN);
    }

    /// A mask sheet's sprite is multiplied by the cell's foreground, in the exact `u8` arithmetic
    /// `SpriteTint::apply` uses. A float approximation in the shader would be off by one here.
    #[test]
    fn mask_sprites_take_the_cell_foreground_in_exact_integer_math() {
        let Some(device) = device_or_skip("mask_sprites_take_the_cell_foreground") else {
            return;
        };
        // A mid-grey sheet times a mid-grey foreground is where integer and float rounding diverge.
        let png = sheet((0x80, 0x80, 0x80), 8);
        let (mut gpu, mut cpu) = pair(1, 1, 1, &png, 8, SheetColor::Mask);

        let fg = (0x33, 0x77, 0xBB);
        let cells = [(
            0u8,
            Pos::new(0, 0),
            Tile::new('@', Style::new().fg(rgb(fg)).bg(rgb(BLUE))),
        )];
        paint_layers(&mut gpu, &cells);
        paint_layers(&mut cpu, &cells);

        let frame = render_to_frame(&mut gpu, device);
        assert_frames_match(&frame, cpu.pixels());
    }
}
