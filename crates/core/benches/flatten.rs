//! Benchmarks for layer compositing (`Grid::flatten_into`), the per-frame step
//! `Terminal::present` runs for every backend that doesn't composite layers itself (see
//! `crate::backend::Output::composites_layers`) -- currently `Headless` and `retroglyph-crossterm`.
//!
//! retroglyph#269 asks for coverage of this hot path across 1/4/16 layers: `flatten_into` walks
//! every allocated layer for every cell unconditionally (see `grid.rs`'s module doc, "No
//! short-circuiting"), so its cost scales linearly with layer count and this benchmark makes that
//! scaling visible instead of assumed.
//!
//! `flatten_into` itself is crate-private (only `Terminal::present` calls it), so this drives it
//! through that public entry point: a `Terminal<Headless>` (which never overrides
//! `composites_layers`, so it takes the flatten path) with `n` layers populated, calling
//! `present()` once per iteration. `present()` also diffs the flattened frame against the
//! previous one, but that diff cost is invariant across the layer counts compared here, while the
//! flatten cost is not -- so the scaling this benchmark reports is attributable to `flatten_into`.

// `criterion_group!`/`criterion_main!` below expand to an undocumented `pub fn benches(..)`; this
// bench binary isn't a published API surface for `missing_docs` to usefully gate.
#![allow(missing_docs)]

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use retroglyph_core::{Color, Headless, Style, Terminal, Tile};
use std::hint::black_box;

/// Builds a `cols x rows` terminal with `layers` layers populated (layer 0..layers-1), each
/// filled with a distinct glyph/color so `flatten_into`'s per-layer overwrite logic (glyph,
/// fg, offsets, flags, and conditionally bg) all do real work rather than short-circuiting.
///
/// Uses a fixed RNG seed so cell content (and therefore the benchmark) is deterministic across
/// runs -- required for `--save-baseline`/`--baseline` comparisons to be meaningful.
fn terminal_with_layers(cols: u16, rows: u16, layers: u8) -> Terminal<Headless> {
    let mut term = Terminal::new(Headless::new(cols, rows));
    let mut rng = fastrand::Rng::with_seed(42);
    for layer in 0..layers {
        let style = Style::new().fg(Color::Rgb {
            r: rng.u8(..),
            g: rng.u8(..),
            b: rng.u8(..),
        });
        let glyph = char::from_u32(u32::from(b'A') + u32::from(layer)).unwrap_or('?');
        for y in 0..rows {
            for x in 0..cols {
                term.grid_mut()
                    .put_tile(layer, x, y, Tile::new(glyph, style));
            }
        }
    }
    term
}

/// Registers one `present()` (flatten + diff) case per layer count, at a fixed grid size.
fn bench_layers(c: &mut Criterion, cols: u16, rows: u16) {
    let mut group = c.benchmark_group(format!("flatten_into/{cols}x{rows}"));

    for layers in [1u8, 4, 16] {
        // Cells touched by `flatten_into` per call: every allocated layer is visited for every
        // cell (see grid.rs's "No short-circuiting" doc), so this is the metric whose scaling
        // this benchmark exists to show.
        let cells_touched = u64::from(cols) * u64::from(rows) * u64::from(layers);
        group.throughput(Throughput::Elements(cells_touched));

        group.bench_function(format!("{layers}_layers"), |b| {
            let mut term = terminal_with_layers(cols, rows, layers);
            b.iter(|| black_box(term.present()));
        });
    }

    group.finish();
}

fn flatten(c: &mut Criterion) {
    // 80x24: the classic terminal default. 200x60: a large terminal / roguelike viewport.
    bench_layers(c, 80, 24);
    bench_layers(c, 200, 60);
}

criterion_group!(benches, flatten);
criterion_main!(benches);
