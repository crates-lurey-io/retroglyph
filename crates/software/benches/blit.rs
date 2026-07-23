//! Benchmarks comparing `blit_glyph` (bitmap font path) against
//! `blit_sprite` (`tilesets` feature, PNG sprite path), opaque vs
//! alpha-blended, to guard `blit_sprite`'s opaque-pixel fast path (source
//! pixels with `alpha == 255` skip `U8x4Rgba` construction and
//! `source_over` entirely; see `blit_sprite`'s doc comment in `lib.rs`)
//! (retroglyph#307).
//!
//! Both blit functions are crate-private, so this bench (an external
//! `benches/` binary) drives them indirectly through `Backend::draw_layers`
//! with content engineered so every cell takes exactly one path: an
//! all-glyph frame for `blit_glyph`, and all-sprite frames (fully opaque vs
//! fully alpha-blended) for `blit_sprite`. `blit_glyph` has no alpha
//! concept -- bitmap fonts are 1-bit, so there is only one `blit_glyph`
//! variant here, not an opaque/alpha pair.
//!
//! Requires `--all-features` (`default-font` for the font, `tilesets` for
//! `blit_sprite`; see the crate-level note in `AGENTS.md`).

#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use retroglyph_core::{Color, Output, Pos, Style, Tile};
use retroglyph_software::SoftwareBackendBuilder;
use retroglyph_software::bitmap_font::unscii16;

#[cfg(feature = "tilesets")]
use retroglyph_software::tileset::{Codepage, TilesetOptions};

const GRID: (u16, u16) = (80, 24);

fn to_content(frame: &[(u8, Pos, Tile)]) -> impl Iterator<Item = (u8, Pos, &Tile, Option<&str>)> {
    frame
        .iter()
        .map(|(layer, pos, tile)| (*layer, *pos, tile, None))
}

fn glyph_frame(cols: u16, rows: u16) -> Vec<(u8, Pos, Tile)> {
    let style = Style::new().fg(Color::Rgb { r: 0, g: 255, b: 0 });
    let mut out = Vec::with_capacity(usize::from(cols) * usize::from(rows));
    for y in 0..rows {
        for x in 0..cols {
            out.push((0, Pos::new(x, y), Tile::new('@', style)));
        }
    }
    out
}

/// Builds a single-tile PNG sprite sheet with a uniform per-pixel alpha, so
/// every pixel in every drawn sprite takes the same `blit_sprite` branch
/// (opaque fast path for `alpha == 255`, blended path otherwise).
#[cfg(feature = "tilesets")]
fn make_sprite_sheet_png(alpha: u8) -> Vec<u8> {
    use image::ImageEncoder;

    let (tile_w, tile_h) = (16u32, 16u32);
    let mut pixels = vec![0u8; (tile_w * tile_h * 4) as usize];
    for px in pixels.chunks_mut(4) {
        px[0] = 255; // r
        px[1] = 0; // g
        px[2] = 0; // b
        px[3] = alpha;
    }
    let mut out = std::io::Cursor::new(Vec::new());
    let encoder = image::codecs::png::PngEncoder::new(&mut out);
    encoder
        .write_image(&pixels, tile_w, tile_h, image::ExtendedColorType::Rgba8)
        .unwrap();
    out.into_inner()
}

#[cfg(feature = "tilesets")]
fn sprite_frame(cols: u16, rows: u16) -> Vec<(u8, Pos, Tile)> {
    let mut out = Vec::with_capacity(usize::from(cols) * usize::from(rows));
    for y in 0..rows {
        for x in 0..cols {
            // 'A' is the tileset's only mapped codepoint (see start_codepoint below).
            out.push((0, Pos::new(x, y), Tile::new('A', Style::new())));
        }
    }
    out
}

#[cfg(feature = "tilesets")]
fn sprite_renderer(alpha: u8) -> retroglyph_software::SoftwareRenderer {
    let tileset = TilesetOptions::from_bytes(make_sprite_sheet_png(alpha))
        .tile_size(16, 16)
        .codepage(Codepage::Unicode { start: 'A' })
        .build()
        .unwrap();
    SoftwareBackendBuilder::new()
        .font(unscii16::FONT)
        .grid_size(GRID.0, GRID.1)
        .scale(1)
        .tileset(tileset)
        .build()
        .unwrap()
        .run_headless()
        .unwrap()
}

fn blit(c: &mut Criterion) {
    let mut group = c.benchmark_group(format!("blit/{}x{}", GRID.0, GRID.1));

    let frame = glyph_frame(GRID.0, GRID.1);
    let mut renderer = SoftwareBackendBuilder::new()
        .font(unscii16::FONT)
        .grid_size(GRID.0, GRID.1)
        .scale(1)
        .build()
        .unwrap()
        .run_headless()
        .unwrap();
    group.bench_function("blit_glyph", |b| {
        b.iter(|| renderer.draw_layers(to_content(&frame)).unwrap());
    });

    #[cfg(feature = "tilesets")]
    {
        let sprite_content = sprite_frame(GRID.0, GRID.1);

        let mut opaque_renderer = sprite_renderer(255);
        group.bench_function("blit_sprite_opaque", |b| {
            b.iter(|| {
                opaque_renderer
                    .draw_layers(to_content(&sprite_content))
                    .unwrap();
            });
        });

        let mut alpha_renderer = sprite_renderer(128);
        group.bench_function("blit_sprite_alpha", |b| {
            b.iter(|| {
                alpha_renderer
                    .draw_layers(to_content(&sprite_content))
                    .unwrap();
            });
        });
    }

    group.finish();
}

criterion_group!(benches, blit);
criterion_main!(benches);
