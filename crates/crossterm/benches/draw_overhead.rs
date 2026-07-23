//! Benchmarks a full `Output::draw` + `Output::flush` frame through the crossterm backend,
//! writing into an in-memory `Vec<u8>` via `CrosstermOptions::build_with_writer` instead of a
//! real terminal.
//!
//! retroglyph#285 asks for this to isolate the crossterm layer's own overhead --
//! `begin_synchronized_update`/`end_synchronized_update` plus the delegation into
//! `retroglyph_terminal::TerminalRenderer` -- on top of the renderer this crate wraps. All
//! optional real-terminal features (raw mode, alt screen, mouse capture, focus-change reporting,
//! bracketed paste, kitty protocol) are disabled via `CrosstermOptions`, matching the existing
//! `build_with_writer_renders_cell_content_into_a_custom_sink` unit test's pattern for
//! constructing a `Crossterm<Vec<u8>>` without a real TTY: only the writer-bound draw/flush path
//! is exercised, so this measures rendering cost, not terminal-protocol setup.
//!
//! Each iteration draws a full grid of distinct tiles (varying glyph and color per cell, so the
//! renderer can't shortcut on "everything unchanged") into a fresh backend and clears the sink
//! buffer between iterations to keep memory bounded and avoid amortizing allocation growth into
//! later iterations' timings.

// `criterion_group!`/`criterion_main!` below expand to an undocumented `pub fn benches(..)`; this
// bench binary isn't a published API surface for `missing_docs` to usefully gate.
#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use retroglyph_core::backend::Output;
use retroglyph_core::color::Color;
use retroglyph_core::grid::Pos;
use retroglyph_core::style::Style;
use retroglyph_core::tile::Tile;
use retroglyph_crossterm::Crossterm;
use std::hint::black_box;

/// Builds a `cols x rows` frame of `(Pos, Tile)` pairs, each cell distinct (glyph cycles through
/// a small alphabet, foreground color cycles through RGB) so every cell differs from the
/// backend's initial (blank) state and the renderer must emit real output for all of them.
fn frame(cols: u16, rows: u16) -> Vec<(Pos, Tile)> {
    const GLYPHS: [char; 4] = ['#', '@', '.', '%'];
    let mut cells = Vec::with_capacity(usize::from(cols) * usize::from(rows));
    for y in 0..rows {
        for x in 0..cols {
            let idx = usize::from(x) + usize::from(y) * usize::from(cols);
            let glyph = GLYPHS[idx % GLYPHS.len()];
            #[allow(clippy::cast_possible_truncation)]
            let color = Color::Rgb {
                r: (idx * 7 % 256) as u8,
                g: (idx * 13 % 256) as u8,
                b: (idx * 29 % 256) as u8,
            };
            cells.push((Pos { x, y }, Tile::new(glyph, Style::new().fg(color))));
        }
    }
    cells
}

/// Constructs a `Crossterm<Vec<u8>>` with every real-terminal feature disabled, matching the
/// pattern the crate's own `build_with_writer_renders_cell_content_into_a_custom_sink` unit test
/// uses to build one without a real TTY.
fn headless_backend() -> Crossterm<Vec<u8>> {
    Crossterm::builder()
        .raw_mode(false)
        .alt_screen(false)
        .mouse_capture(false)
        .focus_change(false)
        .bracketed_paste(false)
        .kitty_protocol(false)
        .build_with_writer(Vec::new())
        .expect("building against a Vec<u8> writer with all TTY features disabled must not require a real terminal")
}

fn bench_size(c: &mut Criterion, cols: u16, rows: u16) {
    let cells = frame(cols, rows);

    let name = format!("draw_overhead/{cols}x{rows}");
    c.bench_function(&name, |b| {
        b.iter_batched(
            headless_backend,
            |mut term| {
                term.draw(cells.iter().map(|(pos, tile)| (*pos, tile, None)))
                    .expect("draw into an in-memory Vec<u8> writer cannot fail");
                term.flush()
                    .expect("flush into an in-memory Vec<u8> writer cannot fail");
                black_box(term.writer().len());
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn draw_overhead(c: &mut Criterion) {
    // 80x24: the classic terminal default. 200x60: a large terminal / roguelike viewport.
    bench_size(c, 80, 24);
    bench_size(c, 200, 60);
}

criterion_group!(benches, draw_overhead);
criterion_main!(benches);
