//! Per-layer redraw control: [`retain_layer`](Terminal::retain_layer) and
//! [`drop_layer`](Terminal::drop_layer).
//!
//! Both are one-shot opt-ins that defer part of [`present`](Terminal::present)'s work by a
//! frame: `retain_layer` re-syncs a layer from `previous` instead of requiring a redraw, and
//! `drop_layer` defers deallocating a layer until its erase has actually reached the backend.
//! See each method's own doc for the full contract.

use super::Terminal;
use crate::backend::Backend;

impl<B: Backend> Terminal<B> {
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
    /// that regeneration too, at the cost of a per-cell copy handled internally (a flat, verbatim
    /// replace, far cheaper than most real content generation).
    ///
    /// This is a one-shot opt-in, not a sticky mode: it only affects the very next `present`, so
    /// a caller that wants a layer retained for several frames in a row must call this again
    /// before each of them. [`resize`](Self::resize) also clears any pending retention.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::backend::Headless;
    /// use retroglyph_core::surface::Layer;
    /// use retroglyph_core::terminal::Terminal;
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

    /// Marks `layer` to be deallocated, forgetting it was ever drawn to.
    ///
    /// [`Grid::max_layer`](crate::grid::Grid::max_layer) only grows on write, so a terminal that ever draws to a layer above 0,
    /// even for a single frame, stays on [`present`](Self::present)'s flatten path for the rest of
    /// the process, whether or not that layer is still in use (retroglyph#1028). This is the
    /// explicit escape hatch: call it once a layer's content is truly done (a one-off overlay
    /// dismissed, a transient effect finished), and once every layer above 0 has been dropped,
    /// `present` falls back onto its single-layer fast path.
    ///
    /// `layer`'s content is cleared immediately (so this frame's diff still tells the backend to
    /// erase whatever it last showed there, exactly as if the app had simply stopped drawing to
    /// it), but the underlying buffer is only freed, and `max_layer` only allowed to fall, once
    /// the next [`present`](Self::present) has sent that erase and no longer needs the layer for
    /// its diff. Deallocating any earlier would make the layer invisible to `present`'s diff
    /// (which only walks `current`'s own allocated layers), silently dropping the erase instead of
    /// sending it.
    ///
    /// This is a one-shot request, not a sticky mode: unlike [`retain_layer`](Self::retain_layer),
    /// which defers one frame's redraw, this defers the actual deallocation, so drawing to `layer`
    /// again before the next `present` (undoing the drop) cancels it instead of losing that draw:
    /// the layer stays allocated and behaves like any other write. Any pending
    /// [`retain_layer`](Self::retain_layer) call for `layer` is cleared immediately, though:
    /// retaining content that no longer exists would resurrect it on the next present regardless
    /// of whether the drop itself goes through.
    ///
    /// # Panics
    ///
    /// Panics if `layer` is 0: layer 0 is always allocated and can never be dropped.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::backend::Headless;
    /// use retroglyph_core::surface::Layer;
    /// use retroglyph_core::terminal::Terminal;
    ///
    /// let mut term = Terminal::new(Headless::new(10, 5));
    /// term.draw(|s| s.on_tier(Layer::Hud).print((0, 0), "Paused", Default::default()))
    ///     .unwrap();
    ///
    /// term.drop_layer(Layer::Hud);
    /// term.present().unwrap(); // Sends the erase, then frees the layer.
    /// assert_eq!(term.grid().max_layer(), 0);
    /// ```
    pub fn drop_layer(&mut self, layer: impl Into<u8>) {
        let id = layer.into();
        assert_ne!(id, 0, "layer 0 is always allocated and cannot be dropped");
        self.current.clear(id);
        let idx = usize::from(id);
        if idx < self.retained_layers.len() {
            self.retained_layers[idx] = false;
        }
        if self.dropped_layers.len() <= idx {
            self.dropped_layers.resize(idx + 1, false);
        }
        self.dropped_layers[idx] = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{Cursor, DrawCell, Headless, Input, Output};
    use crate::color::Style;
    use crate::event::Event;
    use crate::grid::{Pos, Size};
    use alloc::vec::Vec;
    use core::time::Duration;

    /// A `composites_layers() == true` cell-recording backend, used to prove `retain_layer`'s
    /// pre-diff copy from `previous` still applies on the branch of `present` that bypasses the
    /// fast path and flatten buffers entirely. See `present`'s own test module for the fuller
    /// version of this fixture (dispatch-mode coverage); this one only needs the diff branch.
    struct CompositingBackend {
        size: Size,
        last_draw_cells: Vec<(u8, Pos, char)>,
    }

    impl CompositingBackend {
        fn new(width: u16, height: u16) -> Self {
            Self {
                size: Size::new(width, height),
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
    }

    impl Input for CompositingBackend {
        fn poll_event(&mut self, _timeout: Duration) -> Option<Event> {
            None
        }
    }

    impl Cursor for CompositingBackend {}

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
    fn test_retain_layer_replaces_a_cell_the_app_draws_at_an_empty_previous_cell() {
        use crate::surface::Layer;

        // retroglyph#956: `present` must discard whatever the app drew into a retained layer
        // this frame, even at a cell where `previous` had nothing (so a naive transparent-skip
        // copy would let the app's write leak through).
        let mut term = Terminal::new(Headless::new(4, 1));
        term.draw(|s| s.on_tier(Layer::World).put((0, 0), 'W', Style::default()))
            .expect("draw failed");

        term.retain_layer(Layer::World);
        term.draw(|s| s.on_tier(Layer::World).put((2, 0), 'X', Style::default()))
            .expect("draw failed");

        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), 'W');
        assert_eq!(term.backend().grid()[Pos::new(2, 0)].glyph(), ' ');
    }

    #[test]
    fn test_retain_layer_restores_a_cell_the_app_erases_with_an_explicit_space() {
        use crate::surface::Layer;

        // retroglyph#956: an explicit-space write on a retained layer is still a draw the app
        // made this frame, so it must be discarded like any other write on that layer, not
        // treated as an opaque erase that survives the retention.
        let mut term = Terminal::new(Headless::new(4, 1));
        term.draw(|s| s.on_tier(Layer::World).put((0, 0), 'W', Style::default()))
            .expect("draw failed");

        term.retain_layer(Layer::World);
        term.draw(|s| s.on_tier(Layer::World).put((0, 0), ' ', Style::default()))
            .expect("draw failed");

        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), 'W');
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
    fn test_retain_layer_skips_redraw_on_a_compositing_backend() {
        use crate::surface::Layer;

        // `composites_layers() == true` takes `present`'s first branch entirely; `retain_layer`'s
        // pre-diff copy from `previous` (`Grid::copy_layer_from`) runs before that branch, so it
        // must still apply here: the retained layer's cell should be re-synced (and so absent
        // from the diff, since it now matches `previous`) rather than diffed as a real change.
        let mut term = Terminal::new(CompositingBackend::new(3, 1));
        term.draw(|s| s.on_tier(Layer::World).put((0, 0), 'W', Style::default()))
            .expect("draw failed");

        term.retain_layer(Layer::World);
        term.draw(|s| s.on_tier(Layer::Hud).put((1, 0), 'H', Style::default()))
            .expect("draw failed");

        let cells = &term.backend().last_draw_cells;
        assert!(
            !cells
                .iter()
                .any(|&(layer, pos, _)| layer == 0 && pos == Pos::new(0, 0)),
            "retained layer's unchanged cell must not be re-sent as a diff: {cells:?}"
        );
        assert!(
            cells.contains(&(1, Pos::new(1, 0), 'H')),
            "the newly drawn Hud cell must still be sent: {cells:?}"
        );
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

    // --- drop_layer ---

    #[test]
    #[should_panic(expected = "layer 0 is always allocated")]
    fn test_drop_layer_zero_panics() {
        let mut term = Terminal::new(Headless::new(3, 1));
        term.drop_layer(0u8);
    }

    #[test]
    fn test_drop_layer_restores_the_single_layer_fast_path() {
        use crate::surface::Layer;

        // retroglyph#1028: a layer stays allocated once written, permanently moving `present`
        // onto the flatten path. `drop_layer` is the explicit escape hatch back to the
        // single-layer fast path once every layer above 0 is gone.
        let mut term = Terminal::new(Headless::new(3, 1));
        term.draw(|s| s.on_tier(Layer::Hud).put((0, 0), 'H', Style::default()))
            .expect("draw failed");
        // `current` was just swapped to the other (never-written) buffer, so `previous` is the
        // one still carrying the allocation right after this draw.
        assert_eq!(term.current.max_layer(), 0);
        assert_ne!(term.previous.max_layer(), 0);

        // The deallocation is deferred to the next `present`: calling `drop_layer` alone must
        // not yet change either buffer's `max_layer`.
        term.drop_layer(Layer::Hud);
        assert_ne!(term.previous.max_layer(), 0);

        term.draw(|_| {}).expect("draw failed");
        assert_eq!(term.current.max_layer(), 0);
        assert_eq!(term.previous.max_layer(), 0);
    }

    #[test]
    fn test_drop_layer_erases_content_from_the_backend_on_the_next_present() {
        use crate::surface::Layer;

        let mut term = Terminal::new(Headless::new(3, 1));
        term.draw(|s| s.on_tier(Layer::Hud).put((0, 0), 'H', Style::default()))
            .expect("draw failed");
        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), 'H');

        term.drop_layer(Layer::Hud);
        term.draw(|_| {}).expect("draw failed");

        // The layer's content is gone once the drop has gone through a present, same as if the
        // app had simply stopped drawing to it.
        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), ' ');
    }

    #[test]
    fn test_drop_layer_is_cancelled_by_a_redraw_before_the_next_present() {
        use crate::surface::Layer;

        // A write to `layer` after `drop_layer` but before the deferred deallocation runs is a
        // live redraw the app wants kept, not stale content left over from before the drop, so
        // it must not be silently discarded.
        let mut term = Terminal::new(Headless::new(3, 1));
        term.draw(|s| s.on_tier(Layer::Hud).put((0, 0), 'H', Style::default()))
            .expect("draw failed");

        term.drop_layer(Layer::Hud);
        term.draw(|s| s.on_tier(Layer::Hud).put((0, 0), 'J', Style::default()))
            .expect("draw failed");

        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), 'J');
        assert_ne!(term.current.max_layer(), 0);
    }

    #[test]
    fn test_drop_layer_clears_pending_retention_for_that_layer() {
        use crate::surface::Layer;

        // Retaining content that no longer exists would resurrect it on the next present, so
        // `drop_layer` must clear any pending `retain_layer` call for the same id.
        let mut term = Terminal::new(Headless::new(3, 1));
        term.draw(|s| s.on_tier(Layer::Hud).put((0, 0), 'H', Style::default()))
            .expect("draw failed");

        term.retain_layer(Layer::Hud);
        term.drop_layer(Layer::Hud);
        term.draw(|_| {}).expect("draw failed");

        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), ' ');
    }

    #[test]
    fn test_drop_layer_never_drawn_is_a_no_op() {
        let mut term = Terminal::new(Headless::new(3, 1));
        term.drop_layer(5u8);
        term.draw(|_| {}).expect("draw failed");
        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), ' ');
    }

    #[test]
    fn test_drop_layer_reallocates_on_the_next_write_after_it_takes_effect() {
        use crate::surface::Layer;

        // The only way back once a drop has actually gone through is drawing to the layer
        // again, which reallocates it from scratch.
        let mut term = Terminal::new(Headless::new(3, 1));
        term.draw(|s| s.on_tier(Layer::Hud).put((0, 0), 'H', Style::default()))
            .expect("draw failed");
        term.drop_layer(Layer::Hud);
        term.draw(|_| {}).expect("draw failed"); // Lets the drop take effect.
        assert_eq!(term.current.max_layer(), 0);
        assert_eq!(term.previous.max_layer(), 0);

        term.draw(|s| s.on_tier(Layer::Hud).put((0, 0), 'J', Style::default()))
            .expect("draw failed");
        assert_eq!(term.backend().grid()[Pos::new(0, 0)].glyph(), 'J');
        assert_ne!(term.previous.max_layer(), 0);
    }
}
