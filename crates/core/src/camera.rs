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
//! # Example
//!
//! ```
//! use retroglyph_core::{Camera, Pos, Rect, Size};
//!
//! // A 10x10 viewport onto a 100x100 world.
//! let mut cam = Camera::new(Rect::new(0, 0, 10, 10), Size { width: 100, height: 100 });
//! cam.center_on(Pos::new(50, 50));
//! assert_eq!(cam.origin(), Pos::new(45, 45));
//! assert_eq!(cam.world_to_screen(Pos::new(50, 50)), Some(Pos::new(5, 5)));
//! // Near an edge the view clamps rather than showing past the world.
//! cam.center_on(Pos::new(1, 1));
//! assert_eq!(cam.origin(), Pos::new(0, 0));
//! ```

use crate::grid::{Pos, Rect, Size};

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
    pub fn set_viewport(&mut self, viewport: Rect) {
        self.viewport = viewport;
        self.origin = Pos::new(
            self.origin
                .x
                .min(max_origin(viewport.width(), self.world.width)),
            self.origin
                .y
                .min(max_origin(viewport.height(), self.world.height)),
        );
    }

    /// Center the view on `target` (world coords), clamped to the world edges so
    /// the viewport never scrolls past `[0, world)`.
    pub fn center_on(&mut self, target: Pos) {
        self.origin = Pos::new(
            center_axis(target.x, self.viewport.width(), self.world.width),
            center_axis(target.y, self.viewport.height(), self.world.height),
        );
    }

    /// The world rectangle currently visible, clamped to world bounds.
    #[must_use]
    pub fn visible_bounds(&self) -> Rect {
        let w = self
            .viewport
            .width()
            .min(self.world.width.saturating_sub(self.origin.x));
        let h = self
            .viewport
            .height()
            .min(self.world.height.saturating_sub(self.origin.y));
        Rect::new(self.origin.x, self.origin.y, w.into(), h.into())
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

    /// Map a screen position back to a world position, or `None` if it is
    /// outside the viewport or beyond the world (useful for mouse picking).
    #[must_use]
    pub fn screen_to_world(&self, screen: Pos) -> Option<Pos> {
        if !self.viewport.contains_pos(screen) {
            return None;
        }
        let wx = self.origin.x + (screen.x - self.viewport.left());
        let wy = self.origin.y + (screen.y - self.viewport.top());
        if wx >= self.world.width || wy >= self.world.height {
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
        Camera::new(
            Rect::new(0, 0, 10, 10),
            Size {
                width: 100,
                height: 100,
            },
        )
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
        let mut c = Camera::new(
            Rect::new(2, 2, 20, 20),
            Size {
                width: 5,
                height: 5,
            },
        );
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
}
