//! Benchmarks for [`TerminalRenderer::draw`], the per-frame hot path both the crossterm and
//! terminal-wasm backends call to turn a [`Grid::diff`] into ANSI/SGR bytes.
//!
//! retroglyph#275 asks for numbers on this path before further optimizing it: full-repaint vs
//! sparse-diff cost (since `draw` only walks *changed* cells, a sparse diff should be
//! proportionally cheaper), the cost of fg/bg "SGR churn" (every cell forcing a fresh color
//! escape vs a frame that coalesces to one color for the whole draw), and plain-mode vs
//! escape-mode output on the same frame (plain mode strips all ANSI/CSI codes -- see
//! [`TerminalRenderer::set_plain_mode`]). Bytes emitted are reported alongside wall time via
//! `Throughput::Bytes`: for `retroglyph-terminal-wasm` the string size pulled into JS each frame
//! matters as much as CPU, so time-only numbers would miss half of what this issue asks for.
//!
//! All benchmarks measure a full render (build the output, don't inspect it) at 200x50, "a large
//! terminal / roguelike viewport" per `grid_diff`'s bench sizing -- large enough that per-cell
//! escape-sequence overhead dominates over fixed setup cost.

// `criterion_group!`/`criterion_main!` below expand to an undocumented `pub fn benches(..)`; this
// bench binary isn't a published API surface for `missing_docs` to usefully gate.
#![allow(missing_docs)]

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use retroglyph_core::{Color, Grid, Style, Tile};
use retroglyph_terminal::TerminalRenderer;
use std::hint::black_box;

const COLS: u16 = 200;
const ROWS: u16 = 50;

/// Builds a `cols x rows` grid with every cell set to `glyph`/`style`.
fn filled(cols: u16, rows: u16, glyph: char, style: Style) -> Grid {
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
/// runs -- required for `--save-baseline`/`--baseline` comparisons to be meaningful. Mirrors
/// `retroglyph-core`'s `grid_diff` bench helper of the same name.
fn sparse_pair(cols: u16, rows: u16, pct: u32) -> (Grid, Grid) {
    let old = filled(cols, rows, ' ', Style::default());
    let mut new = filled(cols, rows, ' ', Style::default());
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

/// Builds a full-grid checkerboard where every cell's fg/bg differs from its neighbor, so a
/// diff-based renderer can never coalesce consecutive cells onto the same tracked SGR state.
fn checkerboard(cols: u16, rows: u16) -> Grid {
    let style_a = Style::new()
        .fg(Color::Rgb { r: 255, g: 0, b: 0 })
        .bg(Color::Rgb { r: 0, g: 0, b: 255 });
    let style_b = Style::new()
        .fg(Color::Rgb { r: 0, g: 255, b: 0 })
        .bg(Color::Rgb {
            r: 255,
            g: 255,
            b: 0,
        });
    let mut grid = Grid::new(cols, rows);
    for y in 0..rows {
        for x in 0..cols {
            let style = if (x + y) % 2 == 0 { style_a } else { style_b };
            grid.put(x, y, Tile::new('#', style));
        }
    }
    grid
}

/// Renders `old.diff(new)` through a [`TerminalRenderer`] and returns the emitted bytes.
fn render_diff(old: &Grid, new: &Grid, plain: bool) -> Vec<u8> {
    let mut renderer = TerminalRenderer::with_plain_mode(Vec::new(), plain);
    renderer
        .draw(
            old.diff(new)
                .map(|(_, pos, tile, extra)| (pos, tile, extra)),
        )
        .expect("Vec<u8> writes never fail");
    renderer.flush().expect("Vec<u8> flush never fails");
    renderer.into_writer()
}

/// Full-repaint vs sparse-diff (1% changed) at 200x50: `draw` only walks changed cells, so this
/// should scale down roughly with the change density.
fn bench_repaint_vs_sparse(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame_render/full_vs_sparse_200x50");

    let old = filled(COLS, ROWS, ' ', Style::default());
    let full_new = filled(
        COLS,
        ROWS,
        'X',
        Style::new().fg(Color::Rgb {
            r: 255,
            g: 255,
            b: 255,
        }),
    );
    let bytes = render_diff(&old, &full_new, false).len() as u64;
    group.throughput(Throughput::Bytes(bytes));
    group.bench_function("full_repaint", |b| {
        b.iter(|| black_box(render_diff(&old, &full_new, false)));
    });

    let (sparse_old, sparse_new) = sparse_pair(COLS, ROWS, 1);
    let bytes = render_diff(&sparse_old, &sparse_new, false).len() as u64;
    group.throughput(Throughput::Bytes(bytes));
    group.bench_function("sparse_1pct", |b| {
        b.iter(|| black_box(render_diff(&sparse_old, &sparse_new, false)));
    });

    group.finish();
}

/// SGR-churn frame (fg/bg alternating every cell) vs a single-color frame of the same size, to
/// quantify the fg/bg-coalescing win and guard against SGR-state-tracking regressions.
fn bench_sgr_churn(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame_render/sgr_churn_200x50");
    let old = filled(COLS, ROWS, ' ', Style::default());

    let churn = checkerboard(COLS, ROWS);
    let bytes = render_diff(&old, &churn, false).len() as u64;
    group.throughput(Throughput::Bytes(bytes));
    group.bench_function("alternating_fg_bg", |b| {
        b.iter(|| black_box(render_diff(&old, &churn, false)));
    });

    let uniform_style = Style::new()
        .fg(Color::Rgb { r: 255, g: 0, b: 0 })
        .bg(Color::Rgb { r: 0, g: 0, b: 255 });
    let coalesced = filled(COLS, ROWS, '#', uniform_style);
    let bytes = render_diff(&old, &coalesced, false).len() as u64;
    group.throughput(Throughput::Bytes(bytes));
    group.bench_function("uniform_color", |b| {
        b.iter(|| black_box(render_diff(&old, &coalesced, false)));
    });

    group.finish();
}

/// Plain mode vs escape mode on the same full-repaint frame: plain mode strips all ANSI/CSI
/// escapes (see [`TerminalRenderer::set_plain_mode`]), so it should be both faster and emit far
/// fewer bytes.
fn bench_plain_vs_escape(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame_render/plain_vs_escape_200x50");
    let old = filled(COLS, ROWS, ' ', Style::default());
    let new = filled(
        COLS,
        ROWS,
        'X',
        Style::new().fg(Color::Rgb {
            r: 255,
            g: 255,
            b: 255,
        }),
    );

    let bytes = render_diff(&old, &new, false).len() as u64;
    group.throughput(Throughput::Bytes(bytes));
    group.bench_function("escape_mode", |b| {
        b.iter(|| black_box(render_diff(&old, &new, false)));
    });

    let bytes = render_diff(&old, &new, true).len() as u64;
    group.throughput(Throughput::Bytes(bytes));
    group.bench_function("plain_mode", |b| {
        b.iter(|| black_box(render_diff(&old, &new, true)));
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_repaint_vs_sparse,
    bench_sgr_churn,
    bench_plain_vs_escape
);
criterion_main!(benches);
