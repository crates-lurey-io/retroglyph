//! Benchmarks for `Grid::blit` and `Grid::blit_alpha`, the per-frame cell-copy paths widgets and
//! backends use to composite one grid's content into another (e.g. a rendered sub-view, a sprite
//! sheet cell, or a translucent overlay).
//!
//! retroglyph#269 asks for coverage of these over a large rect: both walk every cell in
//! `src_rect` unconditionally, and `blit_alpha` additionally runs a color blend per non-empty
//! cell (see [`BlendMode`]), so this benchmark reports cells-touched throughput alongside time to
//! make regressions in per-cell cost visible even if the loop itself doesn't change shape.

// `criterion_group!`/`criterion_main!` below expand to an undocumented `pub fn benches(..)`; this
// bench binary isn't a published API surface for `missing_docs` to usefully gate.
#![allow(missing_docs)]

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use retroglyph_core::{Color, Grid, Rect, Style, Tile};
use std::hint::black_box;

/// Builds a `cols x rows` grid with every cell on layer 0 set to a distinct RGB color, so
/// `blit_alpha`'s blend runs against varied (rather than uniform, easily-branch-predicted) input.
///
/// Uses a fixed RNG seed so the source content (and therefore the benchmark) is deterministic
/// across runs -- required for `--save-baseline`/`--baseline` comparisons to be meaningful.
fn filled(cols: u16, rows: u16, seed: u64) -> Grid {
    let mut rng = fastrand::Rng::with_seed(seed);
    let mut grid = Grid::new(cols, rows);
    for y in 0..rows {
        for x in 0..cols {
            let style = Style::new().fg(Color::Rgb {
                r: rng.u8(..),
                g: rng.u8(..),
                b: rng.u8(..),
            });
            grid.put(x, y, Tile::new('#', style));
        }
    }
    grid
}

fn bench_blit(c: &mut Criterion, cols: u16, rows: u16) {
    let mut group = c.benchmark_group(format!("blit/{cols}x{rows}"));
    let cells_touched = u64::from(cols) * u64::from(rows);
    group.throughput(Throughput::Elements(cells_touched));

    let src = filled(cols, rows, 1);
    let rect = Rect::new(0, 0, cols, rows);

    group.bench_function("blit", |b| {
        let mut dst = Grid::new(cols, rows);
        b.iter(|| {
            dst.blit(0, &src, rect, 0, 0);
            black_box(&dst);
        });
    });

    group.finish();
}

/// Registers a `blit_alpha` case per [`BlendMode`] variant, over the same large rect used by
/// `bench_blit` above, so the two are directly comparable.
#[cfg(feature = "gem")]
fn bench_blit_alpha(c: &mut Criterion, cols: u16, rows: u16) {
    use retroglyph_core::BlendMode;

    let mut group = c.benchmark_group(format!("blit_alpha/{cols}x{rows}"));
    let cells_touched = u64::from(cols) * u64::from(rows);
    group.throughput(Throughput::Elements(cells_touched));

    let src = filled(cols, rows, 1);
    // Distinct seed from `src` so every cell's blend mixes two different colors, not a color
    // with itself.
    let dst_seed = filled(cols, rows, 2);
    let rect = Rect::new(0, 0, cols, rows);

    for mode in [
        BlendMode::Linear,
        BlendMode::Screen,
        BlendMode::Dodge,
        BlendMode::Burn,
        BlendMode::Overlay,
    ] {
        group.bench_function(format!("{mode:?}"), |b| {
            b.iter_batched(
                || dst_seed.clone(),
                |mut dst| {
                    dst.blit_alpha(0, &src, rect, 0, 0, mode, 0.5, 0.5);
                    black_box(dst)
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

fn blit(c: &mut Criterion) {
    // 80x24: the classic terminal default. 200x60: a large terminal / roguelike viewport.
    bench_blit(c, 80, 24);
    bench_blit(c, 200, 60);
    #[cfg(feature = "gem")]
    {
        bench_blit_alpha(c, 80, 24);
        bench_blit_alpha(c, 200, 60);
    }
}

criterion_group!(benches, blit);
criterion_main!(benches);
