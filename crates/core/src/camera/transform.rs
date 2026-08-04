//! Coordinate conversions between world and screen space.

use super::Camera;
use crate::grid::{Pos, Rect};
use crate::surface::Surface;
use ixy::HasSize;

impl Camera {
    /// The world rectangle currently visible, clamped to world bounds.
    ///
    /// Never panics: the clamp against `world` uses
    /// [`saturating_sub`](u16::saturating_sub), so it cannot underflow even if `origin` is
    /// somehow past `world`'s edge.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{Camera, Pos, Rect, Size};
    ///
    /// // A 10x10 viewport near the bottom-right corner of a 12x12 world: the origin clamps
    /// // to (2, 2), so the visible rect is narrower than the viewport rather than reading
    /// // past the world edge.
    /// let mut cam = Camera::new(Rect::new(0, 0, 10, 10), Size::new(12, 12));
    /// cam.center_on(Pos::new(11, 11));
    /// assert_eq!(cam.origin(), Pos::new(2, 2));
    /// assert_eq!(cam.visible_bounds(), Rect::new(2, 2, 10, 10));
    ///
    /// // A world smaller than the viewport: the visible rect is the whole world, not the
    /// // full viewport size.
    /// let small = Camera::new(Rect::new(0, 0, 20, 20), Size::new(5, 5));
    /// assert_eq!(small.visible_bounds(), Rect::new(0, 0, 5, 5));
    /// ```
    #[must_use]
    pub fn visible_bounds(&self) -> Rect {
        Rect::from_tl_size(self.origin, self.viewport.size()).intersect(self.world.to_rect())
    }

    /// Map a world position to its screen position, or `None` if it is outside
    /// [`visible_bounds`](Self::visible_bounds): the viewport, clamped to the world.
    #[must_use]
    pub const fn world_to_screen(&self, world: Pos) -> Option<Pos> {
        if world.x < self.origin.x || world.y < self.origin.y {
            return None;
        }
        if world.x >= self.world.width || world.y >= self.world.height {
            return None;
        }
        let dx = world.x - self.origin.x;
        let dy = world.y - self.origin.y;
        if dx >= self.viewport.width() || dy >= self.viewport.height() {
            return None;
        }
        Some(Pos::new(
            self.viewport.left().saturating_add(dx),
            self.viewport.top().saturating_add(dy),
        ))
    }

    /// Map a world position to its screen position, without culling: the result may fall
    /// outside the viewport (negative, or past its far edge) instead of coming back `None`.
    ///
    /// [`world_to_screen`](Self::world_to_screen) is the right call when the only question is
    /// "is this single cell visible" (a minimap dot, a cursor). It falls short for anything
    /// wider than one cell (a hex, an iso diamond, a multi-cell sprite) where the *anchor*
    /// can be off-viewport while part of the content is still visible. This is the signed
    /// sibling for that case: it hands back the same math `world_to_screen` computes, minus the
    /// culling, ready for [`Surface::put_signed`] to clip.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{Camera, Pos, Rect, Size};
    ///
    /// let mut cam = Camera::new(Rect::new(0, 0, 10, 10), Size::new(100, 100));
    /// cam.center_on(Pos::new(50, 50));
    ///
    /// // Inside the viewport: matches `world_to_screen`.
    /// assert_eq!(cam.world_to_offset(Pos::new(50, 50)), (5, 5));
    ///
    /// // A multi-cell sprite's top-left anchor two cells left of the viewport: negative, not
    /// // `None`, so a caller can still hand this to `Surface::put_signed` and let the visible
    /// // half draw.
    /// assert_eq!(cam.world_to_offset(Pos::new(43, 50)), (-2, 5));
    /// ```
    #[must_use]
    pub const fn world_to_offset(&self, world: Pos) -> (i32, i32) {
        let dx = world.x as i32 - self.origin.x as i32;
        let dy = world.y as i32 - self.origin.y as i32;
        (
            self.viewport.left() as i32 + dx,
            self.viewport.top() as i32 + dy,
        )
    }

    /// A view of `surface` in this camera's world coordinate space, clipped to
    /// [`visible_bounds`](Self::visible_bounds): [`Surface::clip_translate`] to the visible
    /// rect, by [`origin`](Self::origin).
    ///
    /// The returned surface's `put`, `put_signed`, `print`, and the rest of `Surface`'s
    /// coordinate-taking methods all take world coordinates directly, and anything that lands
    /// outside `visible_bounds` (including a multi-cell draw anchored off-screen, or - for a
    /// world smaller than the viewport - the dead margin past the world edge) is dropped by the
    /// surface's own bounds check, the same way [`world_to_offset`] composes with
    /// [`Surface::put_signed`] by hand. This is that composition done once instead of at every
    /// call site.
    ///
    /// Clipping to `visible_bounds` rather than [`viewport`](Self::viewport) directly matches
    /// [`world_to_screen`](Self::world_to_screen) and [`screen_to_world`](Self::screen_to_world):
    /// a world smaller than the viewport (under plain [`set_viewport`](Self::set_viewport), not
    /// [`set_viewport_fitted`](Self::set_viewport_fitted)) shrinks the clip to the world's size
    /// instead of leaving the viewport's dead margin drawable.
    ///
    /// [`world_to_offset`]: Self::world_to_offset
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{Camera, Grid, Pos, Rect, Size, Style, Surface};
    ///
    /// let mut grid = Grid::new(20, 20);
    /// let mut root = Surface::new(&mut grid, Rect::new(0, 0, 20, 20), 0);
    ///
    /// let mut cam = Camera::new(Rect::new(5, 5, 10, 10), Size::new(100, 100));
    /// cam.center_on(Pos::new(50, 50));
    ///
    /// let mut world = cam.surface(&mut root);
    /// // Drawn in world coordinates: (50, 50) is the centered target, landing at the
    /// // viewport's center cell (10, 10) in grid space.
    /// world.put(Pos::new(50, 50), '@', Style::default());
    /// // A world position outside the viewport is dropped, not a panic or a manual guard.
    /// world.put(Pos::new(0, 0), 'X', Style::default());
    ///
    /// assert_eq!(grid[Pos::new(10, 10)].glyph(), '@');
    /// ```
    ///
    /// A world smaller than the viewport: drawing into the dead margin past the world edge is
    /// dropped, not written past the world into unused grid cells.
    ///
    /// ```
    /// use retroglyph_core::{Camera, Grid, Pos, Rect, Size, Style, Surface};
    ///
    /// let mut grid = Grid::new(20, 20);
    /// let mut root = Surface::new(&mut grid, Rect::new(0, 0, 20, 20), 0);
    ///
    /// // A 20x20 viewport over a 5x5 world: `visible_bounds` is only 5x5, not the full
    /// // viewport, so the clip shrinks to match.
    /// let cam = Camera::new(Rect::new(0, 0, 20, 20), Size::new(5, 5));
    ///
    /// let mut world = cam.surface(&mut root);
    /// world.put(Pos::new(0, 0), '@', Style::default());
    /// // Inside the viewport but past the (smaller) world's edge: dropped.
    /// world.put(Pos::new(10, 10), 'X', Style::default());
    ///
    /// assert_eq!(grid[Pos::new(0, 0)].glyph(), '@');
    /// assert_eq!(grid[Pos::new(10, 10)].glyph(), ' ');
    /// ```
    #[must_use]
    pub fn surface<'a>(&self, surface: &'a mut Surface<'_>) -> Surface<'a> {
        let visible = self.visible_bounds();
        let area = Rect::new(
            self.viewport.left(),
            self.viewport.top(),
            visible.width(),
            visible.height(),
        );
        surface.clip_translate(area, (i32::from(self.origin.x), i32::from(self.origin.y)))
    }

    /// Map a screen position back to a world position, or `None` if it is
    /// outside the viewport or beyond the world (useful for mouse picking).
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{Camera, Pos, Rect, Size};
    ///
    /// let mut cam = Camera::new(Rect::new(5, 5, 10, 10), Size::new(100, 100));
    /// cam.center_on(Pos::new(50, 50));
    ///
    /// // Inside the viewport: maps back to the world cell under it.
    /// assert_eq!(cam.screen_to_world(Pos::new(5, 5)), Some(Pos::new(45, 45)));
    ///
    /// // Off the viewport entirely (the viewport starts at x = 5): `None`, not a clamp.
    /// assert_eq!(cam.screen_to_world(Pos::new(0, 0)), None);
    /// ```
    #[must_use]
    pub fn screen_to_world(&self, screen: Pos) -> Option<Pos> {
        if !self.viewport.contains_pos(screen) {
            return None;
        }
        // Safe without saturating: `origin` is only ever written by `center_on`/`set_viewport`,
        // both of which clamp it via `Rect::clamp_within` to `[0, world - view]`, and
        // `contains_pos` above guarantees the offset is `< view`, so the sum is `< world <=
        // u16::MAX`.
        let wx = self.origin.x + (screen.x - self.viewport.left());
        let wy = self.origin.y + (screen.y - self.viewport.top());
        if wx >= self.world.width() || wy >= self.world.height() {
            return None;
        }
        Some(Pos::new(wx, wy))
    }

    /// Map a screen position back to a world position, without culling: the result may fall
    /// outside the viewport or outside `[0, world)`, instead of coming back `None`.
    ///
    /// [`screen_to_world`](Self::screen_to_world) is the right call when the only question is
    /// "which world cell is under this screen position" (mouse picking, a single-cell cursor).
    /// It falls short once a gesture can leave the viewport or the world mid-flight: a pointer
    /// drag that overshoots the edge, or a rubber-band selection rect that extends past it, has
    /// no `Pos` to report and no way to compute a world-space delta. This is the signed sibling
    /// for that case: it hands back the same math `screen_to_world` computes, minus the culling.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{Camera, Pos, Rect, Size};
    ///
    /// let mut cam = Camera::new(Rect::new(5, 5, 10, 10), Size::new(100, 100));
    /// cam.center_on(Pos::new(50, 50));
    ///
    /// // Inside the viewport: matches `screen_to_world`.
    /// assert_eq!(cam.screen_to_world_signed(Pos::new(5, 5)), (45, 45));
    ///
    /// // Off the viewport entirely (the viewport starts at x = 5): negative, not `None`, so a
    /// // caller mid-drag can still compute a world-space delta.
    /// assert_eq!(cam.screen_to_world_signed(Pos::new(0, 0)), (40, 40));
    /// ```
    #[must_use]
    pub const fn screen_to_world_signed(&self, screen: Pos) -> (i32, i32) {
        let dx = screen.x as i32 - self.viewport.left() as i32;
        let dy = screen.y as i32 - self.viewport.top() as i32;
        (self.origin.x as i32 + dx, self.origin.y as i32 + dy)
    }

    /// Iterate the visible cells as `(world, screen)` position pairs, in
    /// row-major order. Only cells that exist in the world are yielded, so the
    /// caller can fill the rest of the viewport with a background.
    #[must_use = "iterators are lazy and do nothing unless consumed"]
    pub fn cells(&self) -> impl Iterator<Item = (Pos, Pos)> {
        let vis = self.visible_bounds();
        let vp = self.viewport;
        let origin = self.origin;
        (vis.top()..vis.bottom()).flat_map(move |wy| {
            (vis.left()..vis.right()).map(move |wx| {
                let screen = Pos::new(
                    vp.left().saturating_add(wx - origin.x),
                    vp.top().saturating_add(wy - origin.y),
                );
                (Pos::new(wx, wy), screen)
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::cam;
    use super::*;
    use crate::grid::Size;

    #[test]
    fn screen_to_world_is_none_outside_the_viewport_or_past_the_world_edge() {
        let mut c = cam();
        c.center_on(Pos::new(50, 50)); // shows world [45, 55).
        // Off the viewport entirely: the viewport starts at x = 0, so a negative screen
        // position never reaches `contains_pos`.
        assert_eq!(c.screen_to_world(Pos::new(20, 20)), None);

        // Inside the viewport, but the mapped world position is past the world edge: a 5x5
        // world with a 10x10 viewport pinned to (0, 0) leaves the bottom-right quadrant of
        // the viewport mapping past `world`.
        let small = Camera::new(Rect::new(0, 0, 10, 10), Size::new(5, 5));
        assert_eq!(small.screen_to_world(Pos::new(9, 9)), None);
    }

    #[test]
    fn offscreen_positions_return_none() {
        let mut c = cam();
        c.center_on(Pos::new(50, 50)); // shows world [45,55)
        assert_eq!(c.world_to_screen(Pos::new(44, 50)), None);
        assert_eq!(c.world_to_screen(Pos::new(55, 50)), None);
    }

    #[test]
    fn world_to_screen_rejects_a_zero_size_world() {
        let c = Camera::new(Rect::new(0, 0, 10, 10), Size::new(0, 0));
        // An empty world has no cells to map, matching `screen_to_world`'s equivalent None.
        assert_eq!(c.world_to_screen(Pos::new(0, 0)), None);
        assert_eq!(c.screen_to_world(Pos::new(0, 0)), None);
    }

    #[test]
    fn screen_to_world_returns_none_past_the_world_edge_within_the_viewport() {
        // A 20x20 viewport over a 5x5 world: the origin pins to (0, 0), so the viewport has a
        // dead margin past (5, 5) that is inside the viewport but outside the world.
        let c = Camera::new(Rect::new(2, 2, 20, 20), Size::new(5, 5));
        // Inside the viewport, but past the world edge: the mouse-picking case the guard exists
        // for, not `None` from missing the viewport.
        assert_eq!(c.screen_to_world(Pos::new(10, 10)), None);
        // Just inside the world edge still resolves normally.
        assert_eq!(c.screen_to_world(Pos::new(6, 6)), Some(Pos::new(4, 4)));
    }

    #[test]
    fn cells_only_yields_cells_that_exist_in_the_world() {
        use alloc::vec::Vec;

        // A 20x20 viewport over a 5x5 world: the clamp in `visible_bounds` is doing real work
        // here, unlike the full-viewport case above.
        let c = Camera::new(Rect::new(0, 0, 20, 20), Size::new(5, 5));
        let pairs: Vec<_> = c.cells().collect();
        assert_eq!(pairs.len(), 25); // 5x5 world, not the 20x20 viewport.
        assert_eq!(pairs[0], (Pos::new(0, 0), Pos::new(0, 0)));
        assert_eq!(pairs[24], (Pos::new(4, 4), Pos::new(4, 4)));
    }

    #[test]
    fn surface_clips_to_the_letterboxed_viewport_after_set_viewport_fitted() {
        use crate::color::Style;
        use crate::grid::Grid;

        let mut grid = Grid::new(20, 20);
        let mut root = Surface::new(&mut grid, Rect::new(0, 0, 20, 20), 0);

        let mut c = Camera::new(Rect::new(0, 0, 1, 1), Size::new(5, 5));
        c.set_viewport_fitted(Rect::new(0, 0, 20, 20));
        assert_eq!(c.viewport(), Rect::new(7, 7, 5, 5)); // shrunk to the world and centered.

        let mut world = c.surface(&mut root);
        // The world origin lands at the letterboxed viewport's own top-left, not the grid's.
        world.put(Pos::new(0, 0), '@', Style::default());
        // Outside the shrunk viewport (but still inside the un-fitted 20x20 rect passed in):
        // dropped, the same as `world_to_screen` returning `None` for it, not drawn into the
        // dead margin.
        world.put(Pos::new(10, 10), 'X', Style::default());

        assert_eq!(grid[Pos::new(7, 7)].glyph(), '@');
        assert_eq!(grid[Pos::new(0, 0)].glyph(), ' '); // untouched margin cell.
    }

    #[test]
    fn cells_yields_visible_world_and_screen_pairs() {
        use alloc::vec::Vec;

        let mut c = cam();
        c.center_on(Pos::new(50, 50));
        let pairs: Vec<_> = c.cells().collect();
        assert_eq!(pairs.len(), 100); // 10x10 viewport, world larger
        assert_eq!(pairs[0], (Pos::new(45, 45), Pos::new(0, 0)));
        assert_eq!(pairs[99], (Pos::new(54, 54), Pos::new(9, 9)));
    }

    #[test]
    fn world_to_offset_matches_world_to_screen_when_visible() {
        let mut c = cam();
        c.center_on(Pos::new(50, 50));
        assert_eq!(c.world_to_offset(Pos::new(50, 50)), (5, 5));
        assert_eq!(c.world_to_screen(Pos::new(50, 50)), Some(Pos::new(5, 5)));
    }

    #[test]
    fn world_to_offset_goes_negative_past_the_low_edge_instead_of_culling() {
        let mut c = cam();
        c.center_on(Pos::new(50, 50)); // shows world [45, 55).
        assert_eq!(c.world_to_offset(Pos::new(44, 50)), (-1, 5));
        // The same position through `world_to_screen`: culled, not negative.
        assert_eq!(c.world_to_screen(Pos::new(44, 50)), None);
    }

    #[test]
    fn world_to_offset_goes_past_the_far_edge_instead_of_culling() {
        let mut c = cam();
        c.center_on(Pos::new(50, 50)); // shows world [45, 55).
        assert_eq!(c.world_to_offset(Pos::new(55, 50)), (10, 5));
        assert_eq!(c.world_to_screen(Pos::new(55, 50)), None);
    }

    #[test]
    fn world_to_offset_includes_a_non_zero_viewport_origin() {
        let mut c = Camera::new(Rect::new(5, 5, 10, 10), Size::new(100, 100));
        c.center_on(Pos::new(50, 50));
        assert_eq!(c.world_to_offset(Pos::new(50, 50)), (10, 10));
    }

    #[test]
    fn screen_to_world_signed_matches_screen_to_world_when_visible() {
        let mut c = Camera::new(Rect::new(5, 5, 10, 10), Size::new(100, 100));
        c.center_on(Pos::new(50, 50));
        assert_eq!(c.screen_to_world_signed(Pos::new(5, 5)), (45, 45));
        assert_eq!(c.screen_to_world(Pos::new(5, 5)), Some(Pos::new(45, 45)));
    }

    #[test]
    fn screen_to_world_signed_goes_negative_before_the_viewport_instead_of_culling() {
        let mut c = Camera::new(Rect::new(5, 5, 10, 10), Size::new(100, 100));
        c.center_on(Pos::new(50, 50)); // origin (45, 45), viewport starts at (5, 5).
        assert_eq!(c.screen_to_world_signed(Pos::new(0, 0)), (40, 40));
        // The same screen position through `screen_to_world`: culled, not negative.
        assert_eq!(c.screen_to_world(Pos::new(0, 0)), None);
    }

    #[test]
    fn screen_to_world_signed_goes_past_the_world_edge_instead_of_culling() {
        let mut c = cam(); // viewport (0, 0, 10, 10), world 100x100.
        c.center_on(Pos::new(99, 99));
        assert_eq!(c.origin(), Pos::new(90, 90)); // origin clamps so origin + viewport = world.
        // One column/row past the viewport's own far edge, so past the world edge too:
        // `screen_to_world` culls (out of viewport), the signed sibling keeps counting.
        assert_eq!(c.screen_to_world_signed(Pos::new(10, 10)), (100, 100));
        assert_eq!(c.screen_to_world(Pos::new(10, 10)), None);
    }

    #[test]
    fn surface_draws_a_multi_cell_anchor_that_is_off_viewport() {
        use crate::color::Style;
        use crate::grid::Grid;

        // The scenario retroglyph#614 could not express: a two-cell-wide sprite whose anchor
        // sits one world column left of the visible range, so only its right half is on screen.
        let mut grid = Grid::new(20, 20);
        let mut root = Surface::new(&mut grid, Rect::new(0, 0, 20, 20), 0);

        let mut c = cam(); // viewport (0, 0, 10, 10), world 100x100.
        c.center_on(Pos::new(50, 50)); // shows world [45, 55).

        let mut view = c.surface(&mut root);
        // The anchor: one column left of the visible world range. `world_to_screen` would cull
        // this entirely, so a caller stuck with it could not draw the sprite's visible half
        // either. Drawn in world coordinates through `Camera::surface`, it is just off-grid and
        // silently dropped, like any other out-of-bounds `put`.
        view.put(Pos::new(44, 50), '[', Style::default());
        // The sprite's other half: the viewport's own leftmost visible column.
        view.put(Pos::new(45, 50), ']', Style::default());

        assert_eq!(grid[Pos::new(0, 5)].glyph(), ']');
    }

    #[test]
    fn surface_print_and_print_line_wrap_at_the_viewport_not_one_step_after_the_origin() {
        use crate::color::Style;
        use crate::grid::Grid;
        use crate::text::{Line, Span};
        use alloc::vec;

        // retroglyph#991: `print`/`print_line`'s wrap threshold used to compare a
        // translated-space column against an area-local one, so it fired one grapheme after
        // `origin_offset.0` on any surface `Camera::surface` produces, instead of at the
        // viewport's own width.
        let mut grid = Grid::new(20, 20);
        let mut c = Camera::new(Rect::new(5, 5, 10, 10), Size::new(100, 100));
        c.center_on(Pos::new(50, 50)); // origin (45, 45): world (50, 50) is local (5, 5).

        {
            let mut root = Surface::new(&mut grid, Rect::new(0, 0, 20, 20), 0);
            c.surface(&mut root)
                .print(Pos::new(50, 50), "abc", Style::default());
        }
        // Written across row 10 starting at column 10, not down column 10 one glyph per row.
        assert_eq!(grid[Pos::new(10, 10)].glyph(), 'a');
        assert_eq!(grid[Pos::new(11, 10)].glyph(), 'b');
        assert_eq!(grid[Pos::new(12, 10)].glyph(), 'c');
        assert_eq!(grid[Pos::new(10, 11)].glyph(), ' ');

        let line = Line::from(vec![Span::raw("de"), Span::raw("fg")]);
        {
            let mut root = Surface::new(&mut grid, Rect::new(0, 0, 20, 20), 0);
            c.surface(&mut root).print_line(Pos::new(50, 51), &line);
        }
        // Both spans still land; the old bug's `break` fired on the very first span.
        assert_eq!(grid[Pos::new(10, 11)].glyph(), 'd');
        assert_eq!(grid[Pos::new(11, 11)].glyph(), 'e');
        assert_eq!(grid[Pos::new(12, 11)].glyph(), 'f');
        assert_eq!(grid[Pos::new(13, 11)].glyph(), 'g');
    }

    #[test]
    fn world_to_screen_saturates_instead_of_overflowing() {
        let c = Camera::new(Rect::new(65_530, 0, 10, 10), Size::new(100, 100));
        assert_eq!(
            c.world_to_screen(Pos::new(9, 0)),
            Some(Pos::new(u16::MAX, 0))
        );
    }

    #[test]
    fn cells_saturates_instead_of_overflowing() {
        let c = Camera::new(Rect::new(65_530, 0, 10, 10), Size::new(100, 100));
        let (_, screen) = c
            .cells()
            .find(|(world, _)| *world == Pos::new(9, 0))
            .expect("world (9, 0) is within the visible bounds");
        assert_eq!(screen, Pos::new(u16::MAX, 0));
    }

    #[test]
    fn surface_clips_to_the_world_not_the_viewport_when_the_world_is_smaller() {
        use crate::color::Style;
        use crate::grid::Grid;

        // A 20x20 viewport over a 5x5 world: `set_viewport` (not `set_viewport_fitted`) pins
        // the origin at (0, 0) and leaves the dead margin to the right and bottom of the world.
        let mut grid = Grid::new(20, 20);
        let mut root = Surface::new(&mut grid, Rect::new(0, 0, 20, 20), 0);
        let c = Camera::new(Rect::new(0, 0, 20, 20), Size::new(5, 5));

        let mut view = c.surface(&mut root);
        view.put(Pos::new(0, 0), '@', Style::default());
        // Inside the viewport but past the (smaller) world's edge: dropped, matching
        // `world_to_screen`/`visible_bounds`, not reaching the dead margin past the world.
        view.put(Pos::new(10, 10), 'X', Style::default());

        assert_eq!(grid[Pos::new(0, 0)].glyph(), '@');
        assert_eq!(grid[Pos::new(10, 10)].glyph(), ' ');
    }
}
