//! Benchmarks for the `Color` conversions on `retroglyph-core`'s per-frame paths: linear
//! blending (`Color::lerp`), the `gem` perceptual 256-color quantizer (`Color::to_indexed`), and
//! -- since `blend_color` itself is crate-private -- the non-linear `BlendMode`s exercised through
//! `Grid::blit_alpha`, the only public entry point that reaches them.
//!
//! retroglyph#269 asks for coverage of `blend_color` per `BlendMode` and the `gem` 256-color
//! quantizer. `blit.rs` already benchmarks `blit_alpha` over a large rect (its realistic calling
//! shape); this file instead isolates the per-color blend/quantize cost on a 1x1 grid so
//! `blit_alpha`'s per-cell loop overhead doesn't dominate the measurement, batching many calls
//! per iteration so setup doesn't dominate it either.

// `criterion_group!`/`criterion_main!` below expand to an undocumented `pub fn benches(..)`; this
// bench binary isn't a published API surface for `missing_docs` to usefully gate.
#![allow(missing_docs)]

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use retroglyph_core::{Color, Style, Tile};
use std::hint::black_box;

/// Number of colors quantized/blended per `b.iter()` call, batched so the RNG/vec setup below is
/// amortized rather than dominating each measured iteration.
const SAMPLES: usize = 4096;

/// Builds `SAMPLES` random RGB colors.
///
/// Uses a fixed RNG seed so the input (and therefore the benchmark) is deterministic across runs
/// -- required for `--save-baseline`/`--baseline` comparisons to be meaningful.
fn random_colors(seed: u64) -> Vec<Color> {
    let mut rng = fastrand::Rng::with_seed(seed);
    (0..SAMPLES)
        .map(|_| Color::Rgb {
            r: rng.u8(..),
            g: rng.u8(..),
            b: rng.u8(..),
        })
        .collect()
}

fn to_indexed(c: &mut Criterion) {
    let mut group = c.benchmark_group("color/to_indexed");
    group.throughput(Throughput::Elements(SAMPLES as u64));

    let colors = random_colors(1);
    // The `gem` 256-color quantizer (perceptual Oklab distance, see `Color::to_indexed`'s doc):
    // active whenever the `gem` feature is enabled, which is this crate's default.
    group.bench_function("to_indexed", |b| {
        b.iter(|| {
            for &color in &colors {
                black_box(color.to_indexed());
            }
        });
    });

    group.finish();
}

#[cfg(feature = "gem")]
fn lerp(c: &mut Criterion) {
    let mut group = c.benchmark_group("color/lerp");
    group.throughput(Throughput::Elements(SAMPLES as u64));

    let a = random_colors(1);
    let b_colors = random_colors(2);
    group.bench_function("lerp", |bencher| {
        bencher.iter(|| {
            for (&x, &y) in a.iter().zip(&b_colors) {
                black_box(Color::lerp(x, y, 0.5));
            }
        });
    });

    group.finish();
}

/// Benchmarks each non-`Linear` [`BlendMode`] via `Grid::blit_alpha` on a 1x1 grid, batching
/// `SAMPLES` calls per iteration -- see this file's module doc for why `blit_alpha` (rather than
/// `blend_color` directly) is the entry point used.
#[cfg(feature = "gem")]
fn blend_modes(c: &mut Criterion) {
    use retroglyph_core::{BlendMode, Grid};

    let mut group = c.benchmark_group("color/blend_color");
    group.throughput(Throughput::Elements(SAMPLES as u64));

    let src_colors = random_colors(1);
    let dst_colors = random_colors(2);

    for mode in [
        BlendMode::Linear,
        BlendMode::Screen,
        BlendMode::Dodge,
        BlendMode::Burn,
        BlendMode::Overlay,
    ] {
        group.bench_function(format!("{mode:?}"), |b| {
            let mut src = Grid::new(1, 1);
            let mut dst = Grid::new(1, 1);
            b.iter(|| {
                for (&sc, &dc) in src_colors.iter().zip(&dst_colors) {
                    src.put(0, 0, Tile::new('#', Style::new().fg(sc)));
                    dst.put(0, 0, Tile::new('#', Style::new().fg(dc)));
                    dst.blit_alpha(
                        0,
                        &src,
                        retroglyph_core::Rect::new(0, 0, 1, 1),
                        0,
                        0,
                        mode,
                        0.5,
                        0.5,
                    );
                    black_box(&dst);
                }
            });
        });
    }

    group.finish();
}

fn color(c: &mut Criterion) {
    to_indexed(c);
    #[cfg(feature = "gem")]
    {
        lerp(c);
        blend_modes(c);
    }
}

criterion_group!(benches, color);
criterion_main!(benches);
