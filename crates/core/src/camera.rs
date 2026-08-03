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
//! than the viewport pins the origin at `(0, 0)`, with all the slack on the
//! right and bottom of the given viewport rect; use
//! [`set_viewport_fitted`](Camera::set_viewport_fitted) instead of
//! [`set_viewport`](Camera::set_viewport) when a world that may be smaller
//! than its viewport (a fixed board, a generated map, a minimap) should be
//! letterboxed and centered instead.
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
use ixy::HasSize;

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
        self.set_origin(self.origin);
    }

    /// Replace the world dimensions (for example when a level changes), keeping the viewport
    /// unchanged and re-clamping the origin so it stays in bounds.
    ///
    /// If the camera was last positioned with
    /// [`set_viewport_fitted`](Self::set_viewport_fitted), this does not re-run that letterboxing
    /// against the new world; call `set_viewport_fitted` again afterward if the new world may be
    /// smaller than the viewport on either axis.
    ///
    /// Never panics: the re-clamp uses the same saturating arithmetic as
    /// [`set_viewport`](Self::set_viewport).
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{Camera, Pos, Rect, Size};
    ///
    /// let mut cam = Camera::new(Rect::new(0, 0, 10, 10), Size::new(100, 100));
    /// cam.center_on(Pos::new(50, 50));
    /// assert_eq!(cam.origin(), Pos::new(45, 45));
    ///
    /// // Shrinking the world re-clamps the origin so it stays in bounds.
    /// cam.set_world(Size::new(20, 20));
    /// assert_eq!(cam.world(), Size::new(20, 20));
    /// assert_eq!(cam.origin(), Pos::new(10, 10));
    /// ```
    pub fn set_world(&mut self, world: Size) {
        self.world = world;
        self.origin = Pos::new(
            self.origin
                .x
                .min(max_origin(self.viewport.width(), world.width())),
            self.origin
                .y
                .min(max_origin(self.viewport.height(), world.height())),
        );
    }

    /// Replace the viewport like [`set_viewport`](Self::set_viewport), but shrink it to the
    /// world's size on any axis where the world is smaller, and center the shrunk rect within
    /// `viewport` rather than pinning it to the top-left.
    ///
    /// A viewport at least as large as the world on both axes lands exactly on the world with no
    /// slack, so `origin` is `(0, 0)` and [`viewport`](Self::viewport) reports that centered
    /// rect, not `viewport` itself; hit-testing via [`screen_to_world`](Self::screen_to_world)
    /// therefore only recognizes screen positions actually over the world, not the letterboxed
    /// margin. This is the fix for the pinned-to-the-corner behaviour
    /// [`set_viewport`](Self::set_viewport) has for a world smaller than the viewport: a fixed
    /// board, a generated map of fixed dimensions, or a minimap drawn into a terminal whose size
    /// the app does not control.
    ///
    /// Odd leftover slack rounds down, the same way [`center_on`](Self::center_on) rounds: any
    /// extra cell of margin lands on the right or bottom, not the left or top.
    ///
    /// Never panics: all arithmetic is saturating, so a `viewport` narrower than it is tall (or
    /// vice versa) relative to `world` cannot underflow.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{Camera, Pos, Rect, Size};
    ///
    /// // A 20x20 viewport at (2, 2) over a 5x5 world: the effective viewport shrinks to 5x5
    /// // and centers within the given rect, instead of pinning to (2, 2).
    /// let mut cam = Camera::new(Rect::new(0, 0, 1, 1), Size::new(5, 5));
    /// cam.set_viewport_fitted(Rect::new(2, 2, 20, 20));
    /// assert_eq!(cam.viewport(), Rect::new(9, 9, 5, 5));
    /// assert_eq!(cam.origin(), Pos::new(0, 0));
    ///
    /// // A viewport already no larger than the world on both axes behaves like `set_viewport`:
    /// // no shrinking, no centering.
    /// let mut cam = Camera::new(Rect::new(0, 0, 1, 1), Size::new(100, 100));
    /// cam.set_viewport_fitted(Rect::new(0, 0, 10, 10));
    /// assert_eq!(cam.viewport(), Rect::new(0, 0, 10, 10));
    /// ```
    pub fn set_viewport_fitted(&mut self, viewport: Rect) {
        let width = viewport.width().min(self.world.width());
        let height = viewport.height().min(self.world.height());
        let x = viewport.left() + (viewport.width() - width) / 2;
        let y = viewport.top() + (viewport.height() - height) / 2;
        self.set_viewport(Rect::new(x, y, width, height));
    }

    /// Center the view on `target` (world coords), clamped to the world edges so
    /// the viewport never scrolls past `[0, world)`.
    ///
    /// Never panics, even for a `target` outside `[0, world)`: the offset and clamp are both
    /// computed with saturating arithmetic.
    pub fn center_on(&mut self, target: Pos) {
        self.set_origin(Pos::new(
            target.x.saturating_sub(self.viewport.width() / 2),
            target.y.saturating_sub(self.viewport.height() / 2),
        ));
    }

    /// Set the top-left world cell directly, clamped to the world edges so `origin` never
    /// scrolls past `[0, world)`, the same invariant [`center_on`](Self::center_on) maintains.
    ///
    /// This is the primitive [`center_on`](Self::center_on) and [`scroll_by`](Self::scroll_by)
    /// both clamp through, and what a save/restore of camera state needs: [`origin`](Self::origin)
    /// is otherwise read-only.
    ///
    /// Never panics: the clamp uses [`saturating_sub`](u16::saturating_sub) via the same
    /// `max_origin` helper [`set_viewport`](Self::set_viewport) uses, so it cannot underflow
    /// even for a `viewport` larger than `world`.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{Camera, Pos, Rect, Size};
    ///
    /// let mut cam = Camera::new(Rect::new(0, 0, 10, 10), Size::new(100, 100));
    /// cam.set_origin(Pos::new(50, 50));
    /// assert_eq!(cam.origin(), Pos::new(50, 50));
    ///
    /// // Clamped to `world - viewport`, same as `center_on`.
    /// cam.set_origin(Pos::new(200, 200));
    /// assert_eq!(cam.origin(), Pos::new(90, 90));
    /// ```
    pub fn set_origin(&mut self, origin: Pos) {
        self.origin = Pos::new(
            origin
                .x
                .min(max_origin(self.viewport.width(), self.world.width())),
            origin
                .y
                .min(max_origin(self.viewport.height(), self.world.height())),
        );
    }

    /// Scroll the view by a signed cell delta, clamped to the world edges like
    /// [`set_origin`](Self::set_origin).
    ///
    /// This is the method a drag or a scroll wheel wants: unlike [`center_on`](Self::center_on),
    /// which reinterprets its argument as a new target to center on, `scroll_by` moves `origin`
    /// directly, so there is exactly one clamp between the input delta and the visible result.
    /// A caller that instead clamps its own running "center" position to `[0, world)` and feeds
    /// it through `center_on` every frame is clamping against a wider range than `center_on`'s
    /// own `[0, world - viewport]`, which leaves slack: dragging past an edge no longer moves
    /// the origin, but the caller's tracked position keeps moving, so dragging back "sticks"
    /// until it works through that slack before the view responds again.
    ///
    /// Never panics: the delta is applied in `i32` and saturates at `0` or `u16::MAX` before the
    /// world-edge clamp in [`set_origin`](Self::set_origin) runs, so neither a very large
    /// negative nor positive `dx`/`dy` can overflow or underflow `u16`.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{Camera, Pos, Rect, Size};
    ///
    /// let mut cam = Camera::new(Rect::new(0, 0, 10, 10), Size::new(100, 100));
    /// cam.scroll_by(5, 3);
    /// assert_eq!(cam.origin(), Pos::new(5, 3));
    ///
    /// // Clamped at the world edge, same as `center_on`: no negative or past-`world` origin.
    /// cam.scroll_by(-100, -100);
    /// assert_eq!(cam.origin(), Pos::new(0, 0));
    /// ```
    pub fn scroll_by(&mut self, dx: i32, dy: i32) {
        self.set_origin(Pos::new(
            saturating_offset(self.origin.x, dx),
            saturating_offset(self.origin.y, dy),
        ));
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

/// Adds a signed delta to a `u16` coordinate, clamped to `[0, u16::MAX]` instead of wrapping.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to [0, u16::MAX] above
fn saturating_offset(value: u16, delta: i32) -> u16 {
    (i32::from(value) + delta).clamp(0, i32::from(u16::MAX)) as u16
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
        // A cell inside the viewport but outside the (smaller) world: rejected, matching
        // `visible_bounds` and `screen_to_world`, not silently mapped past the world edge.
        assert_eq!(c.world_to_screen(Pos::new(7, 7)), None);
    }

    #[test]
    fn set_world_re_clamps_the_origin_when_the_world_shrinks() {
        let mut c = cam();
        c.center_on(Pos::new(50, 50));
        assert_eq!(c.origin(), Pos::new(45, 45));

        c.set_world(Size::new(20, 20));
        assert_eq!(c.world(), Size::new(20, 20));
        // max_origin(10, 20) = 10, so origin clamps down from 45 to 10.
        assert_eq!(c.origin(), Pos::new(10, 10));
    }

    #[test]
    fn set_world_leaves_an_in_bounds_origin_unchanged_when_the_world_grows() {
        let mut c = cam();
        c.center_on(Pos::new(50, 50));
        assert_eq!(c.origin(), Pos::new(45, 45));

        c.set_world(Size::new(200, 200));
        assert_eq!(c.world(), Size::new(200, 200));
        assert_eq!(c.origin(), Pos::new(45, 45));
    }

    #[test]
    fn set_world_pins_origin_to_zero_when_the_new_world_is_smaller_than_the_viewport() {
        let mut c = cam();
        c.center_on(Pos::new(50, 50));

        c.set_world(Size::new(5, 5));
        assert_eq!(c.origin(), Pos::new(0, 0));
    }

    #[test]
    fn set_viewport_fitted_shrinks_and_centers_a_world_smaller_on_both_axes() {
        let mut c = Camera::new(Rect::new(0, 0, 1, 1), Size::new(5, 5));
        c.set_viewport_fitted(Rect::new(2, 2, 20, 20));
        assert_eq!(c.viewport(), Rect::new(9, 9, 5, 5));
        assert_eq!(c.origin(), Pos::new(0, 0));
        assert_eq!(c.visible_bounds(), Rect::new(0, 0, 5, 5));
        assert_eq!(c.world_to_screen(Pos::new(0, 0)), Some(Pos::new(9, 9)));
    }

    #[test]
    fn set_viewport_fitted_shrinks_only_the_axis_that_is_smaller() {
        // World is smaller than the viewport on x only.
        let mut c = Camera::new(Rect::new(0, 0, 1, 1), Size::new(5, 100));
        c.set_viewport_fitted(Rect::new(0, 0, 20, 10));
        assert_eq!(c.viewport(), Rect::new(7, 0, 5, 10));
    }

    #[test]
    fn set_viewport_fitted_rounds_odd_slack_toward_the_right_and_bottom() {
        let mut c = Camera::new(Rect::new(0, 0, 1, 1), Size::new(4, 4));
        c.set_viewport_fitted(Rect::new(0, 0, 9, 9));
        // 9 - 4 = 5 of slack, split 2/3: two columns/rows left and top, three right and bottom.
        assert_eq!(c.viewport(), Rect::new(2, 2, 4, 4));
    }

    #[test]
    fn set_viewport_reclamps_the_origin_when_the_viewport_grows() {
        let mut c = cam(); // 10x10 viewport, 100x100 world.
        c.center_on(Pos::new(99, 99)); // origin (90, 90).
        c.set_viewport(Rect::new(0, 0, 40, 40));
        assert_eq!(c.origin(), Pos::new(60, 60)); // 100 - 40.
        assert_eq!(c.visible_bounds(), Rect::new(60, 60, 40, 40));
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
    fn zero_size_camera_is_inert() {
        let c = Camera::new(Rect::new(0, 0, 0, 0), Size::new(0, 0));
        assert_eq!(c.visible_bounds(), Rect::new(0, 0, 0, 0));
        assert_eq!(c.cells().count(), 0);
        assert_eq!(c.world_to_screen(Pos::new(0, 0)), None);
        assert_eq!(c.screen_to_world(Pos::new(0, 0)), None);

        // A zero-size world under a normal viewport behaves the same way: nothing to show.
        // `world_to_screen` only checks against the viewport, not `world`, so it is
        // `screen_to_world` (which does check `world`) that actually guards this case.
        let zero_world = Camera::new(Rect::new(0, 0, 10, 10), Size::new(0, 0));
        assert_eq!(zero_world.visible_bounds(), Rect::new(0, 0, 0, 0));
        assert_eq!(zero_world.cells().count(), 0);
        assert_eq!(zero_world.screen_to_world(Pos::new(0, 0)), None);
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
    fn set_viewport_fitted_matches_set_viewport_when_the_world_is_not_smaller() {
        let mut a = Camera::new(Rect::new(0, 0, 1, 1), Size::new(100, 100));
        a.set_viewport_fitted(Rect::new(0, 0, 10, 10));

        let mut b = Camera::new(Rect::new(0, 0, 1, 1), Size::new(100, 100));
        b.set_viewport(Rect::new(0, 0, 10, 10));

        assert_eq!(a.viewport(), b.viewport());
        assert_eq!(a.origin(), b.origin());
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

    #[test]
    fn scroll_by_moves_the_origin_by_the_delta() {
        let mut c = cam();
        c.scroll_by(5, 3);
        assert_eq!(c.origin(), Pos::new(5, 3));
        c.scroll_by(-2, 1);
        assert_eq!(c.origin(), Pos::new(3, 4));
    }

    #[test]
    fn scroll_by_clamps_at_the_low_edge_without_overshooting() {
        let mut c = cam();
        c.scroll_by(-1000, -1000);
        assert_eq!(c.origin(), Pos::new(0, 0));
    }

    #[test]
    fn scroll_by_clamps_at_the_high_edge_without_overshooting() {
        let mut c = cam(); // viewport 10x10, world 100x100: max origin is (90, 90).
        c.scroll_by(1000, 1000);
        assert_eq!(c.origin(), Pos::new(90, 90));
    }

    #[test]
    fn scroll_by_has_no_dead_zone_reversing_direction_past_an_edge() {
        // The bug `scroll_by` replaces: a caller that clamps its own "center" to `[0, world)`
        // and re-derives the origin via `center_on` every frame accumulates slack once the
        // center clamps past what `center_on`'s own `[0, world - viewport]` clamp allows, so
        // reversing direction doesn't move the origin until that slack is used up. `scroll_by`
        // has one clamp on `origin` itself, so the very next opposite-direction scroll moves it.
        let mut c = cam(); // viewport 10x10, world 100x100: max origin is (90, 90).
        c.scroll_by(1000, 0); // drive past the edge; origin clamps to (90, 0).
        assert_eq!(c.origin(), Pos::new(90, 0));
        c.scroll_by(-1, 0); // reverse by a single cell.
        assert_eq!(
            c.origin(),
            Pos::new(89, 0),
            "a single reversed cell must move the origin"
        );
    }

    #[test]
    fn set_origin_places_the_origin_exactly_when_in_bounds() {
        let mut c = cam();
        c.set_origin(Pos::new(12, 34));
        assert_eq!(c.origin(), Pos::new(12, 34));
    }

    #[test]
    fn set_origin_clamps_to_the_world_edge() {
        let mut c = cam(); // viewport 10x10, world 100x100: max origin is (90, 90).
        c.set_origin(Pos::new(95, 200));
        assert_eq!(c.origin(), Pos::new(90, 90));
    }

    #[test]
    fn set_origin_never_underflows_when_the_viewport_exceeds_the_world() {
        let mut c = Camera::new(Rect::new(0, 0, 20, 20), Size::new(5, 5));
        c.set_origin(Pos::new(3, 3));
        assert_eq!(c.origin(), Pos::new(0, 0));
    }

    #[test]
    fn center_on_and_set_origin_agree_on_the_clamped_result() {
        // `center_on` now routes through `set_origin`; this pins that composition down.
        let mut a = cam();
        a.center_on(Pos::new(99, 99));

        let mut b = cam();
        b.set_origin(Pos::new(94, 94)); // 99 - viewport.width() / 2 = 94.

        assert_eq!(a.origin(), b.origin());
    }
}
