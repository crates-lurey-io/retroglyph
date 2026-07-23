//! Benchmarks for `winit::translate`'s per-keystroke conversion helpers (retroglyph#299).
//!
//! `translate_key` itself (the `run.rs` call site) takes `winit::event::KeyEvent`, which has a
//! private platform-specific field in the pinned winit version and so can't be constructed
//! outside the crate -- the same constraint documented on `key_code_from_logical` in
//! `translate.rs`, which is why that crate's own unit tests bypass `translate_key` too. This
//! benchmarks the two pieces of that same per-key hot path that *are* public and constructible:
//! `key_event_kind` (state/repeat -> `KeyEventKind`) and `translate_modifiers` (winit modifier
//! state -> `KeyModifiers`), both called once per key event in `handle_window_event`.

#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use retroglyph_window::winit::translate::{key_event_kind, translate_modifiers};
use std::hint::black_box;
use winit::event::ElementState;
use winit::keyboard::ModifiersState;

/// A representative stream of (state, repeat) pairs: mostly plain presses/releases with an
/// occasional OS-generated repeat, matching what a held key produces.
fn key_state_stream(len: usize) -> Vec<(ElementState, bool)> {
    let mut rng = fastrand::Rng::with_seed(11);
    (0..len)
        .map(|_| {
            let state = if rng.bool() {
                ElementState::Pressed
            } else {
                ElementState::Released
            };
            let repeat = state == ElementState::Pressed && rng.f64() < 0.3;
            (state, repeat)
        })
        .collect()
}

/// A representative stream of modifier combinations (mostly none/shift, occasionally all four).
fn modifier_stream(len: usize) -> Vec<ModifiersState> {
    let mut rng = fastrand::Rng::with_seed(13);
    (0..len)
        .map(|_| {
            let mut m = ModifiersState::empty();
            if rng.f64() < 0.3 {
                m |= ModifiersState::SHIFT;
            }
            if rng.f64() < 0.1 {
                m |= ModifiersState::CONTROL;
            }
            if rng.f64() < 0.1 {
                m |= ModifiersState::ALT;
            }
            if rng.f64() < 0.05 {
                m |= ModifiersState::SUPER;
            }
            m
        })
        .collect()
}

fn bench_key_event_kind(c: &mut Criterion) {
    let states = key_state_stream(10_000);
    c.bench_function("key_event_kind/10k_keys", |b| {
        b.iter(|| {
            for &(state, repeat) in &states {
                black_box(key_event_kind(black_box(state), black_box(repeat)));
            }
        });
    });
}

fn bench_translate_modifiers(c: &mut Criterion) {
    let modifiers = modifier_stream(10_000);
    c.bench_function("translate_modifiers/10k_keys", |b| {
        b.iter(|| {
            for &state in &modifiers {
                black_box(translate_modifiers(black_box(state)));
            }
        });
    });
}

criterion_group!(benches, bench_key_event_kind, bench_translate_modifiers);
criterion_main!(benches);
