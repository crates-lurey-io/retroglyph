//! Benchmarks for full-frame `Output::draw_layers` rasterization in
//! `retroglyph-software`, the CPU hot path this crate has never measured
//! (retroglyph#307).
//!
//! `draw_layers` is called once per frame with `needs_full_frame() == true`
//! (see `SoftwareRenderer::needs_full_frame`), so it always redraws every
//! cell regardless of how much actually changed: `scale` multiplies the
//! per-glyph blit inner loop quadratically (each source pixel becomes a
//! `scale x scale` block), so this benchmark sweeps grid size and `scale`
//! together, and separates glyph-heavy content (the `blit_glyph` path) from
//! sprite-heavy content (the `blit_sprite` path, `tilesets` feature only) so
//! the two rasterization strategies can be compared instead of averaged
//! together.
//!
//! Requires `--all-features`: `default-font` supplies the bitmap font this
//! bench draws with (see the crate-level note in `AGENTS.md` about
//! `retroglyph-software`'s default features not including `default-font`).

#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use retroglyph_core::{Color, Output, Pos, Style, Tile};
use retroglyph_font::unscii16;
use retroglyph_software::SoftwareBackendBuilder;

#[cfg(feature = "tilesets")]
use retroglyph_software::tileset::{Codepage, TilesetOptions};

/// Builds a deterministic glyph-heavy frame: every cell holds a distinct
/// printable ASCII glyph (cycling) with a distinct RGB foreground, driven by
/// a seeded RNG so the frame content is stable across runs.
fn glyph_frame(cols: u16, rows: u16) -> Vec<(u8, Pos, Tile)> {
    let mut rng = fastrand::Rng::with_seed(42);
    let glyphs: Vec<char> = ('!'..='~').collect();
    let mut out = Vec::with_capacity(usize::from(cols) * usize::from(rows));
    for y in 0..rows {
        for x in 0..cols {
            let glyph = glyphs[rng.usize(0..glyphs.len())];
            let style = Style::new().fg(Color::Rgb {
                r: rng.u8(..),
                g: rng.u8(..),
                b: rng.u8(..),
            });
            out.push((0, Pos::new(x, y), Tile::new(glyph, style)));
        }
    }
    out
}

/// Builds a 16x16-tile CP437 PNG sprite sheet in memory (see
/// `sprite_cache.rs`'s `make_test_png` for the same pattern used in-crate).
#[cfg(feature = "tilesets")]
fn make_sprite_sheet_png() -> Vec<u8> {
    use image::ImageEncoder;

    let (tile_w, tile_h, cols, rows) = (16u32, 16u32, 16u32, 16u32);
    let img_w = tile_w * cols;
    let img_h = tile_h * rows;
    let mut pixels = vec![0u8; (img_w * img_h * 4) as usize];
    for row in 0..rows {
        for col in 0..cols {
            #[allow(clippy::cast_possible_truncation)]
            let r = ((col * 20) % 256) as u8;
            #[allow(clippy::cast_possible_truncation)]
            let g = ((row * 20) % 256) as u8;
            for py in 0..tile_h {
                for px in 0..tile_w {
                    let idx = ((row * tile_h + py) * img_w + col * tile_w + px) as usize * 4;
                    pixels[idx] = r;
                    pixels[idx + 1] = g;
                    pixels[idx + 2] = 0;
                    pixels[idx + 3] = 255; // fully opaque: exercises blit_sprite's fast path.
                }
            }
        }
    }
    let mut out = std::io::Cursor::new(Vec::new());
    let encoder = image::codecs::png::PngEncoder::new(&mut out);
    encoder
        .write_image(&pixels, img_w, img_h, image::ExtendedColorType::Rgba8)
        .unwrap();
    out.into_inner()
}

/// Builds a deterministic sprite-heavy frame: every cell holds a CP437
/// glyph present in the loaded tileset, so `draw_layers` takes the
/// `blit_sprite` path instead of `blit_glyph` for every cell.
#[cfg(feature = "tilesets")]
fn sprite_frame(cols: u16, rows: u16) -> Vec<(u8, Pos, Tile)> {
    let mut rng = fastrand::Rng::with_seed(42);
    // CP437 printable ASCII range (indices 32..=126) is guaranteed present in
    // the 256-tile sheet built by `make_sprite_sheet_png`.
    let glyphs: Vec<char> = (32u8..=126u8).map(char::from).collect();
    let mut out = Vec::with_capacity(usize::from(cols) * usize::from(rows));
    for y in 0..rows {
        for x in 0..cols {
            let glyph = glyphs[rng.usize(0..glyphs.len())];
            out.push((0, Pos::new(x, y), Tile::new(glyph, Style::new())));
        }
    }
    out
}

/// Registers the glyph-heavy `draw_layers` benchmark for one `(cols, rows, scale)`.
fn bench_glyph(c: &mut Criterion, cols: u16, rows: u16, scale: u8) {
    let mut group = c.benchmark_group(format!("raster/glyph/{cols}x{rows}@scale{scale}"));
    let frame = glyph_frame(cols, rows);
    let mut renderer = SoftwareBackendBuilder::new()
        .font(unscii16::FONT)
        .grid_size(cols, rows)
        .scale(scale)
        .build()
        .unwrap()
        .run_headless()
        .unwrap();

    group.bench_function("draw_layers", |b| {
        b.iter(|| {
            let content = frame
                .iter()
                .map(|(layer, pos, tile)| (*layer, *pos, tile, None));
            renderer.draw_layers(content).unwrap();
        });
    });
    group.finish();
}

/// Registers the sprite-heavy `draw_layers` benchmark for one `(cols, rows, scale)`.
#[cfg(feature = "tilesets")]
fn bench_sprite(c: &mut Criterion, cols: u16, rows: u16, scale: u8) {
    let mut group = c.benchmark_group(format!("raster/sprite/{cols}x{rows}@scale{scale}"));
    let frame = sprite_frame(cols, rows);
    let tileset = TilesetOptions::from_bytes(make_sprite_sheet_png())
        .tile_size(16, 16)
        .codepage(Codepage::Cp437)
        .build()
        .unwrap();
    let mut renderer = SoftwareBackendBuilder::new()
        .font(unscii16::FONT)
        .grid_size(cols, rows)
        .scale(scale)
        .tileset(tileset)
        .build()
        .unwrap()
        .run_headless()
        .unwrap();

    group.bench_function("draw_layers", |b| {
        b.iter(|| {
            let content = frame
                .iter()
                .map(|(layer, pos, tile)| (*layer, *pos, tile, None));
            renderer.draw_layers(content).unwrap();
        });
    });
    group.finish();
}

fn raster(c: &mut Criterion) {
    // 40x20: small viewport. 80x24: classic terminal default. 160x48: large
    // roguelike viewport. Scales 1/2/4 span the range `SoftwareBackendBuilder::scale`
    // actually gets used at in practice, and `scale` multiplies inner-loop work
    // quadratically, so 4x is worth measuring even though it's an extreme.
    for &(cols, rows) in &[(40u16, 20u16), (80, 24), (160, 48)] {
        for &scale in &[1u8, 2, 4] {
            bench_glyph(c, cols, rows, scale);
            #[cfg(feature = "tilesets")]
            bench_sprite(c, cols, rows, scale);
        }
    }
}

criterion_group!(benches, raster);
criterion_main!(benches);
