//! [`FrameRecorder`], its [`FrameRecorderHandle`], and [`CapturedFrame`]: the `TestHarness`/
//! `Headless`-driven capture source.

use crate::OwnedCell;
use retroglyph_core::backend::{Compositing, Cursor, CursorStyle, DrawCell, Input, Output};
use retroglyph_core::event::Event;
use retroglyph_core::grid::{Pos, Size};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// One changed frame: every cell [`Output::draw_layers`] reported changed on that call, with the
/// wall-clock time since [`FrameRecorder::new`].
#[derive(Debug, Clone)]
pub struct CapturedFrame {
    /// Time since the recorder was created that this frame's `draw_layers` call arrived.
    pub at: Duration,
    /// Every cell changed on this call, in the order [`Output::draw_layers`] delivered them.
    pub cells: Vec<OwnedCell>,
}

/// State shared between a [`FrameRecorder`] and every [`FrameRecorderHandle`] cloned from it.
struct State {
    started: Instant,
    frames: Vec<CapturedFrame>,
}

/// A cheap, cloneable handle onto a [`FrameRecorder`]'s captured frames.
///
/// Exists for the same reason [`InputRecorder`](crate::InputRecorder)'s
/// [`RecorderHandle`](crate::RecorderHandle) does: a driver like
/// [`retroglyph_core::app::run_on`] takes a `Terminal<B>` (and so the `FrameRecorder<B>` it
/// wraps) by value and never hands it back, so the only way to read what was captured once the
/// loop returns is a handle taken out beforehand. Backed by `Arc<Mutex<..>>`: cloning shares the
/// same underlying frames, it does not fork them.
#[derive(Clone)]
pub struct FrameRecorderHandle {
    state: Arc<Mutex<State>>,
}

impl FrameRecorderHandle {
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                started: Instant::now(),
                frames: Vec::new(),
            })),
        }
    }

    fn record(&self, cells: Vec<OwnedCell>) {
        let mut state = self.lock();
        let at = state.started.elapsed();
        state.frames.push(CapturedFrame { at, cells });
    }

    /// Locks the shared state, recovering it if a previous holder panicked while holding the
    /// lock rather than poisoning every future access -- the same rationale
    /// [`RecorderHandle`](crate::RecorderHandle)'s own lock helper uses.
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A clone of every frame captured so far, in the order it was drawn.
    #[must_use]
    pub fn frames(&self) -> Vec<CapturedFrame> {
        self.lock().frames.clone()
    }

    /// Takes every captured frame, leaving the shared buffer empty. Use this (rather than
    /// [`frames`](Self::frames) plus a manual clear) to hand ownership to
    /// [`write_cast`](crate::write_cast) without cloning it.
    #[must_use]
    pub fn take_frames(&self) -> Vec<CapturedFrame> {
        std::mem::take(&mut self.lock().frames)
    }
}

/// Wraps a backend and records every frame it draws.
///
/// Taps [`Output::draw_layers`] -- the same [`DrawCell`] diff stream
/// [`Terminal::present`](retroglyph_core::terminal::Terminal::present) and every backend already
/// consume -- and buffers each call's changed cells as an owned [`CapturedFrame`], timestamped
/// against when recording started. Works over a [`Backend`](retroglyph_core::backend::Backend),
/// or any subset of `Output`/[`Input`]/[`Cursor`]: only `Output` is tapped, everything else is
/// delegated unchanged, the same shape as
/// [`InputRecorder`](crate::InputRecorder).
///
/// `DrawCell` borrows from the source grid, so capture converts to [`OwnedCell`] immediately on
/// each `draw_layers` call rather than deferring to export time, when the borrow would already be
/// gone.
///
/// # Known limitation
///
/// Buffers the whole session in memory (every frame captured, and every cell in it, lives in the
/// shared [`FrameRecorderHandle`] until [`write_cast`](crate::write_cast) exports and
/// [`take_frames`](FrameRecorderHandle::take_frames) drops it) rather than streaming to disk as
/// frames arrive. Fine for docs/demo-length captures -- the ones this crate exists for -- but not
/// a fit for an unbounded, long-running recording.
///
/// # Examples
///
/// ```
/// use retroglyph_core::backend::{Headless, Output};
/// use retroglyph_core::color::Style;
/// use retroglyph_core::grid::Pos;
/// use retroglyph_core::tile::Tile;
/// use retroglyph_recorder::FrameRecorder;
///
/// let mut recorder = FrameRecorder::new(Headless::new(10, 3));
/// let tile = Tile::new('x', Style::default());
/// let cell = retroglyph_core::backend::DrawCell::new(Pos::new(0, 0), &tile);
/// recorder.draw_layers(std::iter::once(cell)).unwrap();
///
/// assert_eq!(recorder.frames().len(), 1);
/// ```
pub struct FrameRecorder<B> {
    inner: B,
    handle: FrameRecorderHandle,
}

impl<B: Output> FrameRecorder<B> {
    /// Wraps `inner`. Recording starts immediately: the clock [`CapturedFrame::at`] is measured
    /// against begins ticking here, not on the first `draw_layers` call.
    pub fn new(inner: B) -> Self {
        Self {
            inner,
            handle: FrameRecorderHandle::new(),
        }
    }
}

impl<B> FrameRecorder<B> {
    /// A cloneable [`FrameRecorderHandle`] onto this recorder's shared state, for reading
    /// captured frames after a driver (e.g. [`retroglyph_core::app::run_on`]) has taken this
    /// recorder's `Terminal` by value and won't hand it back.
    #[must_use]
    pub fn handle(&self) -> FrameRecorderHandle {
        self.handle.clone()
    }

    /// [`FrameRecorderHandle::frames`] on this recorder's own handle.
    #[must_use]
    pub fn frames(&self) -> Vec<CapturedFrame> {
        self.handle.frames()
    }

    /// [`FrameRecorderHandle::take_frames`] on this recorder's own handle.
    pub fn take_frames(&mut self) -> Vec<CapturedFrame> {
        self.handle.take_frames()
    }

    /// The wrapped backend, for anything not exposed through the decorator itself.
    pub const fn inner(&self) -> &B {
        &self.inner
    }

    /// The wrapped backend, mutably.
    pub const fn inner_mut(&mut self) -> &mut B {
        &mut self.inner
    }
}

impl<B: Output> Output for FrameRecorder<B> {
    type Error = B::Error;

    // `Output::draw`'s default body already forwards to `draw_layers` below, so every cell this
    // recorder ever sees -- including ones arriving through the single-layer `draw` entry point
    // -- is captured without needing an override here.

    fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = DrawCell<'a>>,
    {
        let cells: Vec<OwnedCell> = content.map(OwnedCell::from).collect();
        // An empty `draw_layers` call (nothing changed since the last present) is common --
        // `Terminal::present` calls it every frame regardless -- and recording it would pad
        // every capture with useless idle frames. Only cells that actually changed produce a
        // `CapturedFrame`.
        if cells.is_empty() {
            return self.inner.draw_layers(std::iter::empty());
        }
        let forwarded = cells.iter().map(|cell| {
            DrawCell::on_layer(cell.layer, cell.pos, &cell.tile)
                .with_grapheme(cell.grapheme.as_deref())
                .with_tint(cell.tint)
        });
        self.inner.draw_layers(forwarded)?;
        self.handle.record(cells);
        Ok(())
    }

    fn compositing(&self) -> Compositing {
        self.inner.compositing()
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush()
    }

    fn size(&self) -> Size {
        self.inner.size()
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.inner.clear()
    }

    fn resize(&mut self, size: Size) {
        self.inner.resize(size);
    }
}

impl<B: Input> Input for FrameRecorder<B> {
    fn poll_event(&mut self, timeout: Duration) -> Option<Event> {
        self.inner.poll_event(timeout)
    }

    fn push_event(&mut self, event: Event) {
        self.inner.push_event(event);
    }
}

impl<B: Cursor> Cursor for FrameRecorder<B> {
    fn set_cursor_visible(&mut self, visible: bool) {
        self.inner.set_cursor_visible(visible);
    }

    fn set_cursor_position(&mut self, position: Pos) {
        self.inner.set_cursor_position(position);
    }

    fn set_cursor_style(&mut self, style: CursorStyle) {
        self.inner.set_cursor_style(style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use retroglyph_core::backend::Headless;
    use retroglyph_core::color::Style;
    use retroglyph_core::tile::Tile;

    fn cell(x: u16, y: u16, glyph: char) -> (Pos, Tile) {
        (Pos::new(x, y), Tile::new(glyph, Style::default()))
    }

    #[test]
    fn buffers_owned_cells_per_draw_layers_call() {
        let mut recorder = FrameRecorder::new(Headless::new(10, 3));
        let (pos, tile) = cell(0, 0, 'a');
        recorder
            .draw_layers(std::iter::once(DrawCell::new(pos, &tile)))
            .unwrap();

        let frames = recorder.frames();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].cells.len(), 1);
        assert_eq!(frames[0].cells[0].tile.glyph(), 'a');
    }

    #[test]
    fn empty_draw_layers_calls_produce_no_frame() {
        let mut recorder = FrameRecorder::new(Headless::new(10, 3));
        recorder
            .draw_layers(std::iter::empty::<DrawCell<'_>>())
            .unwrap();

        assert!(recorder.frames().is_empty());
    }

    #[test]
    fn take_frames_empties_the_buffer() {
        let mut recorder = FrameRecorder::new(Headless::new(10, 3));
        let (pos, tile) = cell(0, 0, 'a');
        recorder
            .draw_layers(std::iter::once(DrawCell::new(pos, &tile)))
            .unwrap();

        let taken = recorder.take_frames();
        assert_eq!(taken.len(), 1);
        assert!(recorder.frames().is_empty());
    }

    #[test]
    fn forwards_to_inner_backend() {
        let mut recorder = FrameRecorder::new(Headless::new(10, 3));
        let (pos, tile) = cell(1, 1, 'z');
        recorder
            .draw_layers(std::iter::once(DrawCell::new(pos, &tile)))
            .unwrap();
        recorder.flush().unwrap();

        assert_eq!(recorder.inner().grid()[pos].glyph(), 'z');
    }

    #[test]
    fn a_handle_taken_before_a_move_still_reads_frames_captured_after_it() {
        let mut recorder = FrameRecorder::new(Headless::new(10, 3));
        let handle = recorder.handle();

        let (pos, tile) = cell(0, 0, 'a');
        recorder
            .draw_layers(std::iter::once(DrawCell::new(pos, &tile)))
            .unwrap();
        drop(recorder); // the value moves on (e.g. into `Terminal`/`run_on`); the handle survives.

        assert_eq!(handle.frames().len(), 1);
    }
}
