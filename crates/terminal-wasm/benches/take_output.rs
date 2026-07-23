//! Benchmarks for [`TerminalWasm::take_output`], the per-frame call a browser terminal emulator
//! makes to pull freshly rendered ANSI bytes out of this backend (see `crates/terminal-wasm/js/
//! xterm-driver.js`, called once per animation frame).
//!
//! retroglyph#292 asks for numbers here specifically because `take_output`'s result crosses the
//! WASM/JS boundary as a marshalled string: the byte length of that string matters as much as the
//! CPU time spent building it, since a JS-side `TextDecoder`/string-copy cost scales with size too.
//! This benchmark reports both, via `Throughput::Bytes`, across a full-repaint frame (worst case:
//! every cell changed, as after a `clear` or a full-screen redraw) and a sparse-diff frame (typical
//! case: a small fraction of cells changed, as after a normal incremental update) at a few
//! representative grid sizes.
//!
//! Each iteration uses `iter_batched` to build a fresh [`Terminal<TerminalWasm>`] with the change
//! set already `present`ed but not yet drained, so the timed routine measures only the
//! `take_output` call itself (the drain), not the `present`/diff work that produced it -- that
//! diff cost is already covered by `crates/core/benches/grid_diff.rs`.

// See `crates/core/benches/grid_diff.rs` for why this bench binary is exempted from `missing_docs`.
#![allow(missing_docs)]

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use retroglyph_core::Terminal;
use retroglyph_terminal_wasm::TerminalWasm;
use std::hint::black_box;

/// Builds a `cols x rows` terminal, fills every cell with a space, presents it, and drains the
/// resulting full-paint output so the backend starts each benchmark from a known, empty-output
/// baseline.
fn baseline(cols: u16, rows: u16) -> Terminal<TerminalWasm> {
    let backend = TerminalWasm::new(cols, rows);
    let mut term = Terminal::new(backend);
    for y in 0..rows {
        for x in 0..cols {
            term.put(x, y, ' ');
        }
    }
    term.present()
        .expect("in-memory TerminalWasm backend never fails to present");
    let _ = term.backend_mut().take_output();
    term
}

/// Builds a terminal with every cell changed and `present`ed, but not yet drained -- the worst
/// case for `take_output`: every cell emits a cursor move plus a glyph.
fn full_repaint_pending(cols: u16, rows: u16) -> Terminal<TerminalWasm> {
    let mut term = baseline(cols, rows);
    for y in 0..rows {
        for x in 0..cols {
            term.put(x, y, 'X');
        }
    }
    term.present()
        .expect("in-memory TerminalWasm backend never fails to present");
    term
}

/// Builds a terminal with `pct` percent of its cells changed and `present`ed, but not yet
/// drained -- the typical case: a small, scattered edit.
///
/// Uses a fixed RNG seed so the change set is deterministic across runs, matching
/// `crates/core/benches/grid_diff.rs`'s `sparse_pair`.
fn sparse_diff_pending(cols: u16, rows: u16, pct: u32) -> Terminal<TerminalWasm> {
    let mut term = baseline(cols, rows);

    let total = u32::from(cols) * u32::from(rows);
    let changes = total * pct / 100;

    let mut rng = fastrand::Rng::with_seed(42);
    for _ in 0..changes {
        let x = rng.u16(0..cols);
        let y = rng.u16(0..rows);
        term.put(x, y, 'X');
    }
    term.present()
        .expect("in-memory TerminalWasm backend never fails to present");
    term
}

/// Registers the `full_repaint` / `sparse_*pct` cases for one grid size, each reporting both
/// wall-clock time and `Throughput::Bytes` for the drained output.
fn bench_size(c: &mut Criterion, cols: u16, rows: u16) {
    let mut group = c.benchmark_group(format!("take_output/{cols}x{rows}"));

    let bytes = full_repaint_pending(cols, rows)
        .backend_mut()
        .take_output()
        .len() as u64;
    group.throughput(Throughput::Bytes(bytes));
    group.bench_function("full_repaint", |b| {
        b.iter_batched(
            || full_repaint_pending(cols, rows),
            |mut term| black_box(term.backend_mut().take_output()),
            BatchSize::SmallInput,
        );
    });

    for pct in [1, 5] {
        let bytes = sparse_diff_pending(cols, rows, pct)
            .backend_mut()
            .take_output()
            .len() as u64;
        group.throughput(Throughput::Bytes(bytes));
        group.bench_function(format!("sparse_{pct}pct"), |b| {
            b.iter_batched(
                || sparse_diff_pending(cols, rows, pct),
                |mut term| black_box(term.backend_mut().take_output()),
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

fn take_output(c: &mut Criterion) {
    // 40x12: a small embedded widget. 80x24: the classic terminal default. 200x60: a large
    // terminal / roguelike viewport (matching `grid_diff.rs`'s larger size).
    bench_size(c, 40, 12);
    bench_size(c, 80, 24);
    bench_size(c, 200, 60);
}

criterion_group!(benches, take_output);
criterion_main!(benches);
