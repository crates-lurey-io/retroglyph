//! Benchmark demonstrating the `#294` `Mouse(Moved)` coalescing fix in
//! `WindowBackend::push_event`.
//!
//! winit can deliver `CursorMoved` at device polling rate (hundreds/sec) though only the latest
//! position matters once the queue is next drained, so `push_event` collapses a run of `Moved`
//! events into the single most recent one instead of growing the queue unbounded. This
//! benchmarks pushing a burst of `Moved` events (as `handle_window_event`'s `CursorMoved` arm
//! would produce) directly through the crate's public `Input::push_event` surface -- see
//! `WindowBackend`'s own `#[cfg(test)]` module for the companion assertion that the queue length
//! after such a burst stays at 1, not `burst size`.

#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use retroglyph_core::backend::{Input, Output};
use retroglyph_core::event::{Event, KeyModifiers, MouseEvent, MouseEventKind};
use retroglyph_core::grid::{Pos, Size};
use retroglyph_core::tile::Tile;
use retroglyph_window::{Presenter, WindowBackend, WindowHandle};
use std::sync::Arc;
use std::time::Duration;

/// A no-op presenter: this benchmark only exercises the input queue, not rendering.
#[derive(Default)]
struct NullPresenter;

impl Output for NullPresenter {
    type Error = core::convert::Infallible;

    fn draw<'a, I>(&mut self, _content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (Pos, &'a Tile, Option<&'a str>)>,
    {
        Ok(())
    }

    fn draw_layers<'a, I>(&mut self, _content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u8, Pos, &'a Tile, Option<&'a str>)>,
    {
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn size(&self) -> Size {
        Size {
            width: 4,
            height: 2,
        }
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn resize(&mut self, _size: Size) {}
}

impl Presenter for NullPresenter {
    type SurfaceError = core::convert::Infallible;

    fn init_surface(&mut self, _window: Arc<dyn WindowHandle>) -> Result<(), Self::SurfaceError> {
        Ok(())
    }

    fn resize_surface(&mut self, _width: u32, _height: u32) {}

    fn present(&mut self) -> Result<(), Self::SurfaceError> {
        Ok(())
    }

    fn cell_size(&self) -> (u32, u32) {
        (8, 16)
    }
}

/// A burst of `Moved` events at monotonically increasing positions, as a fast mouse sweep across
/// the window would produce between two frame polls.
fn moved_burst(len: usize) -> Vec<Event> {
    (0..len)
        .map(|i| {
            #[allow(clippy::cast_possible_truncation)]
            let x = (i % usize::from(u16::MAX)) as u16;
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Moved,
                position: Pos { x, y: 0 },
                pixel_position: None,
                modifiers: KeyModifiers::NONE,
            })
        })
        .collect()
}

fn bench_cursor_moved_burst(c: &mut Criterion) {
    let burst = moved_burst(1_000);
    c.bench_function("push_event/1k_cursor_moved_burst", |b| {
        b.iter(|| {
            let mut backend = WindowBackend::new(NullPresenter);
            for event in &burst {
                backend.push_event(event.clone());
            }
            // Drain so the coalescing behavior (not just the push loop) is part of what's
            // timed, matching how the real event loop polls once per frame.
            while backend.poll_event(Duration::ZERO).is_some() {}
        });
    });
}

criterion_group!(benches, bench_cursor_moved_burst);
criterion_main!(benches);
