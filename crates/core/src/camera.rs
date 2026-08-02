//! A scrolling viewport into a world larger than the screen.
//!
//! [`Camera`] is pure geometry: it converts between world coordinates (cells in
//! some large space) and screen coordinates (cells in a [`Rect`] on the
//! terminal), and reports which world cells are currently visible. It holds no
//! rendering opinion, so it works with any drawing style and is testable
//! without a backend.
//!
//! Centering clamps to the world edges (the "scrolling map" convention): the
//! viewport never scrolls past `[0, world)`, so the target stays centered
//! except near the edges, where it drifts toward the corner. A world smaller
//! than the viewport pins the origin at `(0, 0)`.
//!
//! See the `12_dungeon_scroll` example for `Camera` in action:
//! <https://main.retroglyph.dev/examples/12_dungeon_scroll/terminal/>.
//!
//! [`Grid::from_charmap`](crate::Grid::from_charmap) builds a styled grid from an ASCII map or
//! level string, one tile per character; combined with a [`Camera`] and multi-layer compositing,
//! this is how a scrolling roguelike loads and follows a map larger than the screen (see the
//! `11_sokoban` example for `from_charmap` itself, and `15_outpost_dashboard` for a `Camera` used
//! alongside a UI).
//!
//! # Example
//!
//! ```
//! use retroglyph_core::{Camera, Pos, Rect, Size};
//!
//! // A 10x10 viewport onto a 100x100 world.
//! let mut cam = Camera::new(Rect::new(0, 0, 10, 10), Size::new(100, 100));
//! cam.center_on(Pos::new(50, 50));
//! assert_eq!(cam.origin(), Pos::new(45, 45));
//! assert_eq!(cam.world_to_screen(Pos::new(50, 50)), Some(Pos::new(5, 5)));
//! // Near an edge the view clamps rather than showing past the world.
//! cam.center_on(Pos::new(1, 1));
//! assert_eq!(cam.origin(), Pos::new(0, 0));
//! ```

use crate::grid::{Pos, Rect, Size};
use crate::surface::Surface;

/// A rectangular viewport onto a larger world, with world/screen conversions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Camera {
    viewport: Rect,
    world: Size,
    origin: Pos,
}

impl Camera {
    /// Create a camera drawing into `viewport` (screen cells) over a world of
    /// `world` cells. The initial origin is `(0, 0)`; call
    /// [`center_on`](Self::center_on) to follow a target.
    #[must_use]
    pub const fn new(viewport: Rect, world: Size) -> Self {
        Self {
            viewport,
            world,
            origin: Pos::new(0, 0),
        }
    }

    /// The screen rectangle the world is drawn into.
    #[must_use]
    pub const fn viewport(&self) -> Rect {
        self.viewport
    }

    /// The world dimensions.
    #[must_use]
    pub const fn world(&self) -> Size {
        self.world
    }

    /// The world cell shown at the viewport's top-left corner.
    #[must_use]
    pub const fn origin(&self) -> Pos {
        self.origin
    }

    /// Replace the viewport (for example after a terminal resize), keeping the
    /// world unchanged and re-clamping the origin so it stays in bounds.
    ///
    /// Never panics: a `viewport` larger than `world` re-clamps the origin to `(0, 0)` via
    /// [`saturating_sub`](u16::saturating_sub) rather than underflowing.
    pub fn set_viewport(&mut self, viewport: Rect) {
        self.viewport = viewport;
        self.origin = Pos::new(
            self.origin
                .x
                .min(max_origin(viewport.width(), self.world.width())),
            self.origin
                .y
                .min(max_origin(viewport.height(), self.world.height())),
        );
    }

    /// Center the view on `target` (world coords), clamped to the world edges so
    /// the viewport never scrolls past `[0, world)`.
    ///
    /// Never panics, even for a `target` outside `[0, world)`: the offset and clamp are both
    /// computed with saturating arithmetic.
    pub fn center_on(&mut self, target: Pos) {
        self.origin = Pos::new(
            center_axis(target.x, self.viewport.width(), self.world.width()),
            center_axis(target.y, self.viewport.height(), self.world.height()),
        );
    }

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
        let w = self
            .viewport
            .width()
            .min(self.world.width().saturating_sub(self.origin.x));
        let h = self
            .viewport
            .height()
            .min(self.world.height().saturating_sub(self.origin.y));
        Rect::new(self.origin.x, self.origin.y, w, h)
    }

    /// Map a world position to its screen position, or `None` if it is outside
    /// the visible viewport.
    #[must_use]
    pub const fn world_to_screen(&self, world: Pos) -> Option<Pos> {
        if world.x < self.origin.x || world.y < self.origin.y {
            return None;
        }
        let dx = world.x - self.origin.x;
        let dy = world.y - self.origin.y;
        if dx >= self.viewport.width() || dy >= self.viewport.height() {
            return None;
        }
        Some(Pos::new(
            self.viewport.left() + dx,
            self.viewport.top() + dy,
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
    /// [`viewport`](Self::viewport): [`Surface::clip_translate`] to the viewport, by
    /// [`origin`](Self::origin).
    ///
    /// The returned surface's `put`, `put_signed`, `print`, and the rest of `Surface`'s
    /// coordinate-taking methods all take world coordinates directly, and anything that lands
    /// outside the current viewport (including a multi-cell draw anchored off-screen) is
    /// dropped by the surface's own bounds check, the same way [`world_to_offset`] composes with
    /// [`Surface::put_signed`] by hand. This is that composition done once instead of at every
    /// call site.
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
    #[must_use]
    pub fn surface<'a>(&self, surface: &'a mut Surface<'_>) -> Surface<'a> {
        surface.clip_translate(
            self.viewport,
            (i32::from(self.origin.x), i32::from(self.origin.y)),
        )
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
        let wx = self.origin.x + (screen.x - self.viewport.left());
        let wy = self.origin.y + (screen.y - self.viewport.top());
        if wx >= self.world.width() || wy >= self.world.height() {
            return None;
        }
        Some(Pos::new(wx, wy))
    }

    /// Iterate the visible cells as `(world, screen)` position pairs, in
    /// row-major order. Only cells that exist in the world are yielded, so the
    /// caller can fill the rest of the viewport with a background.
    #[must_use = "iterators are lazy and do nothing unless consumed"]
    pub fn cells(&self) -> impl Iterator<Item = (Pos, Pos)> + '_ {
        let vis = self.visible_bounds();
        let vp = self.viewport;
        let origin = self.origin;
        (vis.top()..vis.bottom()).flat_map(move |wy| {
            (vis.left()..vis.right()).map(move |wx| {
                let screen = Pos::new(vp.left() + (wx - origin.x), vp.top() + (wy - origin.y));
                (Pos::new(wx, wy), screen)
            })
        })
    }
}

/// The largest in-bounds origin for a `view`-wide window over `[0, world)`.
/// Zero when the world is no larger than the view.
const fn max_origin(view: u16, world: u16) -> u16 {
    world.saturating_sub(view)
}

/// Origin that centers `target` in a `view`-wide window, clamped to bounds.
fn center_axis(target: u16, view: u16, world: u16) -> u16 {
    target.saturating_sub(view / 2).min(max_origin(view, world))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam() -> Camera {
        Camera::new(Rect::new(0, 0, 10, 10), Size::new(100, 100))
    }

    #[test]
    fn centers_in_the_interior() {
        let mut c = cam();
        c.center_on(Pos::new(50, 50));
        assert_eq!(c.origin(), Pos::new(45, 45));
        assert_eq!(c.world_to_screen(Pos::new(50, 50)), Some(Pos::new(5, 5)));
        assert_eq!(c.screen_to_world(Pos::new(5, 5)), Some(Pos::new(50, 50)));
    }

    #[test]
    fn clamps_at_the_low_edge() {
        let mut c = cam();
        c.center_on(Pos::new(1, 1));
        assert_eq!(c.origin(), Pos::new(0, 0));
        assert_eq!(c.world_to_screen(Pos::new(1, 1)), Some(Pos::new(1, 1)));
    }

    #[test]
    fn clamps_at_the_high_edge() {
        let mut c = cam();
        c.center_on(Pos::new(99, 99));
        // origin = min(99 - 5, 100 - 10) = min(94, 90) = 90.
        assert_eq!(c.origin(), Pos::new(90, 90));
        assert_eq!(c.world_to_screen(Pos::new(99, 99)), Some(Pos::new(9, 9)));
    }

    #[test]
    fn offscreen_positions_return_none() {
        let mut c = cam();
        c.center_on(Pos::new(50, 50)); // shows world [45,55)
        assert_eq!(c.world_to_screen(Pos::new(44, 50)), None);
        assert_eq!(c.world_to_screen(Pos::new(55, 50)), None);
    }

    #[test]
    fn world_smaller_than_viewport_pins_origin() {
        let mut c = Camera::new(Rect::new(2, 2, 20, 20), Size::new(5, 5));
        c.center_on(Pos::new(3, 3));
        assert_eq!(c.origin(), Pos::new(0, 0));
        let visible = c.visible_bounds();
        assert_eq!((visible.width(), visible.height()), (5, 5));
        // Cells map into the viewport, offset by its top-left.
        assert_eq!(c.world_to_screen(Pos::new(0, 0)), Some(Pos::new(2, 2)));
    }

    #[test]
    fn cells_yields_visible_world_and_screen_pairs() {
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
    fn surface_draws_a_multi_cell_anchor_that_is_off_viewport() {
        use crate::grid::Grid;
        use crate::style::Style;

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
}
