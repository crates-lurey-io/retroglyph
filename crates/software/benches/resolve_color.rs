//! Benchmarks for per-cell color resolution (`resolve_color` and its
//! `ansi_to_rgb`/`indexed_to_rgb` palette lookups) in `retroglyph-software`
//! (retroglyph#307). `resolve_color` is cheap per call, but `draw_layers`
//! calls it (at least) twice per cell -- once for the background, once for
//! the foreground when the glyph isn't a space -- so it runs on every cell
//! of every frame.
//!
//! `resolve_color` is crate-private, so this bench (an external `benches/`
//! binary) drives it indirectly through `Output::draw_layers`: each
//! scenario below fills a grid with the same constant glyph (so the raster
//! path -- `blit_glyph`'s inner loop -- is identical across scenarios) and
//! varies only the [`Color`] variant used for both foreground and
//! background, isolating the color-resolution branch: `Color::Default` (a
//! constant lookup), `Color::Rgb` (already-packed, cheapest real case),
//! `Color::Ansi` (16-entry match in `ansi_to_rgb`), and `Color::Indexed`
//! (256-color cube/greyscale-ramp math in `indexed_to_rgb`).
//!
//! Requires `--all-features` (`default-font`; see the crate-level note in
//! `AGENTS.md`).

#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use retroglyph_core::{AnsiColor, Color, Output, Pos, Style, Tile};
use retroglyph_font::unscii16;
use retroglyph_software::SoftwareBackendBuilder;

const GRID: (u16, u16) = (80, 24);

fn frame(cols: u16, rows: u16, color: Color) -> Vec<(u8, Pos, Tile)> {
    let style = Style::new().fg(color).bg(color);
    let mut out = Vec::with_capacity(usize::from(cols) * usize::from(rows));
    for y in 0..rows {
        for x in 0..cols {
            out.push((0, Pos::new(x, y), Tile::new('@', style)));
        }
    }
    out
}

fn to_content(frame: &[(u8, Pos, Tile)]) -> impl Iterator<Item = (u8, Pos, &Tile, Option<&str>)> {
    frame
        .iter()
        .map(|(layer, pos, tile)| (*layer, *pos, tile, None))
}

fn resolve_color(c: &mut Criterion) {
    let mut group = c.benchmark_group(format!("resolve_color/{}x{}", GRID.0, GRID.1));

    let cases: &[(&str, Color)] = &[
        ("default", Color::Default),
        (
            "rgb",
            Color::Rgb {
                r: 12,
                g: 200,
                b: 44,
            },
        ),
        ("ansi", Color::Ansi(AnsiColor::BrightGreen)),
        ("indexed", Color::Indexed(200)),
    ];

    for &(name, color) in cases {
        let content = frame(GRID.0, GRID.1, color);
        let mut renderer = SoftwareBackendBuilder::new()
            .font(unscii16::FONT)
            .grid_size(GRID.0, GRID.1)
            .scale(1)
            .build()
            .unwrap()
            .run_headless()
            .unwrap();

        group.bench_function(name, |b| {
            b.iter(|| renderer.draw_layers(to_content(&content)).unwrap());
        });
    }

    group.finish();
}

criterion_group!(benches, resolve_color);
criterion_main!(benches);
