//! Benchmarks for [`truncate`] on ASCII, wide-char, and zero-width input.
//!
//! retroglyph#316 asks for this to "guard the borrowing rewrite": `truncate` currently walks
//! `s.chars()` accumulating a display-width budget via `unicode_width::UnicodeWidthChar` and
//! copies the surviving prefix into a new `String`. A future rewrite that returns a borrowed
//! `&str` slice instead of an owned `String` should show up here as a real allocation-cost win,
//! not just a signature change -- these three input shapes exercise the width accounting
//! differently: ASCII is the width-1-per-char fast path, wide CJK-style characters are width-2
//! (so the walk terminates roughly twice as early per column budget), and zero-width combining
//! marks contribute 0 width each, forcing the walk all the way to the end of the string before
//! the budget is ever exceeded.
//!
//! `--test` runs each benchmark once (a compile/smoke check); a real run is
//! `cargo bench -p retroglyph-widgets --bench text_truncate`.

// `criterion_group!`/`criterion_main!` below expand to an undocumented `pub fn benches(..)`; this
// bench binary isn't a published API surface for `missing_docs` to usefully gate.
#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use retroglyph_widgets::truncate;
use std::hint::black_box;

/// Builds a `len`-character-long ASCII string, longer than any `max_cols` budget used below so
/// truncation always has to walk (and stop partway through) the string.
fn ascii(len: usize) -> String {
    "the quick brown fox jumps over the lazy dog "
        .chars()
        .cycle()
        .take(len)
        .collect()
}

/// Builds a `len`-character-long string of width-2 wide characters (CJK-style, "あ" U+3042),
/// so the column budget is exhausted in half as many characters as the ASCII case.
fn wide(len: usize) -> String {
    "あ".repeat(len)
}

/// Builds a `len`-character-long string of zero-width combining marks (U+0301 COMBINING ACUTE
/// ACCENT) around a single leading `'a'`, so a width-based truncation walks the *entire* string
/// before ever exceeding a realistic column budget -- the pathological case for a naive
/// per-character loop, since every zero-width char still costs a `chars()` step and a
/// `UnicodeWidthChar::width` call.
fn zero_width(len: usize) -> String {
    let mut s = String::with_capacity(len + 1);
    s.push('a');
    s.extend(std::iter::repeat_n('\u{0301}', len.saturating_sub(1)));
    s
}

// A representative column budget (a table cell / list item width), well inside every input's
// full length so the walk-and-stop-early behavior is what's actually measured.
const MAX_COLS: usize = 40;
const LEN: usize = 500;

fn text_truncate(c: &mut Criterion) {
    let mut group = c.benchmark_group("text_truncate");

    let ascii_input = ascii(LEN);
    group.bench_function("ascii", |b| {
        b.iter(|| black_box(truncate(black_box(&ascii_input), MAX_COLS)));
    });

    let wide_input = wide(LEN);
    group.bench_function("wide_char", |b| {
        b.iter(|| black_box(truncate(black_box(&wide_input), MAX_COLS)));
    });

    let zero_width_input = zero_width(LEN);
    group.bench_function("zero_width", |b| {
        b.iter(|| black_box(truncate(black_box(&zero_width_input), MAX_COLS)));
    });

    group.finish();
}

criterion_group!(benches, text_truncate);
criterion_main!(benches);
