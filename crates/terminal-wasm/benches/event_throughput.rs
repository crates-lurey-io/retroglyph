//! Benchmarks for [`decode_key_event`]/[`decode_mouse_event`] throughput and for the
//! [`TerminalWasm`] event queue's drain throughput under a burst of pushed events.
//!
//! retroglyph#292 asks for these: `decode_key_event`/`decode_mouse_event` are called once per
//! input event crossing the WASM/JS boundary (`wasm_terminal_push_key`/`wasm_terminal_push_mouse`
//! in `src/lib.rs`'s `wasm` module), and are correctness-proptested today but never measured for
//! perf; the queue itself is a plain `VecDeque<Event>` with no back-pressure, so it's worth knowing
//! how it behaves under a realistic burst (e.g. a fast mouse drag firing many `mousemove` events
//! per frame) before that ever becomes a problem.

// See `crates/core/benches/grid_diff.rs` for why this bench binary is exempted from `missing_docs`.
#![allow(missing_docs)]

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use retroglyph_core::backend::Backend as _;
use retroglyph_core::event::Event;
use retroglyph_terminal_wasm::{TerminalWasm, decode_key_event, decode_mouse_event, mouse_actions};
use std::hint::black_box;

/// A representative, deterministic stream of `(code, mods)` pairs for [`decode_key_event`]:
/// mostly printable ASCII characters (as typed text tends to be), with a scattering of named keys
/// and modifier combinations thrown in, matching how a real input stream mixes both.
fn key_event_stream(len: usize) -> Vec<(u32, u8)> {
    use retroglyph_terminal_wasm::key_codes;

    let named = [
        key_codes::LEFT,
        key_codes::RIGHT,
        key_codes::UP,
        key_codes::DOWN,
        key_codes::ENTER,
        key_codes::BACKSPACE,
    ];

    let mut rng = fastrand::Rng::with_seed(42);
    (0..len)
        .map(|_| {
            if rng.u8(0..10) == 0 {
                (named[rng.usize(0..named.len())], rng.u8(0..16))
            } else {
                (u32::from(rng.u8(b'a'..=b'z')), rng.u8(0..16))
            }
        })
        .collect()
}

/// A representative, deterministic stream of `(x, y, action, button, mods)` tuples for
/// [`decode_mouse_event`]: mostly `Moved` (as a drag or hover produces many more move events than
/// clicks), with occasional button-down/up and scroll events.
fn mouse_event_stream(len: usize) -> Vec<(u16, u16, u8, u8, u8)> {
    use retroglyph_terminal_wasm::mouse_buttons;

    let mut rng = fastrand::Rng::with_seed(42);
    (0..len)
        .map(|_| {
            let x = rng.u16(0..200);
            let y = rng.u16(0..60);
            let action = match rng.u8(0..20) {
                0 => mouse_actions::DOWN,
                1 => mouse_actions::UP,
                2 => mouse_actions::SCROLL_UP,
                3 => mouse_actions::SCROLL_DOWN,
                _ => mouse_actions::MOVED,
            };
            (x, y, action, mouse_buttons::LEFT, rng.u8(0..16))
        })
        .collect()
}

/// Benchmarks `decode_key_event`/`decode_mouse_event` over a fixed-size representative stream,
/// reporting `Throughput::Elements` so criterion normalizes to ns/event.
fn decode_throughput(c: &mut Criterion) {
    const STREAM_LEN: usize = 10_000;

    let mut group = c.benchmark_group("decode_event");
    group.throughput(Throughput::Elements(STREAM_LEN as u64));

    let keys = key_event_stream(STREAM_LEN);
    group.bench_function("decode_key_event", |b| {
        b.iter(|| {
            for &(code, mods) in &keys {
                black_box(decode_key_event(code, mods));
            }
        });
    });

    let mice = mouse_event_stream(STREAM_LEN);
    group.bench_function("decode_mouse_event", |b| {
        b.iter(|| {
            for &(x, y, action, button, mods) in &mice {
                black_box(decode_mouse_event(x, y, action, button, mods));
            }
        });
    });

    group.finish();
}

/// Benchmarks pushing a burst of mouse-move events onto [`TerminalWasm`]'s event queue and then
/// fully draining it via `poll_event`, at a few burst sizes -- e.g. simulating a fast mouse drag
/// firing many `mousemove` events within a single animation frame before JS's next
/// `take_output`/drain cycle.
fn queue_drain_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("event_queue_drain");

    for burst in [100u64, 1_000, 10_000] {
        group.throughput(Throughput::Elements(burst));
        #[allow(clippy::cast_possible_truncation)]
        group.bench_function(format!("burst_{burst}"), |b| {
            b.iter_batched(
                || {
                    let mut backend = TerminalWasm::new(80, 24);
                    for i in 0..burst {
                        backend.push_event(Event::Mouse(
                            decode_mouse_event(
                                (i % 80) as u16,
                                (i % 24) as u16,
                                mouse_actions::MOVED,
                                0,
                                0,
                            )
                            .expect("MOVED action always decodes"),
                        ));
                    }
                    backend
                },
                |mut backend| {
                    let mut count = 0u64;
                    while let Some(event) = backend.poll_event(std::time::Duration::ZERO) {
                        black_box(event);
                        count += 1;
                    }
                    black_box(count)
                },
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

criterion_group!(benches, decode_throughput, queue_drain_throughput);
criterion_main!(benches);
