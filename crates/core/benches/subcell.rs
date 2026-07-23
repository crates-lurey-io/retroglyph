//! Benchmarks for the `subcell` posterizers (`quantize_half_block`/`quantize_quadrant`/
//! `quantize_sextant`), the per-cell hot path a pixel-to-text blitter (`doryen-rs`/libtcod-style
//! subcell rendering) would call once per output cell every frame.
//!
//! retroglyph#269 asks for coverage of all three quantizers. Each does an exhaustive search over
//! `2^N` foreground/background splits (see `subcell.rs`'s module doc, "Algorithm"), so cost grows
//! with the pixel count `N` (2 for half-block, 4 for quadrant, 6 for sextant) -- this benchmark
//! makes that growth visible, reporting blocks-quantized throughput alongside time.

// `criterion_group!`/`criterion_main!` below expand to an undocumented `pub fn benches(..)`; this
// bench binary isn't a published API surface for `missing_docs` to usefully gate.
#![allow(missing_docs)]

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use retroglyph_core::subcell::Rgb;
use retroglyph_core::{quantize_half_block, quantize_quadrant, quantize_sextant};
use std::hint::black_box;

/// Number of blocks quantized per `b.iter()` call, batched so the RNG/vec setup below is
/// amortized rather than dominating each measured iteration.
const BLOCKS: usize = 4096;

/// Builds `BLOCKS` random pixel blocks of `N` pixels each.
///
/// Uses a fixed RNG seed so the input (and therefore the benchmark) is deterministic across runs
/// -- required for `--save-baseline`/`--baseline` comparisons to be meaningful.
fn random_blocks<const N: usize>(seed: u64) -> Vec<[Rgb; N]> {
    let mut rng = fastrand::Rng::with_seed(seed);
    (0..BLOCKS)
        .map(|_| core::array::from_fn(|_| (rng.u8(..), rng.u8(..), rng.u8(..))))
        .collect()
}

fn subcell(c: &mut Criterion) {
    let mut group = c.benchmark_group("subcell");
    group.throughput(Throughput::Elements(BLOCKS as u64));

    let half_blocks: Vec<[Rgb; 2]> = random_blocks(1);
    group.bench_function("quantize_half_block", |b| {
        b.iter(|| {
            for &block in &half_blocks {
                black_box(quantize_half_block(block));
            }
        });
    });

    let quadrants: Vec<[Rgb; 4]> = random_blocks(2);
    group.bench_function("quantize_quadrant", |b| {
        b.iter(|| {
            for &block in &quadrants {
                black_box(quantize_quadrant(block));
            }
        });
    });

    let sextants: Vec<[Rgb; 6]> = random_blocks(3);
    group.bench_function("quantize_sextant", |b| {
        b.iter(|| {
            for &block in &sextants {
                black_box(quantize_sextant(block));
            }
        });
    });

    group.finish();
}

criterion_group!(benches, subcell);
criterion_main!(benches);
