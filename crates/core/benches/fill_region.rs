//! Benchmarks for `Grid::fill_region` (the shared body of `Surface::clear`,
//! `Surface::clear_region`, and `Surface::fill_rect`) on a grid tiled with multi-cell spans.
//!
//! retroglyph#1020: `fill_region` used to call `clear_span_overlap` once per row, with the whole
//! row width, so a row with many spans intersecting it re-collected and re-scanned the same
//! anchors on every one of `rect.height()` calls, and a span several rows tall was fully reset
//! once per row it occupied. That is quadratic in a densely spanned grid, and "many spans, filled
//! every frame" is the span API's own intended use case (a sheet-driven tile map), not a
//! pathological input. This benchmark measures `fill_region` on exactly that grid shape: every
//! cell covered by a 2x2 span, filled whole every iteration, the way a game loop's
//! `Surface::clear()` would.
//!
//! Compares a sparse grid (no spans, the `has_spans` fast path) against the fully spanned one at
//! the same size, so the cost the span bookkeeping adds is visible on its own rather than folded
//! into `fill_region`'s baseline cost.

// `criterion_group!`/`criterion_main!` below expand to an undocumented `pub fn benches(..)`; this
// bench binary isn't a published API surface for `missing_docs` to usefully gate.
#![allow(missing_docs)]

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use retroglyph_core::{Grid, Rect, Style, Tile};
use std::hint::black_box;

/// Builds a `cols x rows` grid on layer 0, tiled edge-to-edge with 2x2 spans (clipping the last
/// span of a row/column if `cols`/`rows` is odd, same as any other span write would).
fn spanned_grid(cols: u16, rows: u16) -> Grid {
    let mut grid = Grid::new(cols, rows);
    let mut y = 0;
    while y < rows {
        let h = (rows - y).min(2);
        let mut x = 0;
        while x < cols {
            let w = (cols - x).min(2);
            grid.write_span_uniform(0, (x, y), (w, h), '#', '.', Style::default())
                .expect("span fits: w/h are clamped to the remaining grid extent");
            x += w;
        }
        y += h;
    }
    grid
}

/// Registers a `fill_region` case per grid, at a fixed grid size.
fn bench_fill_region(c: &mut Criterion, cols: u16, rows: u16) {
    let mut group = c.benchmark_group(format!("fill_region/{cols}x{rows}"));
    let rect = Rect::new(0, 0, cols, rows);
    let tile = Tile::new('#', Style::default());
    group.throughput(Throughput::Elements(u64::from(cols) * u64::from(rows)));

    group.bench_function("no_spans", |b| {
        let mut grid = Grid::new(cols, rows);
        b.iter(|| {
            grid.fill_region(0, rect, tile);
            black_box(());
        });
    });

    group.bench_function("fully_spanned_2x2", |b| {
        // `fill_region` overwrites every span it clears with plain tiles, so a grid reused across
        // iterations would have `has_spans` set but no spans left to scan past the first one.
        // `iter_batched` rebuilds a freshly spanned grid per iteration without that rebuild
        // counting toward the measured time.
        b.iter_batched(
            || spanned_grid(cols, rows),
            |mut grid| {
                grid.fill_region(0, rect, tile);
                black_box(());
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn fill_region(c: &mut Criterion) {
    // 80x24: the classic terminal default, and the grid retroglyph#1020's numbers are quoted
    // against. 200x60: a large terminal / roguelike viewport.
    bench_fill_region(c, 80, 24);
    bench_fill_region(c, 200, 60);
}

criterion_group!(benches, fill_region);
criterion_main!(benches);
