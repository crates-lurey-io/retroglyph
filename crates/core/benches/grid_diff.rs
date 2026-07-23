//! Benchmarks for `Grid::diff`, the operation `TerminalRenderer::take_output`
//! (`crates/terminal-wasm`) walks every frame to build its ANSI output.
//!
//! retroglyph#109 proposes exposing an incremental "diff since last output" API instead of
//! re-serializing the whole grid every frame, but flags itself as possibly premature and asks for
//! numbers before committing to the API change. This benchmark supplies those numbers: it measures
//! `Grid::diff` across a range of change densities (nothing changed, a sparse edit, a full repaint)
//! at two representative grid sizes, so that tradeoff can be judged against real timings instead of
//! intuition.

// `criterion_group!`/`criterion_main!` below expand to an undocumented `pub fn benches(..)`; this
// bench binary isn't a published API surface for `missing_docs` to usefully gate.
#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use retroglyph_core::{Color, Grid, Style, Tile};
use std::hint::black_box;

/// Builds a `cols x rows` grid with every cell set to `glyph`/`fg`.
fn filled(cols: u16, rows: u16, glyph: char, fg: Color) -> Grid {
    let style = Style::new().fg(fg);
    let mut grid = Grid::new(cols, rows);
    for y in 0..rows {
        for x in 0..cols {
            grid.put(x, y, Tile::new(glyph, style));
        }
    }
    grid
}

/// Builds two same-sized grids differing in exactly `pct` percent of their cells.
///
/// Uses a fixed RNG seed so the change set (and therefore the benchmark) is deterministic across
/// runs -- required for `--save-baseline`/`--baseline` comparisons to be meaningful.
fn sparse_pair(cols: u16, rows: u16, pct: u32) -> (Grid, Grid) {
    let old = filled(cols, rows, ' ', Color::Default);
    let mut new = filled(cols, rows, ' ', Color::Default);
    let changed_style = Style::new().fg(Color::Rgb { r: 255, g: 0, b: 0 });

    let total = u32::from(cols) * u32::from(rows);
    let changes = total * pct / 100;

    let mut rng = fastrand::Rng::with_seed(42);
    for _ in 0..changes {
        let x = rng.u16(0..cols);
        let y = rng.u16(0..rows);
        new.put(x, y, Tile::new('X', changed_style));
    }
    (old, new)
}

/// Registers the `no_changes` / `sparse_*pct` / `full_repaint` cases for one grid size.
fn bench_size(c: &mut Criterion, cols: u16, rows: u16) {
    let mut group = c.benchmark_group(format!("grid_diff/{cols}x{rows}"));

    group.bench_function("no_changes", |b| {
        let old = filled(cols, rows, ' ', Color::Default);
        let new = filled(cols, rows, ' ', Color::Default);
        b.iter(|| black_box(old.diff(&new).count()));
    });

    for pct in [1, 5, 25] {
        let (old, new) = sparse_pair(cols, rows, pct);
        group.bench_function(format!("sparse_{pct}pct"), |b| {
            b.iter(|| black_box(old.diff(&new).count()));
        });
    }

    group.bench_function("full_repaint", |b| {
        let old = filled(cols, rows, ' ', Color::Default);
        let new = filled(
            cols,
            rows,
            'X',
            Color::Rgb {
                r: 255,
                g: 255,
                b: 255,
            },
        );
        b.iter(|| black_box(old.diff(&new).count()));
    });

    group.finish();
}

fn grid_diff(c: &mut Criterion) {
    // 80x24: the classic terminal default. 200x60: a large terminal / roguelike viewport.
    bench_size(c, 80, 24);
    bench_size(c, 200, 60);
}

criterion_group!(benches, grid_diff);
criterion_main!(benches);
