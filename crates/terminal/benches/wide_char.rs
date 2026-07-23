//! Benchmark for [`TerminalRenderer::draw`] on a wide-char-heavy frame (CJK/emoji written via
//! [`Grid::write_grapheme`]), to measure the `unicode_width` cost the `egc` feature path pays per
//! cell versus the plain single-codepoint path.
//!
//! retroglyph#275 asks for this specifically: with `egc` enabled, `draw` computes each cell's
//! display width via `unicode_width::UnicodeWidthStr::width` on the full grapheme text rather
//! than `UnicodeWidthChar::width` on a single `char`, and multi-codepoint EGCs (ZWJ sequences)
//! additionally pull their text out of the grid's per-layer extras side-table. This bench
//! isolates that cost by comparing an all-wide-glyph frame against an all-ASCII frame of the same
//! size and change shape.
//!
//! Only built when the `egc` feature is enabled (`write_grapheme` doesn't exist otherwise); see
//! the matching `required-features` entry in `Cargo.toml`.

// `criterion_group!`/`criterion_main!` below expand to an undocumented `pub fn benches(..)`; this
// bench binary isn't a published API surface for `missing_docs` to usefully gate.
#![allow(missing_docs)]

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use retroglyph_core::{Color, Grid, Style, Tile};
use retroglyph_terminal::TerminalRenderer;
use std::hint::black_box;

const COLS: u16 = 200;
const ROWS: u16 = 50;

/// A small pool of 2-column-wide graphemes: plain CJK ideographs, simple emoji, and one
/// multi-codepoint ZWJ sequence (family emoji) to also exercise the extras-side-table /
/// `cap_grapheme` path that single-codepoint wide chars don't touch.
const WIDE_GLYPHS: [&str; 6] = ["宽", "文", "字", "😀", "🎉", "👨‍👩‍👧‍👦"];

/// Fills a grid with wide (2-column) graphemes via [`Grid::write_grapheme`], cycling through
/// [`WIDE_GLYPHS`] so the benchmark isn't measuring a single memoized width lookup.
fn wide_char_grid(cols: u16, rows: u16) -> Grid {
    let style = Style::new().fg(Color::Rgb {
        r: 200,
        g: 200,
        b: 0,
    });
    let mut grid = Grid::new(cols, rows);
    let mut glyph_index = 0usize;
    let mut x = 0u16;
    while x + 1 < cols {
        for y in 0..rows {
            let glyph = WIDE_GLYPHS[glyph_index % WIDE_GLYPHS.len()];
            grid.write_grapheme(0, x, y, glyph, style);
        }
        glyph_index += 1;
        x += 2;
    }
    grid
}

/// Fills a grid with plain single-column ASCII glyphs, as a baseline for the wide-char frame
/// above (same size, same style, only the glyph width and codepoint count differ).
fn ascii_grid(cols: u16, rows: u16) -> Grid {
    let style = Style::new().fg(Color::Rgb {
        r: 200,
        g: 200,
        b: 0,
    });
    let mut grid = Grid::new(cols, rows);
    for y in 0..rows {
        for x in 0..cols {
            grid.put(x, y, Tile::new('a', style));
        }
    }
    grid
}

/// Renders a full-repaint diff (from an empty base grid) through a [`TerminalRenderer`] and
/// returns the emitted bytes.
fn render_full(base: &Grid, frame: &Grid) -> Vec<u8> {
    let mut renderer = TerminalRenderer::new(Vec::new());
    renderer
        .draw(
            base.diff(frame)
                .map(|(_, pos, tile, extra)| (pos, tile, extra)),
        )
        .expect("Vec<u8> writes never fail");
    renderer.flush().expect("Vec<u8> flush never fails");
    renderer.into_writer()
}

/// Wide-char-heavy (CJK/emoji, `egc`) frame vs an all-ASCII frame of the same size.
fn bench_wide_char(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame_render/wide_char_200x50");
    let base = Grid::new(COLS, ROWS);

    let wide = wide_char_grid(COLS, ROWS);
    let bytes = render_full(&base, &wide).len() as u64;
    group.throughput(Throughput::Bytes(bytes));
    group.bench_function("wide_char_cjk_emoji", |b| {
        b.iter(|| black_box(render_full(&base, &wide)));
    });

    let ascii = ascii_grid(COLS, ROWS);
    let bytes = render_full(&base, &ascii).len() as u64;
    group.throughput(Throughput::Bytes(bytes));
    group.bench_function("ascii_baseline", |b| {
        b.iter(|| black_box(render_full(&base, &ascii)));
    });

    group.finish();
}

criterion_group!(benches, bench_wide_char);
criterion_main!(benches);
