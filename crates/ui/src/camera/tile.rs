//! Tile-to-screen projection for [`TileCamera`](super::TileCamera).

use super::TileCamera;
use retroglyph_core::grid::{Pos, Rect};
use retroglyph_core::surface::Surface;

impl TileCamera {
    /// Iterate the visible tiles as `(tile, screen)` position pairs, in row-major order, where
    /// `screen` is the tile's top-left screen cell. Only tiles that exist in the world are
    /// yielded, so the caller can fill the rest of the viewport with a background. The tile
    /// equivalent of [`Camera::cells`](crate::camera::Camera::cells): one entry per tile, not
    /// per screen cell, so `cells().count()` is a tile count even at a zoom where each tile
    /// covers several screen cells.
    #[must_use = "iterators are lazy and do nothing unless consumed"]
    pub fn cells(&self) -> impl Iterator<Item = (Pos, Pos)> {
        let viewport = self.viewport;
        let origin = self.tiles.origin();
        let zoom = self.zoom;
        self.tiles.cells().map(move |(tile, _)| {
            let dx = (tile.x - origin.x) * zoom.width;
            let dy = (tile.y - origin.y) * zoom.height;
            let screen = Pos::new(
                viewport.left().saturating_add(dx),
                viewport.top().saturating_add(dy),
            );
            (tile, screen)
        })
    }

    /// Map a screen position back to a tile position, or `None` if it is outside the viewport
    /// or beyond the world (useful for mouse picking at a non-default zoom). The tile
    /// equivalent of [`Camera::screen_to_world`](crate::camera::Camera::screen_to_world): where
    /// that divides by `1`, this divides by [`zoom`](Self::zoom), so it agrees with
    /// [`tile_rect`](Self::tile_rect) about which tile a screen cell belongs to.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::grid::{Pos, Rect, Size};
    /// use retroglyph_ui::camera::TileCamera;
    ///
    /// let cam = TileCamera::new(Rect::new(0, 0, 12, 8), Size::new(20, 20), Size::new(3, 2));
    /// // Any screen cell inside tile (1, 0)'s 3x2 footprint maps back to (1, 0).
    /// assert_eq!(cam.screen_to_tile(Pos::new(4, 1)), Some(Pos::new(1, 0)));
    /// ```
    #[must_use]
    pub fn screen_to_tile(&self, screen: Pos) -> Option<Pos> {
        if !self.viewport.contains_pos(screen) {
            return None;
        }
        let origin = self.tiles.origin();
        let tx = origin.x + (screen.x - self.viewport.left()) / self.zoom.width;
        let ty = origin.y + (screen.y - self.viewport.top()) / self.zoom.height;
        let world = self.tiles.world();
        if tx >= world.width || ty >= world.height {
            return None;
        }
        Some(Pos::new(tx, ty))
    }

    /// The absolute screen rect a tile's footprint covers, and (in `i32`, so it can go negative
    /// or past `u16::MAX`) that footprint's ideal, unclipped top-left -- the ongoing reference
    /// point every screen cell inside the tile is measured from, even once the visible rect
    /// below has been clipped down to a smaller slice of it.
    fn tile_geometry(&self, tile: Pos) -> (Rect, (i32, i32)) {
        let zoom_w = i64::from(self.zoom.width);
        let zoom_h = i64::from(self.zoom.height);
        let origin = self.tiles.origin();
        let ox =
            i64::from(self.viewport.left()) + (i64::from(tile.x) - i64::from(origin.x)) * zoom_w;
        let oy =
            i64::from(self.viewport.top()) + (i64::from(tile.y) - i64::from(origin.y)) * zoom_h;

        let ideal = ixy::Rect::<i64>::new(ox, oy, zoom_w, zoom_h);
        let bounds = ixy::Rect::<i64>::new(
            i64::from(self.viewport.left()),
            i64::from(self.viewport.top()),
            i64::from(self.viewport.width()),
            i64::from(self.viewport.height()),
        );
        let clipped = ideal.intersect(bounds);

        let left = u16::try_from(clipped.left()).unwrap_or(u16::MAX);
        let top = u16::try_from(clipped.top()).unwrap_or(u16::MAX);
        let width = u16::try_from(clipped.width()).unwrap_or(0);
        let height = u16::try_from(clipped.height()).unwrap_or(0);

        let ox = i32::try_from(ox).unwrap_or(if ox < 0 { i32::MIN } else { i32::MAX });
        let oy = i32::try_from(oy).unwrap_or(if oy < 0 { i32::MIN } else { i32::MAX });

        (Rect::new(left, top, width, height), (ox, oy))
    }

    /// The absolute screen rect a tile's footprint covers at this camera's
    /// [`zoom`](Self::zoom), clipped to the viewport. An empty `Rect` (zero width or height)
    /// means no part of the tile is visible.
    ///
    /// `tile_rect` only clips to the viewport, not to [`world`](Self::world): a tile's existence
    /// in the world is the caller's own concern.
    ///
    /// A tile straddling the viewport edge reports only the visible slice of its full,
    /// ideal-sized footprint, not the full footprint or an empty `Rect`, matching how
    /// [`Camera::surface`](crate::camera::Camera::surface) clips a multi-cell draw that's only
    /// partially on screen instead of dropping it entirely.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::grid::{Pos, Rect, Size};
    /// use retroglyph_ui::camera::TileCamera;
    ///
    /// let mut cam = TileCamera::new(Rect::new(0, 0, 10, 10), Size::new(20, 20), Size::new(3, 2));
    ///
    /// // tile (0, 0) is drawn at origin (0, 0) here, so its footprint covers a 3x2 block.
    /// assert_eq!(cam.tile_rect(Pos::new(0, 0)), Rect::new(0, 0, 3, 2));
    ///
    /// // A tile straddling the viewport's right edge is clipped to what's actually visible.
    /// let cam = TileCamera::new(Rect::new(0, 0, 4, 4), Size::new(20, 20), Size::new(3, 3));
    /// assert_eq!(cam.tile_rect(Pos::new(1, 0)), Rect::new(3, 0, 1, 3));
    /// ```
    #[must_use]
    pub fn tile_rect(&self, tile: Pos) -> Rect {
        self.tile_geometry(tile).0
    }

    /// A view of `surface` scoped to one tile's footprint at this camera's
    /// [`zoom`](Self::zoom), or `None` if none of that footprint is visible.
    ///
    /// The returned surface's own coordinate space is tile-local: `(0, 0)` is the tile's own
    /// top-left corner, and `put`/`print`/the rest of `Surface`'s drawing methods take positions
    /// in `0..zoom.width()` by `0..zoom.height()`, the same way a small standalone `Surface`
    /// would for a `zoom.width() x zoom.height()` sprite. A tile straddling the viewport edge is
    /// still clipped automatically: only the on-screen slice of it accepts writes.
    ///
    /// Calling this once per visible tile (for example every `(tile, _)` pair from
    /// [`cells`](Self::cells)) replaces hand-rolled tile-range-plus-offset math with one
    /// drawable region per tile.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::color::Style;
    /// use retroglyph_core::grid::{Grid, Pos, Rect, Size};
    /// use retroglyph_core::surface::Surface;
    /// use retroglyph_ui::camera::TileCamera;
    ///
    /// let mut grid = Grid::new(10, 10);
    /// let mut root = Surface::new(&mut grid, Rect::new(0, 0, 10, 10), 0);
    ///
    /// let cam = TileCamera::new(Rect::new(0, 0, 10, 10), Size::new(20, 20), Size::new(3, 2));
    ///
    /// // tile (1, 0)'s footprint covers screen cells (3, 0)..(6, 2); draw distinct glyphs at
    /// // two corners of it in tile-local coordinates.
    /// let mut tile = cam.tile_surface(Pos::new(1, 0), &mut root).expect("tile is visible");
    /// tile.put(Pos::new(0, 0), '[', Style::default());
    /// tile.put(Pos::new(2, 1), ']', Style::default());
    ///
    /// assert_eq!(grid[Pos::new(3, 0)].glyph(), '[');
    /// assert_eq!(grid[Pos::new(5, 1)].glyph(), ']');
    /// ```
    ///
    /// A tile straddling the viewport edge only accepts writes to its visible slice; the rest is
    /// silently dropped, like any other out-of-clip `Surface` write.
    ///
    /// ```
    /// use retroglyph_core::color::Style;
    /// use retroglyph_core::grid::{Grid, Pos, Rect, Size};
    /// use retroglyph_core::surface::Surface;
    /// use retroglyph_ui::camera::TileCamera;
    ///
    /// let mut grid = Grid::new(10, 10);
    /// let cam = TileCamera::new(Rect::new(0, 0, 4, 4), Size::new(20, 20), Size::new(3, 3));
    ///
    /// // This tile's ideal footprint is (3, 0, 3, 3); only its leftmost column (3, 0)..(4, 3)
    /// // is inside the 4-wide viewport.
    /// {
    ///     let mut root = Surface::new(&mut grid, Rect::new(0, 0, 10, 10), 0);
    ///     let mut tile = cam.tile_surface(Pos::new(1, 0), &mut root).expect("tile is visible");
    ///     tile.put(Pos::new(0, 0), 'L', Style::default()); // the tile's own visible column.
    ///     tile.put(Pos::new(2, 0), 'R', Style::default()); // clipped off past the viewport edge.
    /// }
    ///
    /// assert_eq!(grid[Pos::new(3, 0)].glyph(), 'L');
    /// assert_eq!(grid[Pos::new(5, 0)].glyph(), ' ');
    ///
    /// // A tile entirely outside the viewport has nothing visible to draw into.
    /// let mut root = Surface::new(&mut grid, Rect::new(0, 0, 10, 10), 0);
    /// assert!(cam.tile_surface(Pos::new(10, 10), &mut root).is_none());
    /// ```
    #[must_use]
    pub fn tile_surface<'a>(&self, tile: Pos, surface: &'a mut Surface<'_>) -> Option<Surface<'a>> {
        let (area, (ox, oy)) = self.tile_geometry(tile);
        if area.is_empty() {
            return None;
        }
        let origin_x = i32::from(area.left()).saturating_sub(ox);
        let origin_y = i32::from(area.top()).saturating_sub(oy);
        Some(surface.clip_translate(area, (origin_x, origin_y)))
    }
}

#[cfg(test)]
mod tests {
    use super::super::tile_cam;
    use super::*;
    use retroglyph_core::grid::Size;

    #[test]
    fn tile_rect_matches_a_single_screen_cell_at_the_default_1x1_zoom() {
        let mut c = tile_cam();
        c.center_on(Pos::new(50, 50));
        assert_eq!(c.tile_rect(Pos::new(50, 50)), Rect::new(5, 5, 1, 1));
        assert_eq!(c.tile_rect(Pos::new(44, 50)), Rect::new(0, 0, 0, 0));
    }

    #[test]
    fn tile_rect_covers_a_zoom_sized_block() {
        let c = TileCamera::new(Rect::new(0, 0, 10, 10), Size::new(20, 20), Size::new(3, 2));
        assert_eq!(c.tile_rect(Pos::new(0, 0)), Rect::new(0, 0, 3, 2));
        assert_eq!(c.tile_rect(Pos::new(1, 0)), Rect::new(3, 0, 3, 2));
    }

    #[test]
    fn tile_rect_clips_a_tile_straddling_the_viewport_edge() {
        let c = TileCamera::new(Rect::new(0, 0, 4, 4), Size::new(20, 20), Size::new(3, 3));
        // Ideal footprint (3, 0, 3, 3); only one column is inside the 4-wide viewport.
        assert_eq!(c.tile_rect(Pos::new(1, 0)), Rect::new(3, 0, 1, 3));
    }

    #[test]
    fn tile_rect_is_empty_for_a_tile_entirely_off_viewport() {
        let c = TileCamera::new(Rect::new(0, 0, 4, 4), Size::new(20, 20), Size::new(1, 1));
        assert_eq!(c.tile_rect(Pos::new(10, 10)), Rect::new(0, 0, 0, 0));
    }

    #[test]
    fn set_zoom_clamps_a_zero_axis_to_one() {
        let mut c = tile_cam();
        c.set_zoom(Size::new(0, 0));
        assert_eq!(c.zoom(), Size::new(1, 1));
    }

    #[test]
    fn tile_surface_draws_within_a_single_tile_footprint() {
        use retroglyph_core::color::Style;
        use retroglyph_core::grid::Grid;

        let mut grid = Grid::new(10, 10);
        let mut root = Surface::new(&mut grid, Rect::new(0, 0, 10, 10), 0);

        let c = TileCamera::new(Rect::new(0, 0, 10, 10), Size::new(20, 20), Size::new(3, 2));

        let mut tile = c.tile_surface(Pos::new(1, 0), &mut root).expect("visible");
        tile.put(Pos::new(0, 0), '[', Style::default());
        tile.put(Pos::new(2, 1), ']', Style::default());
        // Outside the tile's own 3x2 footprint: dropped, not drawn into the next tile over.
        tile.put(Pos::new(3, 0), 'X', Style::default());

        assert_eq!(grid[Pos::new(3, 0)].glyph(), '[');
        assert_eq!(grid[Pos::new(5, 1)].glyph(), ']');
        assert_eq!(grid[Pos::new(6, 0)].glyph(), ' ');
    }

    #[test]
    fn tile_surface_clips_a_tile_straddling_the_viewport_edge() {
        use retroglyph_core::color::Style;
        use retroglyph_core::grid::Grid;

        let mut grid = Grid::new(10, 10);
        let c = TileCamera::new(Rect::new(0, 0, 4, 4), Size::new(20, 20), Size::new(3, 3));

        {
            let mut root = Surface::new(&mut grid, Rect::new(0, 0, 10, 10), 0);
            let mut tile = c.tile_surface(Pos::new(1, 0), &mut root).expect("visible");
            // Tile-local (0, 0) is the tile's own leftmost column, the only part on screen.
            tile.put(Pos::new(0, 0), 'L', Style::default());
            // Tile-local (2, 0) falls past the viewport's right edge: dropped.
            tile.put(Pos::new(2, 0), 'R', Style::default());
        }

        assert_eq!(grid[Pos::new(3, 0)].glyph(), 'L');
        assert_eq!(grid[Pos::new(5, 0)].glyph(), ' ');
    }

    #[test]
    fn tile_surface_is_none_for_a_tile_entirely_off_viewport() {
        use retroglyph_core::grid::Grid;

        let mut grid = Grid::new(10, 10);
        let mut root = Surface::new(&mut grid, Rect::new(0, 0, 10, 10), 0);
        let c = TileCamera::new(Rect::new(0, 0, 4, 4), Size::new(20, 20), Size::new(1, 1));

        assert!(c.tile_surface(Pos::new(10, 10), &mut root).is_none());
    }

    // The regression tests the issue asks for: `Camera` and `TileCamera` now hold entirely
    // separate projections, so pan/cull/hit-test all agree with `tile_rect` at zoom != 1,
    // instead of `Camera`'s old single struct silently disagreeing with itself.

    #[test]
    fn screen_to_tile_agrees_with_tile_rect_at_a_non_default_zoom() {
        let mut c = TileCamera::new(Rect::new(0, 0, 12, 8), Size::new(20, 20), Size::new(3, 2));
        c.center_on(Pos::new(10, 10));

        let tile = Pos::new(10, 10);
        let screen = c.tile_rect(tile).top_left();
        // The two projections `TileCamera` offers (draw via `tile_rect`, hit-test via
        // `screen_to_tile`) must round-trip: this is exactly what the old `Camera` with an
        // internal `zoom` field could not guarantee, since `screen_to_world` ignored zoom while
        // `tile_rect` did not.
        assert_eq!(c.screen_to_tile(screen), Some(tile));
    }

    #[test]
    fn scroll_by_reaches_the_last_tile_at_a_non_default_zoom() {
        // A 10x10 screen viewport at 4x4 zoom is 2x2 tiles; scrolled to the far edge of a 15x15
        // tile world, the max origin is `world - viewport_tiles` = 15 - 2 = 13, the same clamp
        // `Camera::scroll_by` applies, just paced in tiles instead of screen cells.
        let mut c = TileCamera::new(Rect::new(0, 0, 10, 10), Size::new(15, 15), Size::new(4, 4));
        c.scroll_by(1000, 1000);
        assert_eq!(c.origin(), Pos::new(13, 13));
        // The world's last tile (index 14) is reachable and its footprint is still fully on
        // screen: one tile past the origin, at pixel offset 4.
        let last = c.tile_rect(Pos::new(14, 14));
        assert_eq!(last, Rect::new(4, 4, 4, 4));
    }

    #[test]
    fn center_on_puts_the_target_tiles_footprint_inside_the_viewport_at_a_non_default_zoom() {
        let mut c = TileCamera::new(Rect::new(0, 0, 12, 8), Size::new(20, 20), Size::new(3, 2));
        c.center_on(Pos::new(10, 10));
        let footprint = c.tile_rect(Pos::new(10, 10));
        assert!(!footprint.is_empty());
        assert!(c.viewport().contains_rect(footprint));
    }

    #[test]
    fn cells_count_matches_the_tile_count_not_the_screen_cell_count() {
        // A 12x8 screen viewport at 3x2 zoom is 4x4 tiles; a 20x20 tile world leaves it fully
        // interior, so every one of the 16 tiles is visible.
        let c = TileCamera::new(Rect::new(0, 0, 12, 8), Size::new(20, 20), Size::new(3, 2));
        assert_eq!(c.cells().count(), 16);
    }
}
