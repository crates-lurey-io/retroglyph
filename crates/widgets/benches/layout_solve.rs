//! Benchmarks for [`split_h`]/[`split_v`] (the [`Constraint`] solver) with mixed constraint
//! kinds.
//!
//! retroglyph#316 lists `layout::solve`/`split_h`/`split_v` as worth benchmarking: every
//! [`Constraint::Fill`]/[`Constraint::Min`]/[`Constraint::Max`] pane pulls in the
//! largest-remainder distribution pass (sorting by fractional remainder), so a layout mixing all
//! five constraint kinds does meaningfully more work per call than an all-`Fixed` split. `solve`
//! itself is a private helper, so this benchmarks it through its public callers, [`split_h`] and
//! [`split_v`], which is also what every real caller in an app actually pays for once per frame,
//! per split.
//!
//! `--test` runs each benchmark once (a compile/smoke check); a real run is
//! `cargo bench -p retroglyph-widgets --bench layout_solve`.

// `criterion_group!`/`criterion_main!` below expand to an undocumented `pub fn benches(..)`; this
// bench binary isn't a published API surface for `missing_docs` to usefully gate.
#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use retroglyph_core::Rect;
use retroglyph_widgets::{Constraint, split_h, split_v};
use std::hint::black_box;

/// A small, fixed-shape mix of every [`Constraint`] kind, repeated `panes` times, so the
/// remainder-distribution pass has a realistic multi-pane fan-out to chew through (a dashboard
/// with a header, several flexible content panes, and a status bar, not just two or three
/// panes).
fn mixed_constraints(panes: usize) -> Vec<Constraint> {
    const PATTERN: [Constraint; 5] = [
        Constraint::Fixed(3),
        Constraint::Percent(10),
        Constraint::Fill(2),
        Constraint::Min(4),
        Constraint::Max(20),
    ];
    PATTERN.iter().copied().cycle().take(panes).collect()
}

fn layout_solve(c: &mut Criterion) {
    let mut group = c.benchmark_group("layout_solve");

    for &panes in &[5usize, 25, 100] {
        let constraints = mixed_constraints(panes);

        let area_h = Rect::new(0, 0, 300, 80);
        group.bench_function(format!("split_h/{panes}_panes"), |b| {
            b.iter(|| black_box(split_h(area_h, &constraints)));
        });

        let area_v = Rect::new(0, 0, 80, 300);
        group.bench_function(format!("split_v/{panes}_panes"), |b| {
            b.iter(|| black_box(split_v(area_v, &constraints)));
        });
    }

    group.finish();
}

criterion_group!(benches, layout_solve);
criterion_main!(benches);
