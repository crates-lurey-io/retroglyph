//! Benchmarks for `SoftwareRenderer`'s damage tracking (`update_damage`) and
//! the end-to-end `draw_layers` + `present` cost at different change
//! densities (retroglyph#307).
//!
//! `update_damage` and `blit_glyph`/`blit_sprite` are crate-private, so this
//! bench (an external `benches/` binary, which only sees the crate's public
//! API) cannot call `update_damage` directly. It isolates the diff cost
//! anyway by exploiting a property of `draw_layers`: it always redraws every
//! cell (`needs_full_frame() == true`), so the *rasterization* cost of two
//! frames with the same glyphs-per-cell count is identical regardless of how
//! many of those cells actually changed color from the previous frame. Using
//! `Criterion::iter_batched` to reset to a known baseline frame before each
//! timed call, then timing a transition to "no change" / "one cell changed" /
//! "every cell changed", isolates exactly the part of the per-call cost that
//! *does* vary with change density: `update_damage`'s row-by-row diff (and,
//! for the fully-changed case, its `copy_from_slice` of the whole buffer).
//!
//! `present()` is a documented no-op in headless mode (no window surface, see
//! `SoftwareRenderer::present`), so the "end to end" group below cannot
//! exercise the real pixel-upload cost -- that requires a live window, which
//! criterion cannot drive. It still benchmarks `draw_layers` + `present`
//! together for parity with how frames are actually driven, with the
//! understanding that `present`'s own contribution is ~0 here; the numbers
//! are dominated by `draw_layers` (raster + diff).
//!
//! Requires `--all-features` (`default-font`; see the crate-level note in
//! `AGENTS.md`).

#![allow(missing_docs)]

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use retroglyph_core::{Color, Output, Pos, Style, Tile};
use retroglyph_software::SoftwareBackendBuilder;
use retroglyph_software::bitmap_font::unscii16;
use std::cell::RefCell;

/// Builds a `cols x rows` frame with every layer-0 cell set to the same
/// background color, with cell `(ox, oy)` overridden (if given) to a
/// different color -- used to construct "no change", "one cell changed", and
/// "all cells changed" frames relative to a shared baseline.
fn frame(
    cols: u16,
    rows: u16,
    base: Color,
    over: Option<((u16, u16), Color)>,
) -> Vec<(u8, Pos, Tile)> {
    let mut out = Vec::with_capacity(usize::from(cols) * usize::from(rows));
    for y in 0..rows {
        for x in 0..cols {
            let color = match over {
                Some(((ox, oy), c)) if ox == x && oy == y => c,
                _ => base,
            };
            out.push((0, Pos::new(x, y), Tile::new(' ', Style::new().bg(color))));
        }
    }
    out
}

fn to_content(frame: &[(u8, Pos, Tile)]) -> impl Iterator<Item = (u8, Pos, &Tile, Option<&str>)> {
    frame
        .iter()
        .map(|(layer, pos, tile)| (*layer, *pos, tile, None))
}

const BASE: Color = Color::Rgb {
    r: 20,
    g: 20,
    b: 20,
};
const CHANGED: Color = Color::Rgb { r: 200, g: 0, b: 0 };

/// Registers `no_change` / `one_cell_changed` / `all_changed` cases for one
/// grid size, benchmarking `draw_layers` alone (isolating diff cost) and
/// `draw_layers` + `present` (end-to-end, `present` a documented headless
/// no-op).
fn bench_size(c: &mut Criterion, cols: u16, rows: u16) {
    let baseline = frame(cols, rows, BASE, None);
    let no_change = frame(cols, rows, BASE, None);
    let one_changed = frame(cols, rows, BASE, Some(((0, 0), CHANGED)));
    let all_changed = frame(cols, rows, CHANGED, None);

    for (name, target) in [
        ("no_change", &no_change),
        ("one_cell_changed", &one_changed),
        ("all_changed", &all_changed),
    ] {
        let renderer = RefCell::new(
            SoftwareBackendBuilder::new()
                .font(unscii16::FONT)
                .grid_size(cols, rows)
                .scale(1)
                .build()
                .unwrap()
                .run_headless()
                .unwrap(),
        );

        {
            let mut group = c.benchmark_group(format!("damage/{cols}x{rows}/draw_layers_only"));
            group.bench_function(name, |b| {
                b.iter_batched(
                    || {
                        renderer
                            .borrow_mut()
                            .draw_layers(to_content(&baseline))
                            .unwrap();
                    },
                    |()| {
                        renderer
                            .borrow_mut()
                            .draw_layers(to_content(target))
                            .unwrap();
                    },
                    BatchSize::SmallInput,
                );
            });
            group.finish();
        }

        {
            let mut group =
                c.benchmark_group(format!("damage/{cols}x{rows}/draw_layers_and_present"));
            group.bench_function(name, |b| {
                b.iter_batched(
                    || {
                        let mut r = renderer.borrow_mut();
                        r.draw_layers(to_content(&baseline)).unwrap();
                        r.present().unwrap();
                    },
                    |()| {
                        let mut r = renderer.borrow_mut();
                        r.draw_layers(to_content(target)).unwrap();
                        r.present().unwrap();
                    },
                    BatchSize::SmallInput,
                );
            });
            group.finish();
        }
    }
}

fn damage(c: &mut Criterion) {
    // 80x24: classic terminal default. 200x60: large viewport, where the
    // O(rows) diff cost has more room to matter.
    bench_size(c, 80, 24);
    bench_size(c, 200, 60);
}

criterion_group!(benches, damage);
criterion_main!(benches);
