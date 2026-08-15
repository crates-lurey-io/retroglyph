//! A scrolling viewport into a world larger than the screen.
//!
//! [`Camera`](crate::camera::Camera) is pure geometry: it converts between world coordinates (cells in
//! some large space) and screen coordinates (cells in a [`Rect`](retroglyph_core::grid::Rect) on the
//! terminal), and reports which world cells are currently visible. It holds no
//! rendering opinion, so it works with any drawing style and is testable
//! without a backend.
//!
//! ```text
//!  world space (Size)                      screen space (terminal cells)
//!  +--------------------------------+
//!  |                                |       viewport (a Rect on screen)
//!  |        origin                  |       +------------------+
//!  |          x----------------+    |       | (vp.left, vp.top)|
//!  |          | visible_bounds |    |  ==>  |   +----------+   |
//!  |          |   (clamped to  |    |       |   | drawn    |   |
//!  |          |    the world)  |    |       |   |  cells   |   |
//!  |          +----------------+    |       |   +----------+   |
//!  |                                |       +------------------+
//!  +--------------------------------+
//!
//!  origin = world cell shown at the viewport's top-left, clamped to [0, world - viewport].
//!  world_to_screen(w) = viewport.top_left + (w - origin), culled to visible_bounds.
//!  screen_to_world(s) = origin + (s - viewport.top_left), culled to the world.
//! ```
//!
//! Centering clamps to the world edges (the "scrolling map" convention): the
//! viewport never scrolls past `[0, world)`, so the target stays centered
//! except near the edges, where it drifts toward the corner. A world smaller
//! than the viewport pins the origin at `(0, 0)`, with all the slack on the
//! right and bottom of the given viewport rect; use
//! [`set_viewport_fitted`](crate::camera::Camera::set_viewport_fitted) instead of
//! [`set_viewport`](crate::camera::Camera::set_viewport) when a world that may be smaller
//! than its viewport (a fixed board, a generated map, a minimap) should be
//! letterboxed and centered instead.
//!
//! [`TileCamera`](crate::camera::TileCamera) is the tile-map sibling of `Camera`: it paces
//! panning and clamping in tiles rather than cells, and projects one tile to a `zoom.width() x
//! zoom.height()` block of screen cells instead of one-to-one, the shape a tile map with a
//! user-controlled zoom level needs (every tile is several cells across) rather than the
//! roguelike shape the rest of this module doc describes.
//!
//! See the `12_dungeon_scroll` example for `Camera` in action:
//! <https://main.retroglyph.dev/examples/12_dungeon_scroll/terminal/>.
//!
//! [`Grid::from_charmap`](retroglyph_core::grid::Grid::from_charmap) builds a styled grid from an ASCII map or
//! level string, one tile per character; combined with a [`Camera`](crate::camera::Camera) and multi-layer compositing,
//! this is how a scrolling roguelike loads and follows a map larger than the screen (see the
//! `11_sokoban` example for `from_charmap` itself, and `15_outpost_dashboard` for a `Camera` used
//! alongside a UI).
//!
//! # Example
//!
//! ```
//! use retroglyph_core::grid::{Pos, Rect, Size};
//! use retroglyph_ui::camera::Camera;
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

use retroglyph_core::grid::{HasSize, Pos, Rect, Size};

mod tile;
mod transform;

/// A rectangular viewport onto a larger world, with world/screen conversions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Camera {
    requested: Rect,
    fit: ViewportFit,
    viewport: Rect,
    world: Size,
    origin: Pos,
}

/// Which shape [`Camera::viewport`](Camera::viewport) is derived from `requested` with: set by
/// [`set_viewport`](Camera::set_viewport) and [`set_viewport_fitted`](Camera::set_viewport_fitted)
/// respectively, and re-applied by [`set_world`](Camera::set_world) so a world change re-derives
/// `viewport` instead of leaving it stale against the previous world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewportFit {
    /// `viewport` is `requested` unchanged.
    Exact,
    /// `viewport` is `requested` shrunk to the world's size on any axis where the world is
    /// smaller, and centered within `requested`.
    Letterbox,
}

/// Shrinks `rect` to `world`'s size on any axis where `world` is smaller, and centers the shrunk
/// rect within `rect`. Identity when `world` is at least as large as `rect` on both axes.
fn letterbox(rect: Rect, world: Size) -> Rect {
    let width = rect.width().min(world.width());
    let height = rect.height().min(world.height());
    let x = rect.left().saturating_add((rect.width() - width) / 2);
    let y = rect.top().saturating_add((rect.height() - height) / 2);
    Rect::new(x, y, width, height)
}

impl Camera {
    /// Create a camera drawing into `viewport` (screen cells) over a world of
    /// `world` cells. The initial origin is `(0, 0)`; call
    /// [`center_on`](Self::center_on) to follow a target.
    #[must_use]
    pub const fn new(viewport: Rect, world: Size) -> Self {
        Self {
            requested: viewport,
            fit: ViewportFit::Exact,
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
        self.requested = viewport;
        self.fit = ViewportFit::Exact;
        self.apply_viewport();
    }

    /// Replace the world dimensions (for example when a level changes), keeping the requested
    /// viewport rect unchanged and re-deriving [`viewport`](Self::viewport) and `origin` against
    /// the new world.
    ///
    /// If the camera was last positioned with
    /// [`set_viewport_fitted`](Self::set_viewport_fitted), this re-runs that letterboxing against
    /// the new world too, so [`viewport`](Self::viewport) stays correct without calling
    /// `set_viewport_fitted` again.
    ///
    /// Never panics: the re-clamp uses the same saturating arithmetic as
    /// [`set_viewport`](Self::set_viewport).
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::grid::{Pos, Rect, Size};
    /// use retroglyph_ui::camera::Camera;
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
        self.apply_viewport();
    }

    /// Re-derives `viewport` from `requested` and `fit`, then re-clamps `origin` against it.
    /// The single funnel [`set_viewport`](Self::set_viewport),
    /// [`set_viewport_fitted`](Self::set_viewport_fitted), and [`set_world`](Self::set_world) all
    /// go through, so `viewport` is always derived fresh rather than a stale snapshot from
    /// whichever of those ran last.
    fn apply_viewport(&mut self) {
        self.viewport = match self.fit {
            ViewportFit::Exact => self.requested,
            ViewportFit::Letterbox => letterbox(self.requested, self.world),
        };
        self.set_origin(self.origin);
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
    /// use retroglyph_core::grid::{Pos, Rect, Size};
    /// use retroglyph_ui::camera::Camera;
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
        self.requested = viewport;
        self.fit = ViewportFit::Letterbox;
        self.apply_viewport();
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
    /// Never panics: the clamp is [`Rect::clamp_within`], which uses
    /// [`saturating_sub`](u16::saturating_sub) internally, so it cannot underflow even for a
    /// `viewport` larger than `world`.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::grid::{Pos, Rect, Size};
    /// use retroglyph_ui::camera::Camera;
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
        self.origin = Rect::from_tl_size(origin, self.viewport.size())
            .clamp_within(self.world.to_rect())
            .top_left();
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
    /// use retroglyph_core::grid::{Pos, Rect, Size};
    /// use retroglyph_ui::camera::Camera;
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
        self.set_origin(self.origin.saturating_add_signed(ixy::Pos::new(dx, dy)));
    }
}

/// The tile-map sibling of [`Camera`](crate::camera::Camera).
///
/// Panning and clamping are paced in tiles rather than screen cells, and
/// [`tile_rect`](Self::tile_rect)/[`tile_surface`](Self::tile_surface) project one tile to a
/// `zoom.width() x zoom.height()` block of screen cells instead of `Camera`'s
/// one-cell-to-one-cell picture.
///
/// Internally this owns a `Camera` counted in tiles instead of screen cells, so panning,
/// clamping, and centering are exactly `Camera`'s own logic, applied to a tile grid; zoom only
/// enters when projecting a tile to its screen-cell footprint.
///
/// # Examples
///
/// ```
/// use retroglyph_core::grid::{Pos, Rect, Size};
/// use retroglyph_ui::camera::TileCamera;
///
/// // A 12x8 screen viewport onto a 20x20 tile world, at 3x2 screen cells per tile.
/// let mut cam = TileCamera::new(Rect::new(0, 0, 12, 8), Size::new(20, 20), Size::new(3, 2));
/// cam.center_on(Pos::new(10, 10));
/// assert_eq!(cam.tile_rect(Pos::new(10, 10)).width(), 3);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileCamera {
    tiles: Camera,
    viewport: Rect,
    zoom: Size,
}

impl TileCamera {
    /// Create a tile camera drawing into `viewport` (screen cells) over a `world` of tiles, at
    /// `zoom` screen cells per tile. The initial origin is `(0, 0)`; call
    /// [`center_on`](Self::center_on) to follow a target.
    ///
    /// A zero `zoom` axis clamps to `1`, the same as [`set_zoom`](Self::set_zoom).
    #[must_use]
    pub fn new(viewport: Rect, world: Size, zoom: Size) -> Self {
        let zoom = clamp_zoom(zoom);
        Self {
            tiles: Camera::new(tile_viewport(viewport, zoom), world),
            viewport,
            zoom,
        }
    }

    /// The screen rectangle the world is drawn into.
    #[must_use]
    pub const fn viewport(&self) -> Rect {
        self.viewport
    }

    /// The world dimensions, in tiles.
    #[must_use]
    pub const fn world(&self) -> Size {
        self.tiles.world()
    }

    /// The tile shown at the viewport's top-left corner.
    #[must_use]
    pub const fn origin(&self) -> Pos {
        self.tiles.origin()
    }

    /// How many screen cells one tile covers, per axis. `(1, 1)` by default: one tile to one
    /// screen cell, same as an un-zoomed [`Camera`](crate::camera::Camera).
    #[must_use]
    pub const fn zoom(&self) -> Size {
        self.zoom
    }

    /// Set how many screen cells one tile covers, per axis, re-deriving the tile-unit viewport
    /// from the current screen [`viewport`](Self::viewport) and re-clamping the origin so it
    /// stays in bounds for the new tile-count viewport.
    ///
    /// A zero width or height clamps to `1`: a zero-size tile has no meaningful footprint to
    /// draw into, so this refuses to create one rather than silently making every tile
    /// disappear.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::grid::{Rect, Size};
    /// use retroglyph_ui::camera::TileCamera;
    ///
    /// let mut cam = TileCamera::new(Rect::new(0, 0, 10, 10), Size::new(20, 20), Size::new(1, 1));
    /// cam.set_zoom(Size::new(3, 2));
    /// assert_eq!(cam.zoom(), Size::new(3, 2));
    ///
    /// // A zero axis clamps to 1 rather than producing an empty tile footprint.
    /// cam.set_zoom(Size::new(0, 4));
    /// assert_eq!(cam.zoom(), Size::new(1, 4));
    /// ```
    pub fn set_zoom(&mut self, zoom: Size) {
        self.zoom = clamp_zoom(zoom);
        self.tiles
            .set_viewport(tile_viewport(self.viewport, self.zoom));
    }

    /// Replace the viewport (for example after a terminal resize), keeping the world and zoom
    /// unchanged and re-clamping the origin so it stays in bounds.
    pub fn set_viewport(&mut self, viewport: Rect) {
        self.viewport = viewport;
        self.tiles.set_viewport(tile_viewport(viewport, self.zoom));
    }

    /// Replace the world dimensions, in tiles, keeping the viewport and zoom unchanged and
    /// re-clamping the origin so it stays in bounds.
    pub fn set_world(&mut self, world: Size) {
        self.tiles.set_world(world);
    }

    /// Center the view on `target` (a tile position), clamped to the world edges so the
    /// viewport never scrolls past `[0, world)` tiles. The tile equivalent of
    /// [`Camera::center_on`](crate::camera::Camera::center_on).
    pub fn center_on(&mut self, target: Pos) {
        self.tiles.center_on(target);
    }

    /// Set the top-left tile directly, clamped to the world edges like
    /// [`center_on`](Self::center_on). The tile equivalent of
    /// [`Camera::set_origin`](crate::camera::Camera::set_origin).
    pub fn set_origin(&mut self, origin: Pos) {
        self.tiles.set_origin(origin);
    }

    /// Scroll the view by a signed tile delta, clamped to the world edges like
    /// [`set_origin`](Self::set_origin). The tile equivalent of
    /// [`Camera::scroll_by`](crate::camera::Camera::scroll_by).
    pub fn scroll_by(&mut self, dx: i32, dy: i32) {
        self.tiles.scroll_by(dx, dy);
    }

    /// The tile rectangle currently visible, in tile coordinates, clamped to world bounds. The
    /// tile equivalent of [`Camera::visible_bounds`](crate::camera::Camera::visible_bounds).
    #[must_use]
    pub fn visible_bounds(&self) -> Rect {
        self.tiles.visible_bounds()
    }
}

/// A zero width or height clamps to `1`: a zero-size tile has no meaningful footprint.
const fn clamp_zoom(zoom: Size) -> Size {
    let width = if zoom.width == 0 { 1 } else { zoom.width };
    let height = if zoom.height == 0 { 1 } else { zoom.height };
    Size::new(width, height)
}

/// The tile-unit viewport an inner [`Camera`] paces its panning and clamping against: the
/// screen `viewport`'s size in tiles, at a fixed `(0, 0)` position, since only the size (not
/// the position) of a `Camera`'s viewport affects its clamp/origin math.
fn tile_viewport(viewport: Rect, zoom: Size) -> Rect {
    Rect::from_tl_size(Pos::new(0, 0), viewport.size() / zoom)
}

#[cfg(test)]
pub(super) const fn cam() -> Camera {
    Camera::new(Rect::new(0, 0, 10, 10), Size::new(100, 100))
}

#[cfg(test)]
pub(super) fn tile_cam() -> TileCamera {
    TileCamera::new(
        Rect::new(0, 0, 10, 10),
        Size::new(100, 100),
        Size::new(1, 1),
    )
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
        // clamp_within(world 20x20) clamps origin down from 45 to 20 - viewport(10) = 10.
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
    fn set_world_re_derives_a_fitted_viewport_instead_of_leaving_it_stale() {
        let mut c = Camera::new(Rect::new(2, 2, 20, 20), Size::new(5, 5));
        c.set_viewport_fitted(Rect::new(2, 2, 20, 20));
        assert_eq!(c.viewport(), Rect::new(9, 9, 5, 5));

        // A larger world no longer needs letterboxing: `viewport` re-expands to the originally
        // requested rect instead of staying pinned to the previous world's letterboxed rect.
        c.set_world(Size::new(100, 100));
        assert_eq!(c.viewport(), Rect::new(2, 2, 20, 20));
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
