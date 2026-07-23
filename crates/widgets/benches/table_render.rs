//! Benchmarks for [`Table::render`] at realistic row/column counts.
//!
//! retroglyph#316 flags `Table::render` as the highest-value widget benchmark in the crate: it
//! exercises per-row truncation (`text::truncate`) and column-by-column formatting -- per-frame
//! traffic that's easy to regress silently without a number to compare against. This benchmark
//! measures full-table renders at a handful of representative sizes (a compact status table, a
//! wide multi-column dashboard, a long scrolling log-like table) so future changes to the
//! truncation path have something concrete to check against.
//!
//! `--test` runs each benchmark once (a compile/smoke check); a real run is
//! `cargo bench -p retroglyph-widgets --bench table_render`.

// `criterion_group!`/`criterion_main!` below expand to an undocumented `pub fn benches(..)`; this
// bench binary isn't a published API surface for `missing_docs` to usefully gate. The cast lints
// mirror the crate-level allow in `src/lib.rs`: bench sizes are small, hand-picked constants,
// never large enough to actually truncate `usize -> u16`.
#![allow(missing_docs, clippy::cast_possible_truncation)]

use criterion::{Criterion, criterion_group, criterion_main};
use retroglyph_core::{Headless, Rect, Terminal};
use retroglyph_widgets::{ListState, StatefulWidget, Table};
use std::hint::black_box;

/// Builds `rows x cols` of short, deterministic cell text -- long enough that some cells need
/// truncation against the column widths used below, short enough to look like realistic table
/// data (not a worst-case stress string).
fn rows(rows: usize, cols: usize) -> Vec<Vec<String>> {
    let mut rng = fastrand::Rng::with_seed(42);
    (0..rows)
        .map(|r| {
            (0..cols)
                .map(|c| format!("row{r}-col{c}-{:04x}", rng.u32(0..0xffff)))
                .collect()
        })
        .collect()
}

/// Registers one `rows x cols` case at a fixed terminal size, with a mid-list selection so the
/// highlighted-row path is also exercised every iteration.
fn bench_size(c: &mut Criterion, rows_n: usize, cols_n: usize) {
    let owned = rows(rows_n, cols_n);
    let cell_rows: Vec<Vec<&str>> = owned
        .iter()
        .map(|row| row.iter().map(String::as_str).collect())
        .collect();
    let data: Vec<&[&str]> = cell_rows.iter().map(Vec::as_slice).collect();
    let headers: Vec<&str> = (0..cols_n).map(|_| "Header").collect();
    let widths: Vec<u16> = (0..cols_n).map(|_| 12u16).collect();
    let area = Rect::new(0, 0, (cols_n * 13) as u16, 24);

    let mut term = Terminal::new(Headless::new(area.width(), area.height()));
    let mut state = ListState::new();
    state.select(Some(rows_n / 2));
    state.ensure_visible(area.height_usize().saturating_sub(1));

    c.bench_function(&format!("table_render/{rows_n}x{cols_n}"), |b| {
        b.iter(|| {
            let table = Table::new(black_box(&headers), black_box(&widths), black_box(&data));
            table.render(area, &mut term, &mut state);
        });
    });
}

fn table_render(c: &mut Criterion) {
    // A compact status table, a wide dashboard, and a long scrolling table -- the viewport
    // (24 rows) only ever shows a slice, so `rows_n` past the viewport also exercises the
    // `visible_window` scroll-offset path, not just raw per-row formatting cost.
    bench_size(c, 20, 4);
    bench_size(c, 20, 12);
    bench_size(c, 5_000, 6);
}

criterion_group!(benches, table_render);
criterion_main!(benches);
