//! Criterion benchmarks for retroglyph core operations.
//!
//! Run with `just bench`. To save a named baseline:
//!   cargo criterion -- --save-baseline main
//! To compare against it:
//!   cargo criterion -- --baseline main
#![allow(missing_docs)] // benches don't need doc comments

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use retroglyph::Grid;
use retroglyph::Terminal;
use retroglyph::Tile;
use retroglyph::backend::Headless;
use retroglyph::style::Style;

// ── helpers ──────────────────────────────────────────────────────────────────

fn tile(glyph: char) -> Tile {
    Tile::new(glyph, Style::default())
}

fn filled_grid(width: u16, height: u16, glyph: char) -> Grid {
    let mut g = Grid::new(width, height);
    for y in 0..height {
        for x in 0..width {
            g.put(x, y, tile(glyph));
        }
    }
    g
}

// ── Grid::diff ───────────────────────────────────────────────────────────────

fn bench_grid_diff(c: &mut Criterion) {
    let mut group = c.benchmark_group("Grid::diff");

    for (label, w, h) in [("80x24", 80u16, 24u16), ("160x50", 160, 50)] {
        let identical_a = filled_grid(w, h, '.');
        let identical_b = filled_grid(w, h, '.');
        group.bench_with_input(BenchmarkId::new("identical", label), &(w, h), |b, _| {
            b.iter(|| identical_a.diff(&identical_b).count());
        });

        let all_diff_a = filled_grid(w, h, '.');
        let all_diff_b = filled_grid(w, h, '#');
        group.bench_with_input(BenchmarkId::new("all_different", label), &(w, h), |b, _| {
            b.iter(|| all_diff_a.diff(&all_diff_b).count());
        });
    }

    group.finish();
}

// ── Grid::put / Grid::get ────────────────────────────────────────────────────

fn bench_grid_put_get(c: &mut Criterion) {
    let mut group = c.benchmark_group("Grid");

    let mut g = Grid::new(80, 24);
    group.bench_function("put/in_bounds", |b| {
        b.iter(|| g.put(40, 12, tile('x')));
    });

    // Checked variants so we can exercise the OOB branch without panicking.
    group.bench_function("checked_put/oob", |b| {
        b.iter(|| g.checked_put(200, 200, tile('x')));
    });

    group.bench_function("get/in_bounds", |b| {
        b.iter(|| g.get(40, 12));
    });

    group.bench_function("checked_get/oob", |b| {
        b.iter(|| g.checked_get(200, 200));
    });

    group.finish();
}

// ── Grid::clear ──────────────────────────────────────────────────────────────

fn bench_grid_clear(c: &mut Criterion) {
    let mut group = c.benchmark_group("Grid::clear");

    for (label, w, h) in [("80x24", 80u16, 24u16), ("160x50", 160, 50)] {
        // Pre-fill so clear actually has work to do.
        let mut g = filled_grid(w, h, '#');
        group.bench_with_input(BenchmarkId::from_parameter(label), &(w, h), |b, _| {
            b.iter(|| {
                g.clear(0);
                // Refill between iterations so every iteration clears real data.
                for y in 0..h {
                    for x in 0..w {
                        g.put(x, y, tile('#'));
                    }
                }
            });
        });
    }

    group.finish();
}

// ── Terminal::present ────────────────────────────────────────────────────────

fn bench_terminal_present(c: &mut Criterion) {
    let mut group = c.benchmark_group("Terminal::present");

    // no-changes: present a cleared terminal (all default tiles, no diff).
    group.bench_function("no_changes", |b| {
        let mut term = Terminal::new(Headless::new(80, 24));
        b.iter(|| {
            let _ = term.present();
        });
    });

    // single-cell: write one character each frame.
    group.bench_function("single_cell", |b| {
        let mut term = Terminal::new(Headless::new(80, 24));
        b.iter(|| {
            term.put(40, 12, '@');
            let _ = term.present();
        });
    });

    // full-redraw: fill every cell before each present.
    group.bench_function("full_redraw/80x24", |b| {
        let mut term = Terminal::new(Headless::new(80, 24));
        b.iter(|| {
            for y in 0..24u16 {
                for x in 0..80u16 {
                    term.put(x, y, '#'); // Terminal::put takes char directly
                }
            }
            let _ = term.present();
        });
    });

    group.finish();
}

// ── Crossterm::draw ───────────────────────────────────────────────────────────

#[cfg(feature = "crossterm")]
fn bench_crossterm_draw(c: &mut Criterion) {
    use retroglyph::backend::Backend;
    use retroglyph::backend::Crossterm;
    use retroglyph::style::Style;

    let mut group = c.benchmark_group("CrosstermBackend::draw");

    // Requires a real TTY. Skip in CI unless BENCH_CROSSTERM=1 is set.
    if !crossterm::terminal::is_raw_mode_enabled().unwrap_or(false)
        && std::env::var("BENCH_CROSSTERM").is_err()
    {
        eprintln!("CrosstermBackend::draw: skipped (no TTY; set BENCH_CROSSTERM=1 to force)");
        group.finish();
        return;
    }

    let mut g = Grid::new(80, 24);
    for y in 0..24u16 {
        for x in 0..80u16 {
            g.put(x, y, Tile::new('#', Style::default()));
        }
    }

    if let Ok(mut backend) = Crossterm::new() {
        group.bench_function("full_frame/80x24", |b| {
            b.iter(|| {
                let _ = backend.draw_layers(g.layers());
                let _ = backend.flush();
            });
        });
    }

    group.finish();
}

#[cfg(not(feature = "crossterm"))]
fn bench_crossterm_draw(_c: &mut Criterion) {}

// ── SoftwareRenderer::draw_layers ────────────────────────────────────────────

#[cfg(feature = "software-default-font")]
fn bench_software_renderer(c: &mut Criterion) {
    use retroglyph::backend::Backend;
    use retroglyph::backend::software::{SoftwareBackendBuilder, bitmap_font::vga8x16};
    use retroglyph::style::Style;

    let mut group = c.benchmark_group("SoftwareRenderer::draw_layers");

    let opts = SoftwareBackendBuilder::new()
        .font(vga8x16::FONT)
        .grid_size(80, 24)
        .build()
        .expect("software backend build failed");
    let mut renderer = opts.run_headless();

    let mut g = Grid::new(80, 24);
    for y in 0..24u16 {
        for x in 0..80u16 {
            g.put(x, y, Tile::new('#', Style::default()));
        }
    }

    group.bench_function("full_frame/80x24", |b| {
        b.iter(|| {
            let _ = renderer.draw_layers(g.layers());
        });
    });

    // Best case: empty grid (all default/blank tiles).
    let empty = Grid::new(80, 24);
    group.bench_function("blank_frame/80x24", |b| {
        b.iter(|| {
            let _ = renderer.draw_layers(empty.layers());
        });
    });

    group.finish();
}

#[cfg(not(feature = "software-default-font"))]
fn bench_software_renderer(_c: &mut Criterion) {}

// ── criterion wiring ─────────────────────────────────────────────────────────

criterion_group!(
    benches,
    bench_grid_diff,
    bench_grid_put_get,
    bench_grid_clear,
    bench_terminal_present,
    bench_crossterm_draw,
    bench_software_renderer,
);
criterion_main!(benches);
