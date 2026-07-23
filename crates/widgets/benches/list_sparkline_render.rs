//! Benchmarks for [`List::render`] over long lists and [`Sparkline::render`]'s per-cell
//! [`Meter`] color ramp.
//!
//! retroglyph#316 calls out both as non-trivial per-frame costs worth a number: `List::render`
//! shares `Table`'s truncation/windowing path over a single column, and `Sparkline::render` does
//! one `Meter::color` (a `gem`-backed lerp) per visible cell every frame, which is easy to
//! overlook as "just a bar chart".
//!
//! `--test` runs each benchmark once (a compile/smoke check); a real run is
//! `cargo bench -p retroglyph-widgets --bench list_sparkline_render`.

// `criterion_group!`/`criterion_main!` below expand to an undocumented `pub fn benches(..)`; this
// bench binary isn't a published API surface for `missing_docs` to usefully gate.
#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use retroglyph_core::{Headless, Rect, Terminal};
use retroglyph_widgets::{List, ListState, Sparkline, StatefulWidget, Widget};
use std::hint::black_box;

/// Builds `n` deterministic, variable-length item strings -- long enough that some need
/// truncation against the render area's width.
fn items(n: usize) -> Vec<String> {
    let mut rng = fastrand::Rng::with_seed(42);
    (0..n)
        .map(|i| format!("item-{i}-{}", "x".repeat(rng.usize(0..40))))
        .collect()
}

fn bench_list(c: &mut Criterion, n: usize) {
    let owned = items(n);
    let refs: Vec<&str> = owned.iter().map(String::as_str).collect();
    let area = Rect::new(0, 0, 40, 24);

    let mut term = Terminal::new(Headless::new(area.width(), area.height()));
    let mut state = ListState::new();
    state.select(Some(n / 2));
    state.ensure_visible(area.height_usize());

    c.bench_function(&format!("list_render/{n}_items"), |b| {
        b.iter(|| {
            let list = List::new(black_box(&refs));
            list.render(area, &mut term, &mut state);
        });
    });
}

/// Builds `n` deterministic samples in `[0, 1)`, matching a realistic normalized-metric
/// sparkline feed (CPU/FPS/latency ratios), not raw unbounded magnitudes.
fn samples(n: usize) -> Vec<f32> {
    let mut rng = fastrand::Rng::with_seed(42);
    (0..n).map(|_| rng.f32()).collect()
}

fn bench_sparkline(c: &mut Criterion, width: u16, sample_count: usize) {
    let data = samples(sample_count);
    let area = Rect::new(0, 0, width, 1);
    let mut term = Terminal::new(Headless::new(area.width(), area.height()));

    c.bench_function(
        &format!("sparkline_render/{width}w_{sample_count}samples"),
        |b| {
            b.iter(|| {
                let spark = Sparkline::new(black_box(&data));
                spark.render(area, &mut term);
            });
        },
    );
}

fn list_sparkline_render(c: &mut Criterion) {
    bench_list(c, 200);
    bench_list(c, 10_000);

    // A typical terminal-width sparkline fed by a short recent-history buffer, and a much longer
    // sample history that still only ever renders its last `width` cells (exercising the
    // "slice off the tail" cost as history grows independent of what's drawn).
    bench_sparkline(c, 80, 200);
    bench_sparkline(c, 80, 50_000);
}

criterion_group!(benches, list_sparkline_render);
criterion_main!(benches);
