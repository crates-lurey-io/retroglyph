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

mod transform;

use crate::grid::{Pos, Rect, Size};
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
        let x = viewport
            .left()
            .saturating_add((viewport.width() - width) / 2);
        let y = viewport
            .top()
            .saturating_add((viewport.height() - height) / 2);
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
pub(super) const fn cam() -> Camera {
    Camera::new(Rect::new(0, 0, 10, 10), Size::new(100, 100))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn set_viewport_fitted_matches_set_viewport_when_the_world_is_not_smaller() {
        let mut a = Camera::new(Rect::new(0, 0, 1, 1), Size::new(100, 100));
        a.set_viewport_fitted(Rect::new(0, 0, 10, 10));

        let mut b = Camera::new(Rect::new(0, 0, 1, 1), Size::new(100, 100));
        b.set_viewport(Rect::new(0, 0, 10, 10));

        assert_eq!(a.viewport(), b.viewport());
        assert_eq!(a.origin(), b.origin());
    }

    #[test]
    fn set_viewport_fitted_saturates_instead_of_overflowing() {
        let mut c = Camera::new(Rect::new(0, 0, 1, 1), Size::new(5, 5));
        c.set_viewport_fitted(Rect::new(65_530, 0, 1_000, 10));
        assert_eq!(c.viewport(), Rect::new(u16::MAX, 2, 5, 5));
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
