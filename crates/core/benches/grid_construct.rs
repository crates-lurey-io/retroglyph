//! Benchmarks for `Grid::new`, measuring per-construction cost.
//!
//! retroglyph#264 flags that `Grid::new` pre-allocates a 256-element `Vec<Option<LayerBuf>>` up
//! front, and that `Terminal` holds up to four `Grid`s (`current`, `previous`,
//! `flattened_current`, `flattened_previous`), multiplying that cost. This benchmark measures the
//! cost of constructing a single `Grid` -- the dominant cost is the layer-table `Vec` itself, not
//! the grid content, so a range of grid sizes is included to confirm the construction cost is flat
//! with respect to `width`/`height` (only layer 0's `LayerBuf` scales with cell count; the other
//! 255 slots are a fixed-size `Vec` regardless of grid dimensions).
//!
//! Per the issue's explicit "measure before changing" instruction: run this benchmark before and
//! after any change to the layer-table allocation strategy and compare `grid_new/*` timings.

// `criterion_group!`/`criterion_main!` below expand to an undocumented `pub fn benches(..)`; this
// bench binary isn't a published API surface for `missing_docs` to usefully gate.
#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use retroglyph_core::Grid;
use std::hint::black_box;

/// Registers `Grid::new` construction benchmarks across representative grid sizes.
fn grid_new(c: &mut Criterion) {
    let mut group = c.benchmark_group("grid_new");

    // 80x24: the classic terminal default. 200x60: a large terminal / roguelike viewport.
    for (cols, rows) in [(80u16, 24u16), (200, 60)] {
        group.bench_function(format!("{cols}x{rows}"), |b| {
            b.iter(|| black_box(Grid::new(cols, rows)));
        });
    }

    // Simulates `Terminal`'s four `Grid`s (`current`, `previous`, `flattened_current`,
    // `flattened_previous`) constructed back-to-back, per retroglyph#264's framing.
    group.bench_function("terminal_4x_80x24", |b| {
        b.iter(|| {
            black_box((
                Grid::new(80, 24),
                Grid::new(80, 24),
                Grid::new(80, 24),
                Grid::new(80, 24),
            ))
        });
    });

    group.finish();
}

criterion_group!(benches, grid_new);
criterion_main!(benches);
