//! Benchmarks for `winit::translate`'s pixel/cell conversion helpers
//! (retroglyph#299): `on_cursor_moved`/`on_mouse_input`/`on_mouse_wheel` (`run.rs`) call
//! `pixel_to_cell` and `physical_pos_from` on every pointer event, so both are on the same hot
//! path the #294 cursor-moved coalescing fix targets.

#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use retroglyph_window::winit::translate::{physical_pos_from, pixel_to_cell};
use std::hint::black_box;

/// A deterministic stream of pointer positions, seeded so results are stable across runs.
///
/// Values range over a few virtual 4K-ish monitors' worth of physical pixels, including some
/// negative jitter (the cursor briefly outside the window reports negative coordinates) to
/// exercise the `.max(0.0)` clamp both functions share.
fn position_stream(len: usize) -> Vec<(f64, f64)> {
    let mut rng = fastrand::Rng::with_seed(7);
    (0..len)
        .map(|_| {
            let x = rng.f64().mul_add(4000.0, -50.0);
            let y = rng.f64().mul_add(3000.0, -50.0);
            (x, y)
        })
        .collect()
}

fn bench_pixel_to_cell(c: &mut Criterion) {
    let positions = position_stream(10_000);
    c.bench_function("pixel_to_cell/10k_positions", |b| {
        b.iter(|| {
            for &(x, y) in &positions {
                black_box(pixel_to_cell(black_box(x), black_box(y), 8, 16));
            }
        });
    });
}

fn bench_physical_pos_from(c: &mut Criterion) {
    let positions = position_stream(10_000);
    c.bench_function("physical_pos_from/10k_positions", |b| {
        b.iter(|| {
            for &(x, y) in &positions {
                black_box(physical_pos_from(black_box(x), black_box(y)));
            }
        });
    });
}

criterion_group!(benches, bench_pixel_to_cell, bench_physical_pos_from);
criterion_main!(benches);
