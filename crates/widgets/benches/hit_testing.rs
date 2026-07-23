//! Benchmarks for [`HitTester`]/[`Interaction`] with many registered widgets.
//!
//! retroglyph#316 asks for this to "keep the no-retained-tree hit test cheap as widget counts
//! grow": [`HitTester::topmost_at`] scans its registrations back-to-front with a linear `rev().
//! find()`, and a full [`Interaction`] frame additionally re-registers every widget's rect (and,
//! for focusable widgets, its position in the [`FocusRing`](retroglyph_widgets::FocusRing)) once
//! per [`Interaction::interact`] call. Both are deliberately O(n) by design (see `HitTester`'s
//! doc comment on why a plain draw-ordered `Vec` beats a spatial index at typical per-frame
//! widget counts), but "deliberately O(n)" still needs a number: this benchmarks
//! [`HitTester::topmost_at`] directly, and a full `Interaction` frame (register + resolve every
//! widget), across a range of registered-widget counts from a small dialog up through a long
//! virtualized list's worth of rows.
//!
//! `--test` runs each benchmark once (a compile/smoke check); a real run is
//! `cargo bench -p retroglyph-widgets --bench hit_testing`.

// `criterion_group!`/`criterion_main!` below expand to an undocumented `pub fn benches(..)`; this
// bench binary isn't a published API surface for `missing_docs` to usefully gate. The cast lint
// mirrors the crate-level allow in `src/lib.rs`: bench widget counts are small, hand-picked
// constants, never large enough to actually truncate `usize -> u16`.
#![allow(missing_docs, clippy::cast_possible_truncation)]

use criterion::{Criterion, criterion_group, criterion_main};
use retroglyph_core::{Pos, Rect};
use retroglyph_widgets::{HitTester, Interaction, Sense};
use std::hint::black_box;

/// Builds a [`HitTester`] with `n` non-overlapping 10x1 rects stacked vertically, ids `0..n`.
fn populated_hit_tester(n: usize) -> HitTester<usize> {
    let mut hits = HitTester::new();
    for i in 0..n {
        hits.push(Rect::new(0, i as u16, 10, 1), i);
    }
    hits
}

fn bench_hit_tester(c: &mut Criterion, n: usize) {
    let hits = populated_hit_tester(n);
    // A miss (past every registered rect) forces `topmost_at`'s backward scan to walk every
    // entry -- the worst case for the linear scan, and the one that scales with `n`.
    let miss_pos = Pos::new(0, n as u16 + 10);

    c.bench_function(&format!("hit_tester_topmost_at_miss/{n}_rects"), |b| {
        b.iter(|| black_box(hits.topmost_at(black_box(miss_pos))));
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct WidgetId(usize);

fn bench_interaction_frame(c: &mut Criterion, n: usize) {
    let mut interaction = Interaction::<WidgetId>::new();
    let sense = Sense::HOVER | Sense::CLICK;

    c.bench_function(&format!("interaction_frame/{n}_widgets"), |b| {
        b.iter(|| {
            interaction.begin_frame();
            for i in 0..n {
                let rect = Rect::new(0, i as u16, 10, 1);
                black_box(interaction.interact(rect, WidgetId(i), sense));
            }
            interaction.end_frame();
        });
    });
}

fn hit_testing(c: &mut Criterion) {
    for &n in &[10usize, 100, 1_000] {
        bench_hit_tester(c, n);
        bench_interaction_frame(c, n);
    }
}

criterion_group!(benches, hit_testing);
criterion_main!(benches);
