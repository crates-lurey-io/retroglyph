//! Stateful terminal lifecycle and double-buffering.
//!
//! [`Terminal::present`] compares the current frame against the previous one and forwards only
//! the changed cells to the [`Backend`]. Pixel-based backends request full frames instead (see
//! [`Output::needs_full_frame`]) because sub-cell offsets can leave orphaned pixels from the
//! previous frame.

use crate::backend::{Backend, CursorStyle, Output};
use crate::event::Event;
use crate::grid::{Grid, Pos, Rect, Size};
use crate::surface::Surface;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::time::Duration;
use ixy::HasSize;

/// A double-buffered terminal generic over a [`Backend`].
///
/// Owns the current and previous frame grids and the backend's lifecycle (resize, present,
/// events). Drawing itself goes entirely through [`Surface`]: see [`draw`](Self::draw) for the
/// common case (draw a frame, then present it) and [`surface`](Self::surface) for manual control
/// over presenting.
///
/// # Out-of-bounds drawing
///
/// [`Surface`] clips any write that falls outside its own area rather than panicking; see
/// [`Surface`]'s own "out-of-bounds drawing" documentation.
///
/// # Examples
///
/// ```
/// use retroglyph_core::backend::Headless;
/// use retroglyph_core::{Color, Terminal};
///
/// let mut term = Terminal::new(Headless::new(20, 5));
/// term.draw(|surface| {
///     surface.put((2, 1), '@', retroglyph_core::Style::new().fg(Color::GREEN));
/// })
/// .unwrap();
/// ```
pub struct Terminal<B: Backend> {
    current: Grid,
    previous: Grid,
    /// Single-layer scratch buffers used only when the backend does not
    /// composite layers itself. `present` flattens `current` into
    /// `flattened_current`, diffs it against `flattened_previous`, and sends the
    /// result. Unused (but allocated) for compositing backends.
    flattened_current: Grid,
    flattened_previous: Grid,
    backend: B,
    /// Events waiting to be handed out by [`poll`](Self::poll) before the backend is polled
    /// again: the [`has_input`](Self::has_input)/[`wait_for_input`](Self::wait_for_input) lookahead
    /// buffers a single event here, and [`requeue_events`](Self::requeue_events) lets a wrapper
    /// (e.g. `PerfOverlayApp`) hand back events it drained but didn't consume, entirely through
    /// `Terminal` itself rather than a backend-specific input path.
    queued_events: VecDeque<Event>,
    /// `true` when the flatten buffers no longer reflect the last frame sent to
    /// the backend (because the single-layer fast path bypassed them). The next
    /// multi-layer present clears `flattened_previous` first so it does a full
    /// redraw instead of diffing against stale data.
    flattened_stale: bool,
    /// Incremented every time [`present`](Self::present) is called.
    ///
    /// Lets embedding drivers detect whether application code already presented during a frame,
    /// so they can skip a redundant driver-side present.
    present_count: u64,
    /// Layers marked by [`retain_layer`](Self::retain_layer) to be re-synced from `previous`
    /// instead of diffed as a real redraw on the next [`present`](Self::present).
    ///
    /// Indexed by layer id; `retained_layers[id]` is `true` if that layer's `previous` content
    /// should be copied into `current` before diffing. Reset to all `false` once consumed at the
    /// start of `present` (it's a one-shot opt-in, not a sticky mode) and on
    /// [`resize`](Self::resize).
    retained_layers: Vec<bool>,
}

impl<B: Backend> Terminal<B> {
    /// Create a terminal with the given backend.
    /// Grid dimensions are queried from the backend.
    #[must_use]
    pub fn new(backend: B) -> Self {
        let size = backend.size();
        let current = Grid::new(size.width(), size.height());
        let previous = Grid::new(size.width(), size.height());
        let flattened_current = Grid::new(size.width(), size.height());
        let flattened_previous = Grid::new(size.width(), size.height());
        Self {
            current,
            previous,
            flattened_current,
            flattened_previous,
            backend,
            queued_events: VecDeque::new(),
            flattened_stale: false,
            present_count: 0,
            retained_layers: Vec::new(),
        }
    }

    /// Draws one frame: `f` gets a [`Surface`] scoped to the whole terminal on layer 0, then the
    /// frame is presented (see [`present`](Self::present)) once `f` returns.
    ///
    /// This is the common entry point for drawing: a caller that draws every frame regardless of
    /// whether anything changed calls this once per frame. A caller that only wants to redraw
    /// when its own state changed should gate the call to `draw` itself (e.g. `if
    /// state.changed() { term.draw(|s| render(s, &state))?; }`) rather than rely on `draw`/
    /// [`present`](Self::present) to no-op. Unlike some earlier revisions of this API, presenting
    /// is unconditional here.
    ///
    /// # Errors
    ///
    /// Propagates errors from [`present`](Self::present).
    pub fn draw(&mut self, f: impl FnOnce(&mut Surface<'_>)) -> Result<(), <B as Output>::Error> {
        let area = self.area();
        let mut surface = Surface::new(&mut self.current, area, 0);
        f(&mut surface);
        self.present()
    }

    /// A [`Surface`] scoped to the whole terminal on layer 0, for manual control over presenting
    /// (e.g. partial updates spread across several calls, or conditionally skipping a present).
    /// Most callers want [`draw`](Self::draw) instead.
    pub const fn surface(&mut self) -> Surface<'_> {
        let area = self.area();
        Surface::new(&mut self.current, area, 0)
    }

    /// Returns the current grid dimensions.
    #[must_use]
    pub const fn size(&self) -> Size {
        self.current.size()
    }

    /// Returns the full drawing surface as a [`Rect`] at the origin.
    ///
    /// Equivalent to `Rect::new(0, 0, width, height)`. Handy for passing the
    /// whole terminal to layout helpers or region-based drawing.
    #[must_use]
    pub const fn area(&self) -> Rect {
        self.current.rect()
    }

    /// Resize both grids to `width` × `height` cells.
    ///
    /// Content within the overlapping region is preserved in the current grid.
    /// The previous grid is cleared so the next [`present`](Self::present) redraws
    /// the entire new surface rather than diffing stale data.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.current.resize(width, height);
        self.previous.resize(width, height);
        self.flattened_current.resize(width, height);
        self.flattened_previous.resize(width, height);
        // Clearing previous forces a full redraw next present(), ensuring no
        // stale cells bleed into the resized layout.
        self.previous.clear_all();
        self.flattened_previous.clear_all();
        // Defensive: `resize` already clears `previous` unconditionally, so the next `present`
        // would just copy empty content forward for a still-marked layer. Dropping pending
        // retention here too keeps that a non-event rather than relying on it.
        self.retained_layers.clear();
        self.backend.resize(Size::new(width, height));
    }

    /// Show or hide the cursor.
    ///
    /// Forwards to [`Cursor::set_cursor_visible`](crate::backend::Cursor::set_cursor_visible) on
    /// the backend.
    pub fn set_cursor_visible(&mut self, visible: bool) {
        self.backend.set_cursor_visible(visible);
    }

    /// Move the cursor to a position.
    ///
    /// Forwards to [`Cursor::set_cursor_position`](crate::backend::Cursor::set_cursor_position)
    /// on the backend.
    pub fn set_cursor_position(&mut self, position: Pos) {
        self.backend.set_cursor_position(position);
    }

    /// Set the cursor's shape (and blink behavior).
    ///
    /// Forwards to [`Cursor::set_cursor_style`](crate::backend::Cursor::set_cursor_style) on the
    /// backend.
    pub fn set_cursor_style(&mut self, style: CursorStyle) {
        self.backend.set_cursor_style(style);
    }

    /// Returns a reference to the current grid.
    #[must_use]
    pub const fn grid(&self) -> &Grid {
        &self.current
    }

    /// Returns a mutable reference to the current grid, with no clipping or layer scoping.
    ///
    /// Escape hatch for whole-grid operations that don't fit [`Surface`]'s clipped,
    /// single-layer model (e.g. [`Grid::blit`]). Most drawing should go through
    /// [`draw`](Self::draw)/[`surface`](Self::surface) instead.
    pub const fn grid_mut(&mut self) -> &mut Grid {
        &mut self.current
    }

    /// Returns a reference to the backend.
    #[must_use]
    pub const fn backend(&self) -> &B {
        &self.backend
    }

    /// Returns a mutable reference to the backend.
    pub const fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    /// Number of times [`present`](Self::present) has been called so far.
    ///
    /// Wraps on overflow; intended for detecting whether `present` was called *at all* between two
    /// points in time (compare a saved count against the current one), not as a precise total.
    /// Embedding drivers (e.g. `retroglyph-window`'s windowed drivers) use this to decide whether
    /// application code already presented during a frame, so they can skip a redundant
    /// driver-side present.
    #[must_use]
    pub const fn present_count(&self) -> u64 {
        self.present_count
    }

    /// Marks `layer` so the next [`present`](Self::present) treats it as unchanged instead of
    /// requiring the app to have redrawn it: `present` copies `layer`'s last-presented content
    /// back into `current` before diffing, so the diff (and thus the backend) sees no change on
    /// it, whatever the app did or didn't draw into it this frame.
    ///
    /// Call this before [`draw`](Self::draw)/[`present`](Self::present) on a frame where a
    /// layer's content is known not to have changed (e.g. the camera didn't move since the last
    /// frame, so a cached map layer is still correct) and skip drawing it that frame. This is
    /// the actual point of the method: [`present`](Self::present)'s diff already keeps the
    /// *backend* from re-receiving unchanged cells, but the *app* still has to regenerate them
    /// every frame to produce a buffer worth diffing. Marking a layer retained lets the app skip
    /// that regeneration too, at the cost of a per-cell copy handled internally (a flat blit, far
    /// cheaper than most real content generation).
    ///
    /// This is a one-shot opt-in, not a sticky mode: it only affects the very next `present`, so
    /// a caller that wants a layer retained for several frames in a row must call this again
    /// before each of them. [`resize`](Self::resize) also clears any pending retention.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::backend::Headless;
    /// use retroglyph_core::{Layer, Terminal};
    ///
    /// let mut term = Terminal::new(Headless::new(10, 5));
    /// let camera_moved = false;
    ///
    /// if camera_moved {
    ///     term.draw(|s| s.on_tier(Layer::World).print((0, 0), "map", Default::default()))
    ///         .unwrap();
    /// } else {
    ///     // The camera didn't move this frame: skip regenerating the map layer.
    ///     term.retain_layer(Layer::World);
    ///     term.draw(|s| s.on_tier(Layer::Hud).print((0, 1), "HP: 10", Default::default()))
    ///         .unwrap();
    /// }
    /// ```
    pub fn retain_layer(&mut self, layer: impl Into<u8>) {
        let idx = usize::from(layer.into());
        if self.retained_layers.len() <= idx {
            self.retained_layers.resize(idx + 1, false);
        }
        self.retained_layers[idx] = true;
    }

    /// Present the current frame: computes the diff against the previous frame, sends changed
    /// cells to the backend, flushes, then swaps buffers. Always presents unconditionally, even
    /// if nothing was drawn since the last call; most callers want [`draw`](Self::draw) instead
    /// of calling this directly.
    ///
    /// When the backend requires a full frame (see
    /// [`crate::Output::needs_full_frame`]), all cells from every allocated layer are
    /// sent rather than just the diff, so pixel-based backends can clear and
    /// redraw to avoid orphaned pixels from sub-cell offsets.
    ///
    /// After a present, the new current buffer is cleared so the next frame starts empty.
    /// Callers should not draw into a frame and skip presenting it: the next [`draw`](Self::draw)
    /// call starts from an empty grid regardless.
    ///
    /// # Immediate mode
    ///
    /// This is an immediate-mode API (the same trade [ratatui] makes): the
    /// current buffer is wiped after every present, so each frame must redraw
    /// its entire scene from scratch by default. [`retain_layer`](Self::retain_layer) is the
    /// escape hatch: it makes one specific layer's last-presented content stand in for a redraw,
    /// so the app can skip regenerating it. The diff only bounds what is sent to the backend
    /// (terminal or pixel I/O); it does not bound the CPU cost of your redraw, except for a
    /// layer marked via `retain_layer`.
    ///
    /// [ratatui]: https://docs.rs/ratatui
    ///
    /// # Errors
    ///
    /// Propagates errors from the backend's [`draw_layers`](crate::Output::draw_layers) or
    /// [`flush`](crate::Output::flush) operations. Either failure returns before the
    /// current/previous buffers are swapped, so the cells from the failed frame stay marked
    /// dirty in `previous` and are resent the next time `present` succeeds. `current` is still
    /// cleared, same as on success, so the caller doesn't need to redraw anything to recover:
    /// just call `draw`/`present` again, and the next frame starts from an empty grid like any
    /// other.
    pub fn present(&mut self) -> Result<(), <B as Output>::Error> {
        self.present_count = self.present_count.wrapping_add(1);
        if self.retained_layers.iter().any(|&retained| retained) {
            // Overwrite each retained layer's (empty, never-drawn-this-frame) content in
            // `current` with `previous`'s, so the diff below finds no change on it: the backend
            // gets nothing to redraw, and the copy (a flat per-layer clone) is far cheaper than
            // whatever the app would have spent regenerating identical content. See
            // `retain_layer`'s doc for why this has to run before the diff rather than skip the
            // post-swap clear: `current` and `previous` alternate buffers every present, so
            // anything short of re-syncing from the authoritative `previous` here would desync
            // them again after a second consecutive retained frame.
            //
            // Uses `copy_layer_from` rather than `blit`: `blit` is a clipping/positioning copy
            // that degrades multi-cell spans to their text fallback and treats empty tiles as
            // transparent (an overlay, not a replacement), both wrong here, since a retained
            // layer is copied whole, at the same geometry, and must be indistinguishable from
            // what was presented last frame (retroglyph#955).
            for (id, &retained) in self.retained_layers.iter().enumerate() {
                if retained {
                    #[allow(clippy::cast_possible_truncation)]
                    let id = id as u8;
                    self.current.copy_layer_from(id, &self.previous);
                }
            }
            for retained in &mut self.retained_layers {
                *retained = false;
            }
        }
        let mut swap_flattened = false;
        // The fallible part is scoped to this closure so both the success and error paths
        // below can clear `current` before returning: `current` is presentation-buffer state
        // for the *next* frame, not part of what makes the resend-on-retry behavior work (that
        // lives entirely in `previous`/`flattened_previous`, left untouched here), so clearing
        // it is safe unconditionally and keeps immediate mode's "next `draw` starts empty"
        // contract true even after a failed present.
        let result = (|| -> Result<(), <B as Output>::Error> {
            if self.backend.composites_layers() {
                // Pixel/GPU backends composite the raw layered stream themselves.
                if self.backend.needs_full_frame() {
                    let all = self.current.layers();
                    self.backend.draw_layers(all)?;
                } else {
                    let diff = self.current.diff(&self.previous);
                    self.backend.draw_layers(diff)?;
                }
            } else if self.current.max_layer() == 0 && self.previous.max_layer() == 0 {
                // Fast path: only layer 0 is in play, so flattening would be an exact
                // copy of `current`. Diff the real grids directly and skip the
                // flatten buffers entirely.
                let diff = self.current.diff(&self.previous);
                self.backend.draw_layers(diff)?;
                self.flattened_stale = true;
            } else {
                // Cell backends receive a pre-flattened, single-layer diff so layers
                // 1+ appear everywhere, not just on pixel backends.
                if self.flattened_stale {
                    // The previous frame used the fast path, so `flattened_previous`
                    // is stale. Clear it to force a full redraw this frame.
                    self.flattened_previous.clear_all();
                    self.flattened_stale = false;
                }
                self.current.flatten_into(&mut self.flattened_current);
                let diff = self.flattened_current.diff(&self.flattened_previous);
                self.backend.draw_layers(diff)?;
                swap_flattened = true;
            }
            self.backend.flush()
        })();
        if let Err(err) = result {
            // `current` is cleared even on failure so the next frame still starts from an
            // empty grid; only the swap below is skipped. `previous`/`flattened_previous`
            // still hold the last confirmed frame, so the next `present`'s diff against them
            // resends the cells that never actually reached the backend instead of silently
            // dropping them.
            self.current.clear_all();
            return Err(err);
        }
        // Both swaps happen only after `flush` succeeds, for the same reason described above.
        if swap_flattened {
            core::mem::swap(&mut self.flattened_current, &mut self.flattened_previous);
        }
        core::mem::swap(&mut self.current, &mut self.previous);
        self.current.clear_all();
        Ok(())
    }

    /// Polls for an input event, waiting up to `timeout`.
    ///
    /// If an event was previously buffered by [`has_input`](Self::has_input), it is
    /// returned immediately. Otherwise, the backend is polled for a new event.
    ///
    /// [`Event::Resize`] events are automatically applied: both grids are resized
    /// before the event is returned to the caller, so the game loop can immediately
    /// redraw at the new size.
    pub fn poll(&mut self, timeout: Duration) -> Option<Event> {
        let event = self
            .queued_events
            .pop_front()
            .or_else(|| self.backend.poll_event(timeout))?;
        if let Event::Resize(w, h) = event {
            self.resize(w, h);
        }
        Some(event)
    }

    /// Hands `events` back to this terminal's own queue, in order, so a later
    /// [`poll`](Self::poll)/[`drain_events`](Self::drain_events)/[`drain_events_into`](Self::drain_events_into)
    /// call yields them again before the backend is polled for anything new.
    ///
    /// This is the supported way for a wrapper that drains events to intercept some of them
    /// (e.g. `retroglyph-widgets`' `PerfOverlayApp` filtering out its own toggle key) to give
    /// the rest back: it goes through `Terminal`'s own queue, never a backend-specific input
    /// path, so it works identically on every [`Backend`] regardless of how (or whether) that
    /// backend implements [`Input::push_event`](crate::backend::Input::push_event).
    pub fn requeue_events(&mut self, events: impl IntoIterator<Item = Event>) {
        self.queued_events.extend(events);
    }

    /// Reads an input event, blocking indefinitely until one is available.
    ///
    /// Only call this on backends that genuinely block (e.g. crossterm, window). Backends
    /// that never block (e.g. [`Headless`](crate::backend::Headless), which returns
    /// immediately regardless of timeout) will panic here once their event queue is
    /// empty; use [`poll`](Self::poll) or [`drain_events`](Self::drain_events) instead if
    /// that is a possibility.
    ///
    /// # Panics
    ///
    /// Panics if the backend's [`poll_event`](crate::Input::poll_event) returns
    /// `None` even with an unbounded timeout.
    pub fn read_blocking(&mut self) -> Event {
        self.poll(Duration::MAX)
            .expect("read_blocking() called but no events available")
    }

    /// Drains all available events without blocking.
    ///
    /// Returns an iterator that yields every pending event: the internal queued event
    /// followed by all events buffered in the backend. The iterator polls the backend
    /// with zero timeout repeatedly until `None` is returned.
    ///
    /// This is needed for frame-based game loops (e.g. software backend + WASM, where
    /// frames are gated by `requestAnimationFrame`). Multiple keypresses can arrive
    /// between frames; draining all of them ensures accumulated input doesn't replay in
    /// slow motion.
    ///
    /// Crossterm and headless backends can also use this, but the single-event `poll`
    /// pattern works for them because their loops aren't frame-capped.
    ///
    /// # One-shot semantics
    ///
    /// The returned iterator borrows `self` and drains the queue as it is consumed: the
    /// first caller to iterate it gets every pending event, and a second, independent
    /// call to `drain_events` afterward gets nothing. If more than one subsystem needs
    /// this frame's input (e.g. persistent chrome and an active screen), collect once
    /// and share the collected events by reference, or use
    /// [`drain_events_into`](Self::drain_events_into) to drain into a reusable buffer
    /// instead of allocating a fresh `Vec` every frame.
    pub fn drain_events(&mut self) -> impl Iterator<Item = Event> + use<'_, B> {
        struct DrainEvents<'a, B: Backend> {
            terminal: &'a mut Terminal<B>,
        }

        impl<B: Backend> Iterator for DrainEvents<'_, B> {
            type Item = Event;

            fn next(&mut self) -> Option<Event> {
                self.terminal.poll(Duration::ZERO)
            }
        }

        impl<B: Backend> core::iter::FusedIterator for DrainEvents<'_, B> {}

        DrainEvents { terminal: self }
    }

    /// Drains all available events without blocking, appending them to `buf`.
    ///
    /// `buf` is cleared first, then filled with every pending event in the same order
    /// [`drain_events`](Self::drain_events) would yield them. Unlike `drain_events`, the
    /// borrow of `self` ends when this call returns, so the terminal is free to draw or
    /// be polled again afterward, and the caller can hand `buf` to multiple consumers by
    /// shared reference without materializing a new `Vec` every frame.
    ///
    /// This is the same shape as `std::io::Read::read_to_end`: allocate the buffer once
    /// at startup, reuse it every frame, and let this method manage its contents.
    pub fn drain_events_into(&mut self, buf: &mut Vec<Event>) {
        buf.clear();
        while let Some(event) = self.poll(Duration::ZERO) {
            buf.push(event);
        }
    }

    /// Checks if a pending input event is available without blocking.
    ///
    /// If an event is already buffered, returns `true`. Otherwise, polls the backend
    /// with zero timeout. If the backend returns an event, it is stored in the internal
    /// buffer and `true` is returned; otherwise, returns `false`.
    pub fn has_input(&mut self) -> bool {
        self.wait_for_input(Duration::ZERO)
    }

    /// Blocks until an input event is available or `timeout` elapses, without consuming it.
    ///
    /// Like [`has_input`](Self::has_input), a discovered event is buffered internally so a
    /// subsequent [`poll`](Self::poll), [`has_input`](Self::has_input), or
    /// [`drain_events`](Self::drain_events) call still observes it: this method only answers
    /// "did something happen", it never hands the event to the caller. That's what lets a driver
    /// loop block between frames without stealing the event the app's own `update` reads; see
    /// [`run_blocking_with`](crate::run_blocking_with)'s use of this for [`Flow::Idle`](crate::Flow::Idle).
    ///
    /// Returns `true` if an event arrived within `timeout`, `false` if `timeout` elapsed with
    /// nothing pending. Pass [`Duration::MAX`] to block indefinitely.
    ///
    /// Backends that never block (e.g. [`Headless`](crate::backend::Headless), which returns
    /// immediately regardless of `timeout`; see [`Input::poll_event`](crate::backend::Input::poll_event))
    /// return promptly rather than actually waiting; this method is a real wait only on
    /// backends that genuinely block (crossterm, window).
    pub fn wait_for_input(&mut self, timeout: Duration) -> bool {
        if !self.queued_events.is_empty() {
            return true;
        }
        let Some(event) = self.backend.poll_event(timeout) else {
            return false;
        };
        if let Event::Resize(w, h) = event {
            self.resize(w, h);
        }
        self.queued_events.push_back(event);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{Cursor, DrawCell, Headless, Input, Output};
    use crate::color::Color;
    use crate::color::Style;
    use crate::tile::Tile;

    /// Wraps [`Headless`] and fails the next [`flush`](Output::flush) call once, then
    /// forwards everything (including the failed call's `draw_layers`, which already
    /// reached the inner backend) as normal. Used to exercise `present`'s documented
    /// error-recovery contract: a failed `flush` must leave the frame's cells marked dirty
    /// so they are resent on the next successful `present`.
    ///
    /// `std`-only: its `Output::Error` is `std::io::Error`, purely as a convenient stand-in
    /// error type for this test.
    #[cfg(feature = "std")]
    struct FlushOnceFailing {
        inner: Headless,
        fail_next_flush: bool,
        /// Number of cells received by the most recent `draw_layers` call, so tests can
        /// tell whether a frame's diff was actually sent, independent of `Headless`'s
        /// applied grid (which a real backend might not update until well after `flush`).
        last_draw_len: usize,
    }

    #[cfg(feature = "std")]
    impl FlushOnceFailing {
        fn new(width: u16, height: u16) -> Self {
            Self {
                inner: Headless::new(width, height),
                fail_next_flush: false,
                last_draw_len: 0,
            }
        }
    }

    #[cfg(feature = "std")]
    impl Output for FlushOnceFailing {
        type Error = std::io::Error;

        fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
        where
            I: Iterator<Item = DrawCell<'a>>,
        {
            let content: Vec<_> = content.collect();
            self.last_draw_len = content.len();
            // Infallible in `Headless`; map its error type to ours to keep the wrapper's
            // error type consistent across all `Output` methods.
            self.inner
                .draw_layers(content.into_iter())
                .map_err(|e| match e {})
        }

        fn flush(&mut self) -> Result<(), Self::Error> {
            if self.fail_next_flush {
                self.fail_next_flush = false;
                return Err(std::io::Error::other("simulated flush failure"));
            }
            self.inner.flush().map_err(|e| match e {})
        }

        fn size(&self) -> Size {
            self.inner.size()
        }

        fn clear(&mut self) -> Result<(), Self::Error> {
            self.inner.clear().map_err(|e| match e {})
        }
    }

    #[cfg(feature = "std")]
    impl Input for FlushOnceFailing {
        fn poll_event(&mut self, timeout: Duration) -> Option<Event> {
            self.inner.poll_event(timeout)
        }
    }

    #[cfg(feature = "std")]
    impl Cursor for FlushOnceFailing {}

    #[test]
    fn test_terminal_grid_mut() {
        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);

        assert_eq!(terminal.grid()[Pos::new(0, 0)].glyph(), ' ');

        terminal
            .grid_mut()
            .put_tile(0, (0, 0), Tile::new('X', Style::default()));

        assert_eq!(terminal.grid()[Pos::new(0, 0)].glyph(), 'X');
    }

    #[test]
    fn test_terminal_poll_and_read() {
        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);

        assert_eq!(terminal.poll(Duration::ZERO), None);

        terminal.backend_mut().push_event(Event::Close);
        assert_eq!(terminal.poll(Duration::ZERO), Some(Event::Close));

        terminal.backend_mut().push_event(Event::Resize(80, 25));
        assert_eq!(terminal.read_blocking(), Event::Resize(80, 25));
    }

    #[test]
    fn test_terminal_has_input() {
        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);

        assert!(!terminal.has_input());

        terminal.backend_mut().push_event(Event::Close);
        assert!(terminal.has_input());
        assert!(terminal.has_input()); // Repeated calls should still be true

        // Read/Poll should retrieve the buffered event
        assert_eq!(terminal.poll(Duration::ZERO), Some(Event::Close));

        // After taking, it should be false again
        assert!(!terminal.has_input());
    }

    #[test]
    fn test_terminal_drain_events_into() {
        use alloc::vec;

        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);

        terminal.backend_mut().push_event(Event::Close);
        terminal.backend_mut().push_event(Event::Resize(80, 25));

        let mut buf = vec![Event::Close]; // pre-existing contents must be cleared
        terminal.drain_events_into(&mut buf);
        assert_eq!(buf, [Event::Close, Event::Resize(80, 25)]);

        // The borrow ends at the call, so the terminal is immediately usable again.
        assert_eq!(terminal.area(), Rect::new(0, 0, 80, 25));

        // Draining again with nothing pending clears the buffer.
        terminal.drain_events_into(&mut buf);
        assert!(buf.is_empty());
    }

    #[test]
    fn test_terminal_requeue_events_replays_before_the_backend() {
        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);

        // Push directly to the backend first, so a requeued event has to come out ahead of it.
        terminal.backend_mut().push_event(Event::Close);
        terminal.requeue_events([Event::Resize(80, 25), Event::Resize(1, 1)]);

        assert_eq!(terminal.poll(Duration::ZERO), Some(Event::Resize(80, 25)));
        assert_eq!(terminal.poll(Duration::ZERO), Some(Event::Resize(1, 1)));
        assert_eq!(terminal.poll(Duration::ZERO), Some(Event::Close));
        assert_eq!(terminal.poll(Duration::ZERO), None);
    }

    #[test]
    fn test_terminal_wait_for_input_buffers_the_event_instead_of_consuming_it() {
        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);

        // Nothing queued: returns false rather than blocking (`Headless` ignores the timeout).
        assert!(!terminal.wait_for_input(Duration::from_millis(1)));

        terminal.backend_mut().push_event(Event::Close);
        assert!(terminal.wait_for_input(Duration::MAX));
        // Repeated calls stay true: the event was buffered, not handed out and lost.
        assert!(terminal.wait_for_input(Duration::MAX));
        assert!(terminal.has_input());

        // A caller reading through the normal input API (as an app's own `update` would) still
        // observes the exact event `wait_for_input` woke up for.
        assert_eq!(terminal.poll(Duration::ZERO), Some(Event::Close));
        assert!(!terminal.has_input());
    }

    #[test]
    fn test_terminal_wait_for_input_applies_pending_resize() {
        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);

        terminal.backend_mut().push_event(Event::Resize(4, 2));
        assert!(terminal.wait_for_input(Duration::MAX));
        // The grid resizes immediately, same as `poll`, even though the event is still buffered
        // rather than consumed.
        assert_eq!(terminal.size(), Size::new(4, 2));
        assert_eq!(terminal.poll(Duration::ZERO), Some(Event::Resize(4, 2)));
    }

    #[test]
    #[should_panic(expected = "read_blocking() called but no events available")]
    fn test_terminal_read_panic() {
        let backend = Headless::new(10, 10);
        let mut terminal = Terminal::new(backend);
        let _ = terminal.read_blocking();
    }

    #[test]
    fn test_draw_composites_layers_for_cell_backend() {
        // A cell backend (Headless) must see layers 1+ composited, not
        // dropped. Terrain on layer 0, entity on layer 1.
        let mut term = Terminal::new(Headless::new(3, 1));
        term.draw(|s| {
            s.put((0, 0), '.', Style::default());
            s.put((1, 0), '.', Style::default());
            s.on_layer(1).put((1, 0), '@', Style::default());
        })
        .expect("draw failed");
        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), '.');
        // Layer 1's glyph wins at (1, 0).
        assert_eq!(term.backend().grid()[Pos::new(1, 0)].glyph(), '@');
    }

    /// A cell backend with `needs_full_frame() == true` and the default `composites_layers()`.
    ///
    /// No real backend in this workspace uses that combination; this pins the interaction
    /// `Output::draw_layers`'s docs describe (retroglyph#763): a `true` `needs_full_frame` only
    /// takes effect inside `composites_layers`'s branch of `present`, so this combination gets
    /// the same diff-only stream as `needs_full_frame() == false` would, not the "all cells,
    /// every call" this method's own doc otherwise promises unconditionally.
    struct NeedsFullFrameWithoutCompositing {
        inner: Headless,
        last_draw_len: usize,
    }

    impl Output for NeedsFullFrameWithoutCompositing {
        type Error = core::convert::Infallible;

        fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
        where
            I: Iterator<Item = DrawCell<'a>>,
        {
            let content: Vec<_> = content.collect();
            self.last_draw_len = content.len();
            self.inner.draw_layers(content.into_iter())
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

        fn needs_full_frame(&self) -> bool {
            true
        }

        // `composites_layers` left at its default `false`: exactly the combination the docs on
        // `Output::draw_layers`/`Output::needs_full_frame` now call out.
    }

    impl Input for NeedsFullFrameWithoutCompositing {
        fn poll_event(&mut self, timeout: Duration) -> Option<Event> {
            self.inner.poll_event(timeout)
        }
    }

    impl Cursor for NeedsFullFrameWithoutCompositing {}

    #[test]
    fn needs_full_frame_without_composites_layers_still_gets_only_the_diff() {
        let mut term = Terminal::new(NeedsFullFrameWithoutCompositing {
            inner: Headless::new(3, 1),
            last_draw_len: 0,
        });
        term.draw(|s| {
            s.put((0, 0), 'a', Style::default());
            s.put((1, 0), 'b', Style::default());
        })
        .expect("draw failed");
        assert_eq!(
            term.backend().last_draw_len,
            2,
            "first frame: diff and full-frame agree (everything is new)"
        );

        // Second, identical frame: a backend for which `needs_full_frame` actually took effect
        // would still receive both cells here. This one, per the documented caveat, gets the
        // diff instead, which is empty, since nothing changed.
        term.draw(|s| {
            s.put((0, 0), 'a', Style::default());
            s.put((1, 0), 'b', Style::default());
        })
        .expect("draw failed");
        assert_eq!(
            term.backend().last_draw_len,
            0,
            "needs_full_frame() alone (without composites_layers()) does not widen present's \
             diff-only dispatch; see Output::draw_layers's docs (retroglyph#763)"
        );
    }

    #[test]
    fn test_draw_explicit_space_on_higher_layer_erases_and_sets_bg() {
        // An explicit space on a higher layer is opaque: it overwrites the
        // glyph beneath (erase) and applies its background. This is the
        // deliberate consequence of the explicit-EMPTY transparency model.
        let mut term = Terminal::new(Headless::new(2, 1));
        term.draw(|s| {
            s.put((0, 0), 'x', Style::default());
            s.on_layer(1).put((0, 0), ' ', Style::new().bg(Color::RED));
        })
        .expect("draw failed");
        let cell = term.backend().grid()[Pos::new(0, 0)];
        assert_eq!(cell.glyph(), ' ');
        assert_eq!(cell.style().background(), Color::RED);
    }

    #[test]
    fn test_draw_single_layer_fast_path_matches_backend() {
        // Only layer 0 is ever touched: the fast path must still deliver the
        // correct cells to a cell backend across multiple frames.
        let mut term = Terminal::new(Headless::new(3, 1));
        term.draw(|s| s.put((0, 0), 'a', Style::default()))
            .expect("draw failed");
        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), 'a');

        // Immediate mode: redraw 'a' and add 'c'.
        term.draw(|s| {
            s.put((0, 0), 'a', Style::default());
            s.put((2, 0), 'c', Style::default());
        })
        .expect("draw failed");
        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), 'a');
        assert_eq!(term.backend().grid()[Pos::new(2, 0)].glyph(), 'c');

        // A cell that is not redrawn is erased (immediate mode).
        term.draw(|s| s.put((0, 0), 'a', Style::default()))
            .expect("draw failed");
        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), 'a');
        assert_eq!(term.backend().grid()[Pos::new(2, 0)].glyph(), ' ');
    }

    #[test]
    fn test_present_transition_single_to_multi_layer() {
        // Start single-layer (fast path), then introduce layer 1. The frame
        // that adds the layer must composite correctly despite the fast path
        // having bypassed the flatten buffers.
        let mut term = Terminal::new(Headless::new(2, 1));
        term.draw(|s| {
            s.put((0, 0), '.', Style::default());
            s.put((1, 0), '.', Style::default());
        })
        .expect("draw failed");

        term.draw(|s| {
            s.put((0, 0), '.', Style::default());
            s.put((1, 0), '.', Style::default());
            s.on_layer(1).put((1, 0), '@', Style::default());
        })
        .expect("draw failed");
        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), '.');
        assert_eq!(term.backend().grid()[Pos::new(1, 0)].glyph(), '@');
    }

    #[cfg(feature = "std")]
    #[test]
    fn present_resends_cells_after_a_failed_flush_on_the_multi_layer_path() {
        // Two-layer terminal so `present` takes the flatten-buffer path (not the
        // single-layer fast path, which already handled this correctly).
        let mut term = Terminal::new(FlushOnceFailing::new(2, 1));

        term.backend_mut().fail_next_flush = true;
        let result = term.draw(|s| {
            s.put((0, 0), 'a', Style::default());
            s.on_layer(1).put((1, 0), 'b', Style::default());
        });
        assert!(result.is_err(), "flush was expected to fail this frame");
        assert_eq!(
            term.backend().last_draw_len,
            2,
            "the failed frame's diff should still have been sent to draw_layers"
        );

        // Same content, flush succeeds this time. If the flatten buffers had already been
        // swapped on the failed attempt, this diff would see "no change" and send nothing.
        term.draw(|s| {
            s.put((0, 0), 'a', Style::default());
            s.on_layer(1).put((1, 0), 'b', Style::default());
        })
        .expect("draw failed");
        assert_eq!(
            term.backend().last_draw_len,
            2,
            "both cells must be resent since neither ever reached the screen"
        );
        assert_eq!(term.backend().inner.grid()[Pos::new(0, 0)].glyph(), 'a');
        assert_eq!(term.backend().inner.grid()[Pos::new(1, 0)].glyph(), 'b');
    }

    #[cfg(feature = "std")]
    #[test]
    fn present_failure_does_not_leak_the_failed_frames_content_into_the_next_frame() {
        // Single-layer terminal so `present` takes the fast path, matching the issue's repro.
        let mut term = Terminal::new(FlushOnceFailing::new(3, 1));

        term.backend_mut().fail_next_flush = true;
        let result = term.draw(|s| s.put((2, 0), 'X', Style::default()));
        assert!(result.is_err(), "flush was expected to fail this frame");

        // Next frame redraws different content and never touches (2, 0). `previous` is still
        // empty (the swap was skipped), so if `current` had also been left holding the failed
        // frame's 'X' (the bug), the diff below would see (2, 0) as newly changed from empty
        // to 'X' and needlessly resend it, on top of the one cell this frame actually drew.
        term.draw(|s| s.put((0, 0), 'A', Style::default()))
            .expect("draw failed");
        assert_eq!(
            term.backend().last_draw_len,
            1,
            "only the redrawn cell should be sent; the failed frame's 'X' must not leak back in"
        );
        assert_eq!(term.backend().inner.grid()[Pos::new(0, 0)].glyph(), 'A');
    }

    #[test]
    fn test_present_untouched_higher_layer_is_transparent() {
        // A higher layer that was allocated but not written at this cell must
        // not disturb the lower layer's glyph or background.
        let mut term = Terminal::new(Headless::new(2, 1));
        term.draw(|s| {
            s.put((0, 0), 'x', Style::default());
            // Allocate layer 1 by writing elsewhere, leaving (0, 0) empty.
            s.on_layer(1).put((1, 0), 'y', Style::default());
        })
        .expect("draw failed");
        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), 'x');
    }

    #[test]
    fn test_terminal_size() {
        let term = Terminal::new(Headless::new(40, 20));
        assert_eq!(term.size(), Size::new(40, 20));
    }

    #[test]
    fn test_terminal_area() {
        let term = Terminal::new(Headless::new(40, 20));
        assert_eq!(term.area(), Rect::new(0, 0, 40, 20));
    }

    #[test]
    fn test_terminal_cursor_passthroughs_forward_to_backend() {
        let mut term = Terminal::new(Headless::new(10, 10));

        term.set_cursor_visible(true);
        assert!(term.backend().cursor_visible());

        term.set_cursor_position(Pos::new(3, 4));
        assert_eq!(term.backend().cursor_position(), Pos::new(3, 4));

        term.set_cursor_style(CursorStyle::SteadyBar);
        assert_eq!(term.backend().cursor_style(), CursorStyle::SteadyBar);
    }

    #[test]
    fn test_terminal_resize_changes_dimensions() {
        let mut term = Terminal::new(Headless::new(10, 10));
        term.resize(30, 15);
        assert_eq!(term.size(), Size::new(30, 15));
        assert_eq!(term.grid().width(), 30);
        assert_eq!(term.grid().height(), 15);
    }

    #[test]
    fn test_terminal_resize_preserves_current_content() {
        // Writes through `surface()` rather than `draw()`, so `current` is inspected before any
        // `present()` clears it: `draw()` always presents, which would swap this content out to
        // `previous` and clear the new `current` before the assertions below could see it.
        let mut term = Terminal::new(Headless::new(10, 10));
        term.surface().put((2, 2), 'X', Style::default());
        term.resize(20, 20);
        assert_eq!(term.grid()[Pos::new(2, 2)].glyph(), 'X');
        assert_eq!(term.grid()[Pos::new(15, 15)].glyph(), ' ');
    }

    #[test]
    fn test_terminal_resize_event_auto_applies() {
        let mut term = Terminal::new(Headless::new(10, 10));
        term.backend_mut().push_event(Event::Resize(80, 25));
        let event = term.poll(Duration::ZERO);
        assert_eq!(event, Some(Event::Resize(80, 25)));
        assert_eq!(term.size(), Size::new(80, 25));
    }

    #[test]
    fn test_terminal_resize_new_cells_accessible() {
        // Resize to a larger area, then draw into the newly created region.
        let mut term = Terminal::new(Headless::new(3, 3));
        term.draw(|s| s.put((0, 0), 'A', Style::default()))
            .expect("draw failed");

        term.resize(5, 5);

        // Draw into the expanded region and verify it reaches the backend.
        term.draw(|s| s.put((4, 4), 'B', Style::default()))
            .expect("draw failed");

        assert_eq!(term.backend().grid()[Pos::new(4, 4)].glyph(), 'B');
        // (0,0) was not redrawn this frame; backend retains 'A' from before resize.
        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), 'A');
    }

    // --- retain_layer ---

    #[test]
    fn test_retain_layer_skips_redraw_and_keeps_backend_content() {
        use crate::surface::Layer;

        let mut term = Terminal::new(Headless::new(3, 1));
        term.draw(|s| s.on_tier(Layer::World).put((0, 0), 'W', Style::default()))
            .expect("draw failed");

        // Retain `World`, then draw a frame that only touches `Hud`.
        term.retain_layer(Layer::World);
        term.draw(|s| s.on_tier(Layer::Hud).put((1, 0), 'H', Style::default()))
            .expect("draw failed");

        // `World` was never redrawn this frame, but the backend still shows it composited under
        // the new `Hud` cell: `present` re-synced it from `previous` before diffing.
        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), 'W');
        assert_eq!(term.backend().grid()[Pos::new(1, 0)].glyph(), 'H');
    }

    #[test]
    fn test_retain_layer_survives_repeated_retention_without_desync() {
        use crate::surface::Layer;

        // Retaining a layer for several frames in a row (never redrawing it) must not desync
        // `current`/`previous`: each present re-syncs the retained layer from `previous`, so the
        // backend keeps showing it correctly across any number of consecutive retains.
        let mut term = Terminal::new(Headless::new(3, 1));
        term.draw(|s| s.on_tier(Layer::World).put((0, 0), 'W', Style::default()))
            .expect("draw failed");

        for _ in 0..3 {
            term.retain_layer(Layer::World);
            term.draw(|_| {}).expect("draw failed");
            assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), 'W');
        }
    }

    #[test]
    fn test_retain_layer_is_one_shot() {
        use crate::surface::Layer;

        let mut term = Terminal::new(Headless::new(3, 1));
        term.draw(|s| s.on_tier(Layer::World).put((0, 0), 'W', Style::default()))
            .expect("draw failed");

        term.retain_layer(Layer::World);
        term.draw(|_| {}).expect("draw failed"); // Retained: the backend keeps showing 'W'.
        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), 'W');

        // Retention was one-shot: the next present, with `World` still undrawn, clears it from
        // the backend like any ordinary immediate-mode frame.
        term.draw(|_| {}).expect("draw failed");
        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), ' ');
    }

    #[test]
    fn test_retain_layer_preserves_multi_cell_span_flags() {
        use crate::surface::Layer;
        use crate::tile::TileFlags;

        // retroglyph#955: `retain_layer` used to re-sync via `Grid::blit`, whose clipping-copy
        // contract intentionally strips `SPAN_ANCHOR`/`SPAN_COVERED` and degrades a span to its
        // text fallback. That's wrong for a retained layer, which is copied whole at the same
        // geometry and must be indistinguishable from what was presented last frame.
        let mut term = Terminal::new(Headless::new(4, 2));
        term.draw(|s| {
            s.on_tier(Layer::World)
                .put_span((0, 0), &["Tr", "__"], Style::default())
                .unwrap();
        })
        .expect("draw failed");

        let anchor_flags = term.backend().grid()[Pos::new(0, 0)].flags();
        let covered_flags = term.backend().grid()[Pos::new(1, 0)].flags();
        assert!(anchor_flags.contains(TileFlags::SPAN_ANCHOR));
        assert!(covered_flags.contains(TileFlags::SPAN_COVERED));

        term.retain_layer(Layer::World);
        term.draw(|_| {}).expect("draw failed");

        // The span survived the retained present untouched: same glyphs, same span flags.
        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), 'T');
        assert_eq!(term.backend().grid()[Pos::new(1, 0)].glyph(), 'r');
        assert_eq!(term.backend().grid()[Pos::new(0, 0)].flags(), anchor_flags);
        assert_eq!(term.backend().grid()[Pos::new(1, 0)].flags(), covered_flags);
    }

    #[test]
    fn test_retain_layer_accepts_raw_u8_and_layer() {
        use crate::surface::Layer;

        // `retain_layer` takes `impl Into<u8>`, so a raw layer id and the `Layer` enum both work.
        let mut term = Terminal::new(Headless::new(1, 1));
        term.draw(|s| s.put((0, 0), 'A', Style::default()))
            .expect("draw failed");

        term.retain_layer(0u8);
        term.draw(|_| {}).expect("draw failed");
        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), 'A');

        term.retain_layer(Layer::World);
        term.draw(|_| {}).expect("draw failed");
        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), 'A');
    }

    #[test]
    fn test_retain_layer_never_drawn_is_a_no_op() {
        // `Grid::copy_layer_from`'s `None` arm, no-op branch: retaining a non-zero layer id
        // that has never been drawn to on either buffer leaves it unallocated on both sides.
        // Must not panic or grow the layer table for a layer id nobody ever wrote to.
        let mut term = Terminal::new(Headless::new(3, 1));
        term.retain_layer(5u8);
        term.draw(|_| {}).expect("draw failed");
        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), ' ');
    }

    #[test]
    fn test_retain_layer_deallocates_when_previous_lacks_it() {
        // `Grid::copy_layer_from`'s `None` arm, deallocating branch: `current` can carry a
        // non-zero layer allocated (but emptied by immediate mode) from an older frame while
        // `previous` never allocated it at all, if that layer went undrawn (and unretained) for
        // a frame in between. Retaining it then must clear it from `current`, not leave stale
        // allocation state behind. Layer 0 can't exercise this (always allocated on both
        // sides), so this writes directly to a non-zero raw layer id via `on_layer`.
        let mut term = Terminal::new(Headless::new(3, 1));
        term.draw(|s| s.on_layer(5).put((0, 0), 'W', Style::default()))
            .expect("draw failed");
        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), 'W');

        // Layer 5 goes undrawn and unretained: ordinary immediate-mode clearing puts `current`'s
        // (still allocated) layer 5 buffer back to empty, and this frame's diff sends that.
        term.draw(|s| s.on_layer(1).put((1, 0), 'H', Style::default()))
            .expect("draw failed");
        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), ' ');

        // Retaining layer 5 now must not resurrect stale content or panic, even though
        // `previous` (this frame's source) never allocated layer 5 at all.
        term.retain_layer(5u8);
        term.draw(|_| {}).expect("draw failed");
        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), ' ');
    }

    // --- unicode width ---

    #[test]
    fn test_put_wide_char_sets_continuation() {
        use crate::tile::TileFlags;

        let mut term = Terminal::new(Headless::new(10, 3));
        term.surface().put((0, 0), '\u{4e2d}', Style::default()); // '中', width 2
        assert_eq!(term.grid()[Pos::new(0, 0)].glyph(), '\u{4e2d}');
        // Both with and without `egc`: `put` (via `Grid::put_tile`, which is wide-char aware on
        // every feature combination) lays down an explicit spacer, flagged `WIDE_CHAR_SPACER`,
        // glyph space.
        assert!(
            term.grid()[Pos::new(1, 0)]
                .flags()
                .contains(TileFlags::WIDE_CHAR_SPACER)
        );
        assert_eq!(term.grid()[Pos::new(1, 0)].glyph(), ' ');
        assert_eq!(term.grid()[Pos::new(2, 0)].glyph(), ' '); // untouched
    }

    #[test]
    fn test_print_advances_by_char_width() {
        use crate::tile::TileFlags;

        let mut term = Terminal::new(Headless::new(10, 3));
        term.surface().print((0, 0), "\u{4e2d}x", Style::default()); // '中' (2) then 'x' at col 2
        assert_eq!(term.grid()[Pos::new(0, 0)].glyph(), '\u{4e2d}');
        // See `test_put_wide_char_sets_continuation` for why the neighbor is a spacer regardless
        // of feature.
        assert!(
            term.grid()[Pos::new(1, 0)]
                .flags()
                .contains(TileFlags::WIDE_CHAR_SPACER)
        );
        assert_eq!(term.grid()[Pos::new(2, 0)].glyph(), 'x');
    }

    #[test]
    fn test_put_accepts_a_pos_or_a_tuple() {
        // `put` takes `impl Into<Pos>`, so a `Pos` and an equivalent `(u16, u16)` tuple must
        // write the same cell.
        let mut term = Terminal::new(Headless::new(10, 3));
        let mut s = term.surface();
        s.put(Pos::new(2, 1), 'X', Style::default());
        s.put((3, 1), 'Y', Style::default());
        assert_eq!(term.grid()[Pos::new(2, 1)].glyph(), 'X');
        assert_eq!(term.grid()[Pos::new(3, 1)].glyph(), 'Y');
    }

    #[test]
    fn test_put_offset_accepts_pos_and_offset_tuples() {
        // `put_offset` takes `impl Into<Pos>` and `impl Into<Offset>`, so `Pos`/`Offset` values
        // and equivalent tuples must produce the same tile.
        let mut term = Terminal::new(Headless::new(4, 1));
        let mut s = term.surface();
        s.put_offset(
            Pos::new(1, 0),
            crate::grid::Offset::new(3, -2),
            'X',
            Style::default(),
        );
        s.put_offset((2, 0), (-1, 4), 'Y', Style::default());
        assert_eq!(term.grid()[Pos::new(1, 0)].glyph(), 'X');
        assert_eq!(term.grid()[Pos::new(1, 0)].dx(), 3);
        assert_eq!(term.grid()[Pos::new(1, 0)].dy(), -2);

        assert_eq!(term.grid()[Pos::new(2, 0)].glyph(), 'Y');
        assert_eq!(term.grid()[Pos::new(2, 0)].dx(), -1);
        assert_eq!(term.grid()[Pos::new(2, 0)].dy(), 4);
    }

    #[test]
    fn test_put_wide_char_at_last_column_does_not_overflow() {
        // Wide char placed at the last column: can't place a spacer, so the write is refused
        // outright (nothing written), on every feature combination. `write_grapheme` (egc-only)
        // and `Grid::put_tile` (used by `put` on every feature combination) both refuse rather
        // than leave an orphaned primary cell with no spacer.
        let mut term = Terminal::new(Headless::new(4, 1));
        term.surface().put((3, 0), '\u{4e2d}', Style::default()); // col 3 is last; need col 4 for spacer
        assert_eq!(term.grid()[Pos::new(3, 0)].glyph(), ' '); // nothing written
    }

    // --- styled spans ---

    #[test]
    fn test_print_styled_basic() {
        use crate::text::{Line, Span};
        use alloc::vec;

        let mut term = Terminal::new(Headless::new(20, 3));
        let line = Line::from(vec![
            Span::raw("HP: "),
            Span::styled("100", Style::new().fg(Color::GREEN)),
        ]);
        term.surface().print_line((0, 0), &line);
        assert_eq!(term.grid()[Pos::new(0, 0)].glyph(), 'H');
        assert_eq!(term.grid()[Pos::new(3, 0)].glyph(), ' ');
        assert_eq!(term.grid()[Pos::new(4, 0)].glyph(), '1');
        assert_eq!(term.grid()[Pos::new(4, 0)].style.fg, Color::GREEN);
        assert_eq!(term.grid()[Pos::new(6, 0)].glyph(), '0');
    }

    #[test]
    fn test_print_styled_wide_chars() {
        use crate::text::Line;
        use crate::tile::TileFlags;
        use alloc::vec;

        let mut term = Terminal::new(Headless::new(10, 3));
        let line = Line::from(vec![crate::text::Span::raw("\u{4e2d}x")]);
        term.surface().print_line((0, 0), &line);
        assert_eq!(term.grid()[Pos::new(0, 0)].glyph(), '\u{4e2d}');
        // See `test_put_wide_char_sets_continuation` for why the neighbor is a spacer regardless
        // of feature.
        assert!(
            term.grid()[Pos::new(1, 0)]
                .flags()
                .contains(TileFlags::WIDE_CHAR_SPACER)
        );
        assert_eq!(term.grid()[Pos::new(2, 0)].glyph(), 'x');
    }

    #[test]
    fn test_print_str_styled_applies_style_to_every_cell() {
        let mut term = Terminal::new(Headless::new(20, 3));
        term.surface()
            .print((0, 0), "HP", Style::new().fg(Color::GREEN));
        assert_eq!(term.grid()[Pos::new(0, 0)].glyph(), 'H');
        assert_eq!(term.grid()[Pos::new(0, 0)].style.fg, Color::GREEN);
        assert_eq!(term.grid()[Pos::new(1, 0)].glyph(), 'P');
        assert_eq!(term.grid()[Pos::new(1, 0)].style.fg, Color::GREEN);
    }
}
