//! Benchmarks `Output::size()`'s per-call cost against a headless `Crossterm<Vec<u8>>`.
//!
//! retroglyph#285 asks for this number to justify (or not) caching `size()` instead of calling
//! `crossterm::terminal::size()` -- an ioctl-backed syscall -- on every call. Run under `cargo
//! bench` (no TTY attached to the process's stdout), `crossterm::terminal::size()` fails and
//! `Output::size()` falls back to its hardcoded `(80, 25)` default; the syscall (and its
//! failure path) still runs every time, which is exactly the per-call cost this benchmark is
//! measuring -- a real TTY would additionally pay for the ioctl's kernel-side work, so this
//! number is a floor, not a ceiling, on the real-terminal cost.
//!
//! # `poll_event`'s retry-on-unmappable-event path: not benchmarked here
//!
//! retroglyph#285 also asks for a `poll_event(Duration::ZERO)` drain-loop throughput benchmark
//! against a queue of unmappable events, to guard the retry loop at `lib.rs:778-786`. This isn't
//! feasible to build headlessly: `crossterm::event::poll`/`read` read from the process's real
//! stdin/TTY file descriptor with no injectable mock event source, so there is no way to hand
//! `Crossterm::poll_event` a queue of synthetic (let alone specifically unmappable) events from
//! a benchmark process without an actual pty. Faking the retry loop's cost by calling
//! `from_crossterm_event` in a tight loop (as `benches/event_translation.rs` already does for the
//! translation-throughput case) would misrepresent what this benchmark claims to measure: the
//! retry loop's cost is dominated by the repeated `crossterm::event::poll`/`read` syscalls, not
//! by `from_crossterm_event` itself, and stubbing out the syscalls would just be measuring
//! `event_translation`'s benchmark a second time under a different name. Guarding the retry path
//! is left to the existing `Input::poll_event` doc comment's non-blocking-single-syscall
//! contract plus manual/integration testing against a real TTY, rather than a headless benchmark
//! that can't actually exercise it.

// `criterion_group!`/`criterion_main!` below expand to an undocumented `pub fn benches(..)`; this
// bench binary isn't a published API surface for `missing_docs` to usefully gate.
#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use retroglyph_core::backend::Output;
use retroglyph_crossterm::Crossterm;
use std::hint::black_box;

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

fn size_cost(c: &mut Criterion) {
    let term = headless_backend();

    c.bench_function("backend_overhead/size", |b| {
        b.iter(|| black_box(term.size()));
    });
}

criterion_group!(benches, size_cost);
criterion_main!(benches);
