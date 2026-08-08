//! Frame presentation: [`draw`](Terminal::draw), [`present`](Terminal::present), and
//! [`present_count`](Terminal::present_count).
//!
//! `present` is the one piece of `Terminal`'s API that has to reconcile three different backend
//! shapes (compositing vs. cell, single-layer vs. multi-layer) with the error-recovery contract
//! documented on it; its own doc comment and the tests below cover that matrix directly.

use super::Terminal;
use crate::backend::{Backend, Output};
use crate::grid::{Grid, HasSize};
use crate::surface::Surface;

impl<B: Backend> Terminal<B> {
    /// Draws one frame: `f` gets a [`Surface`] scoped to the whole terminal on layer 0, then the
    /// frame is presented (see [`present`](Self::present)) once `f` returns.
    ///
    /// This is the common entry point for drawing: a caller that draws every frame regardless of
    /// whether anything changed calls this once per frame. A caller that only wants to redraw
    /// when its own state changed should gate the call to `draw` itself (e.g. `if
    /// state.changed() { term.draw(|s| render(s, &state))?; }`) rather than rely on `draw`/
    /// [`present`](Self::present) to no-op.
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

    /// Present the current frame: computes the diff against the previous frame, sends changed
    /// cells to the backend, flushes, then swaps buffers. Always presents unconditionally, even
    /// if nothing was drawn since the last call; most callers want [`draw`](Self::draw) instead
    /// of calling this directly.
    ///
    /// When the backend requires a full frame (see
    /// [`crate::backend::Output::needs_full_frame`]), all cells from every allocated layer are
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
    /// # Panics
    ///
    /// Never panics in practice: `retained_layers` and `dropped_layers` are indexed by u8 layer
    /// id and grown only up to `idx + 1` for `idx = usize::from(layer_id)` in
    /// [`retain_layer`](Self::retain_layer)/[`drop_layer`](Self::drop_layer), so their length is
    /// always at most 256 and every index encountered here fits in u8.
    ///
    /// # Errors
    ///
    /// Propagates errors from the backend's [`draw_layers`](crate::backend::Output::draw_layers) or
    /// [`flush`](crate::backend::Output::flush) operations. Either failure returns before the
    /// current/previous buffers are swapped, so the cells from the failed frame stay marked
    /// dirty in `previous` and are resent the next time `present` succeeds. `current` is still
    /// cleared, same as on success, so the caller doesn't need to redraw anything to recover:
    /// just call `draw`/`present` again, and the next frame starts from an empty grid like any
    /// other.
    #[doc(alias = "flush")]
    #[doc(alias = "render")]
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
            // what was presented last frame, whatever the app did or didn't draw into it this
            // frame (retroglyph#955, retroglyph#956).
            for (id, &retained) in self.retained_layers.iter().enumerate() {
                if retained {
                    // `retained_layers` is indexed by u8 layer id: `retain_layer` only ever grows
                    // it to `idx + 1` for `idx = usize::from(layer_id)`, so its length is at most
                    // 256 and every index here fits in u8. `expect` makes that a checked invariant
                    // instead of a silently-truncating `as`.
                    let id = u8::try_from(id).expect("layer table is indexed by u8 layer ids");
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
                // Same reasoning as the fast path below: this branch bypasses the flatten buffers
                // too, so the next present that lands in the flatten branch (e.g. a backend whose
                // `composites_layers()` flips to `false`) must not diff against a
                // `flattened_previous` that was never actually the last frame presented.
                self.flattened_stale = true;
            } else if self.current.max_layer() == 0 && self.previous.max_layer() == 0 {
                // Fast path: only layer 0 is in play, so flattening would be an exact
                // copy of `current`. Diff the real grids directly and skip the
                // flatten buffers entirely.
                //
                // This is sticky-off, not sticky-on: layers are never deallocated on their own
                // once written (see `Grid`'s layer storage), so `max_layer()` never drops back to
                // 0 on its own. A terminal that ever draws to layer 1+, even for a single
                // transient frame, stays on the flatten path in the `else` branch below for the
                // rest of the process, unless it explicitly calls `drop_layer` on every layer
                // above 0 (retroglyph#1028).
                let diff = self.current.diff(&self.previous);
                self.backend.draw_layers(diff)?;
                self.flattened_stale = true;
            } else {
                // Cell backends receive a pre-flattened, single-layer diff so layers
                // 1+ appear everywhere, not just on pixel backends.
                let size = self.current.size();
                let flattened_current = self
                    .flattened_current
                    .get_or_insert_with(|| Grid::new(size.width(), size.height()));
                let flattened_previous = self
                    .flattened_previous
                    .get_or_insert_with(|| Grid::new(size.width(), size.height()));
                if self.flattened_stale {
                    // The previous frame used the fast path, so `flattened_previous`
                    // is stale. Clear it to force a full redraw this frame.
                    flattened_previous.clear_all();
                    self.flattened_stale = false;
                }
                self.current.flatten_into(flattened_current);
                let diff = flattened_current.diff(flattened_previous);
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
        // Deallocate any layer `drop_layer` marked, now that the diff above (computed while the
        // layer was still allocated, if only as an already-cleared buffer) has told the backend
        // to erase whatever it last showed there. Also gated on `flush` succeeding, for the same
        // reason as the swaps below: on failure, `previous` must keep the layer allocated so a
        // retried `present` can still resend the erase that never actually reached the backend.
        if self.dropped_layers.iter().any(|&dropped| dropped) {
            for (id, &dropped) in self.dropped_layers.iter().enumerate() {
                if dropped {
                    let id = u8::try_from(id).expect("layer table is indexed by u8 layer ids");
                    // If the app drew to `layer` again after calling `drop_layer` but before
                    // this present, that write is a live redraw the app clearly wants kept, not
                    // stale content: cancel the drop instead of discarding it.
                    if self.current.layer_is_empty(id) {
                        self.current.deallocate_layer(id);
                        self.previous.deallocate_layer(id);
                    }
                }
            }
            for dropped in &mut self.dropped_layers {
                *dropped = false;
            }
        }
        // Both swaps happen only after `flush` succeeds, for the same reason described above.
        if swap_flattened {
            core::mem::swap(&mut self.flattened_current, &mut self.flattened_previous);
        }
        core::mem::swap(&mut self.current, &mut self.previous);
        self.current.clear_all();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{Cursor, DrawCell, Headless, Input};
    use crate::color::Color;
    use crate::color::Style;
    use crate::event::Event;
    use crate::grid::{Pos, Size};
    use alloc::vec::Vec;
    use core::time::Duration;

    /// Wraps [`Headless`] and fails the next [`flush`](Output::flush) or
    /// [`draw_layers`](Output::draw_layers) call once, then forwards everything (including a
    /// failed `draw_layers` call's content, which already reached the inner backend) as normal.
    /// Used to exercise `present`'s documented error-recovery contract: either failure must
    /// leave the frame's cells marked dirty so they are resent on the next successful `present`.
    ///
    /// `composites_layers` is also configurable, so the same helper covers the compositing,
    /// flatten, and single-layer fast-path branches of `present`.
    ///
    /// `std`-only: its `Output::Error` is `std::io::Error`, purely as a convenient stand-in
    /// error type for this test.
    #[cfg(feature = "std")]
    struct FlushOnceFailing {
        inner: Headless,
        fail_next_flush: bool,
        fail_next_draw_layers: bool,
        composites_layers: bool,
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
                fail_next_draw_layers: false,
                composites_layers: false,
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
            if self.fail_next_draw_layers {
                self.fail_next_draw_layers = false;
                return Err(std::io::Error::other("simulated draw_layers failure"));
            }
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

        fn composites_layers(&self) -> bool {
            self.composites_layers
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

    /// A `composites_layers() == true` backend, the branch of `present` no real backend in this
    /// workspace's core tests exercises (`retroglyph-gl`/`retroglyph-software` test their own
    /// side of the [`Output`] contract, not `present`'s choice between it and a diff).
    /// `needs_full_frame` is fixed at construction, so one struct covers both dispatch modes.
    ///
    /// Unlike [`Headless`], this records the raw `(layer, pos, glyph)` cells it receives instead
    /// of writing them into a single flat grid: a real compositing backend interprets an
    /// unwritten (default/blank) cell on a higher layer as transparent, but `Headless::draw_layers`
    /// writes every cell it's handed literally to one shared grid regardless of layer, so replaying
    /// a raw multi-layer stream through it (rather than the pre-flattened stream the non-
    /// compositing path sends) does not reproduce correct compositing.
    struct CompositingBackend {
        size: Size,
        full_frame: bool,
        last_draw_cells: Vec<(u8, Pos, char)>,
    }

    impl CompositingBackend {
        fn new(width: u16, height: u16, full_frame: bool) -> Self {
            Self {
                size: Size::new(width, height),
                full_frame,
                last_draw_cells: Vec::new(),
            }
        }
    }

    impl Output for CompositingBackend {
        type Error = core::convert::Infallible;

        fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
        where
            I: Iterator<Item = DrawCell<'a>>,
        {
            self.last_draw_cells = content
                .map(|cell| (cell.layer, cell.pos, cell.tile.glyph()))
                .collect();
            Ok(())
        }

        fn flush(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }

        fn size(&self) -> Size {
            self.size
        }

        fn clear(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }

        fn composites_layers(&self) -> bool {
            true
        }

        fn needs_full_frame(&self) -> bool {
            self.full_frame
        }
    }

    impl Input for CompositingBackend {
        fn poll_event(&mut self, _timeout: Duration) -> Option<Event> {
            None
        }
    }

    impl Cursor for CompositingBackend {}

    #[test]
    fn test_composites_layers_diff_dispatches_only_changed_cells() {
        let mut term = Terminal::new(CompositingBackend::new(3, 1, false));
        term.draw(|s| {
            s.put((0, 0), 'a', Style::default());
            s.put((1, 0), 'b', Style::default());
        })
        .expect("draw failed");
        assert_eq!(
            term.backend().last_draw_cells.len(),
            2,
            "first frame: only the two written cells differ from the pre-allocated blank layer 0"
        );

        // Second, identical frame: nothing changed, so the compositing branch's diff half sends
        // nothing, same as the non-compositing diff path.
        term.draw(|s| {
            s.put((0, 0), 'a', Style::default());
            s.put((1, 0), 'b', Style::default());
        })
        .expect("draw failed");
        assert!(
            term.backend().last_draw_cells.is_empty(),
            "composites_layers() == true with needs_full_frame() == false still dispatches only \
             the diff"
        );
    }

    #[test]
    fn test_composites_layers_full_frame_dispatches_every_allocated_cell() {
        let mut term = Terminal::new(CompositingBackend::new(3, 1, true));
        term.draw(|s| {
            s.put((0, 0), 'a', Style::default());
            s.put((1, 0), 'b', Style::default());
        })
        .expect("draw failed");
        assert_eq!(
            term.backend().last_draw_cells.len(),
            3,
            "first frame: every cell in the sole allocated layer (width 3), not just the two \
             written ones"
        );

        // Second, identical frame: unlike the diff branch above, needs_full_frame() actually
        // takes effect here, so the whole layer is resent rather than an empty diff.
        term.draw(|s| {
            s.put((0, 0), 'a', Style::default());
            s.put((1, 0), 'b', Style::default());
        })
        .expect("draw failed");
        assert_eq!(
            term.backend().last_draw_cells.len(),
            3,
            "composites_layers() == true with needs_full_frame() == true resends every allocated \
             cell on every present"
        );
    }

    /// A cell backend whose `composites_layers()` can be toggled between presents, standing in
    /// for a backend that degrades from pixel compositing to a cell path at runtime (retroglyph#960).
    struct TogglingCompositor {
        inner: Headless,
        composites: bool,
    }

    impl Output for TogglingCompositor {
        type Error = core::convert::Infallible;

        fn draw_layers<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
        where
            I: Iterator<Item = DrawCell<'a>>,
        {
            self.inner.draw_layers(content)
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

        fn composites_layers(&self) -> bool {
            self.composites
        }
    }

    impl Input for TogglingCompositor {
        fn poll_event(&mut self, timeout: Duration) -> Option<Event> {
            self.inner.poll_event(timeout)
        }
    }

    impl Cursor for TogglingCompositor {}

    #[test]
    fn present_marks_flatten_buffers_stale_after_a_composites_layers_present() {
        // A backend that ever answers `true` from `composites_layers()` and later `false` must
        // not leave `flattened_previous` holding a frame that was never actually the last one
        // presented. Sequence: flatten branch (establishes stale-looking data) -> composites
        // branch (bypasses the flatten buffers entirely) -> flatten branch again, where the bug
        // would incorrectly diff against the first frame's flattened data instead of the second.
        let mut term = Terminal::new(TogglingCompositor {
            inner: Headless::new(3, 1),
            composites: false,
        });

        // Frame 1: flatten branch. Layer 1 is touched so `max_layer() != 0`, and
        // `composites_layers()` is `false`, so this flattens and diffs normally.
        term.draw(|s| {
            s.put((0, 0), 'a', Style::default());
            s.on_layer(1).put((1, 0), '#', Style::default());
        })
        .expect("draw failed");
        assert_eq!(term.backend().inner.grid()[Pos::new(0, 0)].glyph(), 'a');

        // Frame 2: composites branch. Bypasses the flatten buffers entirely, so
        // `flattened_previous` still holds frame 1's flattened content. Layer 1 is redrawn
        // identically to frame 1 so `Grid::diff` (now that it also reports a layer that stopped
        // being written, retroglyph#1018) sees no change there and doesn't emit anything for it;
        // `Headless::draw_layers` writes every cell to one shared grid regardless of layer (see
        // `CompositingBackend`'s docs above), so a real layer-1 diff would corrupt this frame's
        // single-grid glyph check, which is unrelated to what this test is verifying.
        term.backend_mut().composites = true;
        term.draw(|s| {
            s.put((0, 0), 'b', Style::default());
            s.on_layer(1).put((1, 0), '#', Style::default());
        })
        .expect("draw failed");
        assert_eq!(term.backend().inner.grid()[Pos::new(0, 0)].glyph(), 'b');

        // Frame 3: back to the flatten branch. Draws 'a' at (0, 0) again, on layer 0, but with
        // layer 1 also touched so this lands in the flatten branch rather than the fast path.
        // 'a' matches what frame 1 left in `flattened_previous`, even though the real last
        // presented frame (frame 2) showed 'b' there. Without the fix, the stale match makes the
        // diff skip (0, 0), and the backend keeps showing frame 2's 'b' forever.
        term.backend_mut().composites = false;
        term.draw(|s| {
            s.put((0, 0), 'a', Style::default());
            s.on_layer(1).put((2, 0), '@', Style::default());
        })
        .expect("draw failed");
        assert_eq!(
            term.backend().inner.grid()[Pos::new(0, 0)].glyph(),
            'a',
            "flattened_previous must be cleared after a composites_layers() present, not diffed \
             against as if it were the last frame actually shown"
        );

        // `TogglingCompositor` forwards `clear` and `poll_event` unconditionally, same as every
        // other method on it, so exercise both here rather than leaving them as dead delegation.
        term.backend_mut().clear().expect("clear failed");
        term.backend_mut().inner.push_event(Event::Close);
        assert_eq!(term.poll(Duration::ZERO), Some(Event::Close));
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

    #[test]
    fn test_present_transition_multi_to_single_to_multi_layer() {
        // The reverse of the transition above: multi-layer (flatten path) drops back to
        // single-layer (fast path, sets `flattened_stale`), then multi-layer again. The frame
        // that returns to multi-layer must see `flattened_previous` cleared rather than diffed
        // against the stale content the fast path bypassed, or the reintroduced layer's cells
        // would wrongly look unchanged.
        let mut term = Terminal::new(Headless::new(2, 1));
        term.draw(|s| {
            s.put((0, 0), '.', Style::default());
            s.on_layer(1).put((1, 0), '@', Style::default());
        })
        .expect("draw failed");

        // Single-layer frame: fast path, `flattened_stale` set.
        term.draw(|s| s.put((0, 0), '.', Style::default()))
            .expect("draw failed");
        assert_eq!(term.backend().grid()[Pos::new(1, 0)].glyph(), ' ');

        // Back to multi-layer: must composite correctly despite the intervening fast-path frame.
        term.draw(|s| {
            s.put((0, 0), '.', Style::default());
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
    fn present_resends_cells_after_a_failed_draw_layers_on_the_multi_layer_path() {
        // `draw_layers` is the other documented early return in `present`: it must leave
        // `previous`/`flattened_previous` untouched, same as a failed `flush`, so the next
        // `present` resends everything rather than silently dropping it.
        let mut term = Terminal::new(FlushOnceFailing::new(2, 1));

        term.backend_mut().fail_next_draw_layers = true;
        let result = term.draw(|s| {
            s.put((0, 0), 'a', Style::default());
            s.on_layer(1).put((1, 0), 'b', Style::default());
        });
        assert!(
            result.is_err(),
            "draw_layers was expected to fail this frame"
        );
        assert_eq!(
            term.backend().last_draw_len,
            0,
            "a failed draw_layers call never recorded any content on the wrapper"
        );

        // Same content, draw_layers succeeds this time.
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
    fn present_resends_cells_after_a_failed_flush_on_the_single_layer_fast_path() {
        // Only layer 0 is ever touched, so `present` takes the fast path that diffs the raw
        // grids directly, skipping the flatten buffers entirely.
        let mut term = Terminal::new(FlushOnceFailing::new(2, 1));

        term.backend_mut().fail_next_flush = true;
        let result = term.draw(|s| s.put((0, 0), 'a', Style::default()));
        assert!(result.is_err(), "flush was expected to fail this frame");
        assert_eq!(
            term.backend().last_draw_len,
            1,
            "the failed frame's diff should still have been sent to draw_layers"
        );

        // Nothing drawn this time: if the fast path's diff had already been swapped forward on
        // the failed attempt, this frame would see no change and resend nothing.
        term.draw(|s| s.put((0, 0), 'a', Style::default()))
            .expect("draw failed");
        assert_eq!(
            term.backend().last_draw_len,
            1,
            "the cell must be resent since it never reached the screen"
        );
        assert_eq!(term.backend().inner.grid()[Pos::new(0, 0)].glyph(), 'a');
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

    #[cfg(feature = "std")]
    #[test]
    fn present_resends_cells_after_a_failed_flush_on_the_compositing_path() {
        // `composites_layers() == true` takes `present`'s first branch entirely, bypassing both
        // the fast path and the flatten buffers; a failed flush there must still leave `previous`
        // untouched so the raw per-layer diff is resent.
        let mut term = Terminal::new(FlushOnceFailing::new(2, 1));
        term.backend_mut().composites_layers = true;

        // Layer 1 is newly allocated this frame, so its diff against an absent previous layer
        // includes every cell in its width (see `Grid::diff`'s "newly allocated layer" case), not
        // just the one actually written; layer 0 contributes only its one real change.
        term.backend_mut().fail_next_flush = true;
        let result = term.draw(|s| {
            s.put((0, 0), 'a', Style::default());
            s.on_layer(1).put((1, 0), 'b', Style::default());
        });
        assert!(result.is_err(), "flush was expected to fail this frame");
        assert_eq!(
            term.backend().last_draw_len,
            3,
            "the failed frame's diff should still have been sent to draw_layers"
        );

        // Same content, flush succeeds this time. If `previous` had already been swapped forward
        // on the failed attempt, layer 1 would no longer be "newly allocated" and this diff would
        // shrink to just the real changes instead of resending the same content.
        term.draw(|s| {
            s.put((0, 0), 'a', Style::default());
            s.on_layer(1).put((1, 0), 'b', Style::default());
        })
        .expect("draw failed");
        assert_eq!(
            term.backend().last_draw_len,
            3,
            "the same diff must be resent since it never reached the screen"
        );
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
    fn test_terminal_present_count_advances_once_per_present_call() {
        let mut term = Terminal::new(Headless::new(2, 1));
        assert_eq!(term.present_count(), 0);

        term.draw(|_| {}).expect("draw failed"); // `draw` always presents.
        assert_eq!(term.present_count(), 1);

        term.present().expect("present failed");
        assert_eq!(term.present_count(), 2);

        term.present().expect("present failed");
        assert_eq!(term.present_count(), 3);
    }
}
