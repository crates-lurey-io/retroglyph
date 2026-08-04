use crate::color::Style;
use crate::color::Tint;
use crate::grid::{Grid, Rect};

use super::styled::StyledSurface;
use super::{Layer, Surface};

impl<'a> Surface<'a> {
    /// The region this surface represents, e.g. for a widget to lay itself out in.
    ///
    /// Unlike [`clip_rect`](Self::clip_rect), this is never narrowed by [`clip`](Self::clip): it
    /// only changes when [`scope`](Self::scope) sets a new one. A widget that reads its own area
    /// off the surface after being clipped (e.g. while partially offscreen) sees the region it
    /// was given, not the visible sliver of it, so it can still center itself correctly and let
    /// the clip take care of what actually lands.
    #[must_use]
    pub const fn area(&self) -> Rect {
        self.area
    }

    /// [`area`](Self::area), translated to this surface's own coordinate space: always
    /// `Rect::new(0, 0, width, height)`.
    ///
    /// Every drawing method on this surface ([`put`](Self::put), [`print`](Self::print),
    /// [`fill_rect`](Self::fill_rect), ...) takes coordinates local to this surface, where
    /// `(0, 0)` is this area's own top-left corner, not the underlying grid's. [`area`](Self::area)
    /// itself is absolute grid space, so `surface.put((surface.area().left(), ...), ...)` only
    /// lands correctly for a surface whose area happens to start at the grid origin; anywhere
    /// else it silently misses. A widget that wants to place itself relative to its own bounds
    /// (e.g. a label in a corner) should reach for `local_area()`, or just [`width`](Self::width)/
    /// [`height`](Self::height) directly, and never for `area()`'s own [`left`](Rect::left)/
    /// [`top`](Rect::top).
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{Grid, Rect, Surface};
    ///
    /// let mut grid = Grid::new(10, 10);
    /// let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 10, 10), 0);
    /// let mut scoped = surface.scope(Rect::new(3, 3, 4, 4));
    ///
    /// assert_eq!(scoped.area(), Rect::new(3, 3, 4, 4));
    /// assert_eq!(scoped.local_area(), Rect::new(0, 0, 4, 4));
    /// ```
    #[must_use]
    pub const fn local_area(&self) -> Rect {
        Rect::new(0, 0, self.area.width(), self.area.height())
    }

    /// The visible subset of [`area`](Self::area). Every write this surface accepts is
    /// bounds-checked against this rect, not `area`.
    #[must_use]
    pub const fn clip_rect(&self) -> Rect {
        self.clip
    }

    /// The width of this surface's area, in columns.
    #[must_use]
    pub const fn width(&self) -> u16 {
        self.area.width()
    }

    /// The height of this surface's area, in rows.
    #[must_use]
    pub const fn height(&self) -> u16 {
        self.area.height()
    }

    /// The grid layer this surface writes to.
    #[must_use]
    pub const fn layer(&self) -> u8 {
        self.layer
    }

    /// A new surface over the same grid, area, and clip, but writing to `layer` instead.
    #[must_use]
    pub const fn on_layer(&mut self, layer: u8) -> Surface<'_> {
        Surface {
            grid: self.grid,
            area: self.area,
            clip: self.clip,
            layer,
            tint: self.tint,
            origin_offset: self.origin_offset,
        }
    }

    /// Equivalent to [`self.on_layer(tier.as_u8())`](Surface::on_layer), for switching to one of
    /// the workspace's named [`Layer`] tiers instead of a raw layer id. See [`Layer`]'s docs for
    /// when to reach for this over a numeric [`Surface::on_layer`] call.
    #[must_use]
    pub const fn on_tier(&mut self, tier: Layer) -> Surface<'_> {
        self.on_layer(tier.as_u8())
    }

    /// The tint every sprite drawn through this surface is recoloured by.
    #[must_use]
    pub const fn tint(&self) -> Tint {
        self.tint
    }

    /// The offset [`translate`](Self::translate) has accumulated on this surface, `(0, 0)` if it
    /// has never been called.
    ///
    /// Every coordinate a caller passes to a coordinate-taking method has this subtracted from it
    /// before the usual bounds check (see [`translate`](Self::translate)'s doc), so a callee
    /// handed a `&mut Surface` can use this to tell whether it is in a translated coordinate
    /// space, compose a further offset relative to the current one without over- or
    /// undershooting, or convert a local coordinate it read back off the surface into the
    /// caller's own coordinate space by adding this back in.
    #[must_use]
    pub const fn origin(&self) -> (i32, i32) {
        self.origin_offset
    }

    /// A new surface over the same grid, area, and layer, recolouring every sprite it draws by
    /// `tint`.
    ///
    /// Substituted rather than combined: unlike [`clip`](Self::clip), which can only narrow,
    /// a tint replaces whatever the parent surface carried. Two tints do not compose into a
    /// third meaningful one, and silently multiplying an inherited shadow into a caller's damage
    /// flash would be harder to predict than replacing it.
    ///
    /// Applies to sprites only. A cell backend has no sprite to recolour and draws the cell's
    /// glyph in its own [`Style`], tinted or not, so this is invisible there. See [`Tint`].
    ///
    /// This tint composes with the sheet's own colour treatment; see
    /// `retroglyph_window::tileset::SheetColor` and `retroglyph_window::sprite_cache::SpriteTint`
    /// for the two-stage resolution (retroglyph-core has no dependency on retroglyph-window, so
    /// these are plain names, not intra-doc links).
    ///
    /// For a multi-cell span the tint lands on the anchor cell, which is where a pixel backend
    /// draws the sprite from. [`blit`](Self::blit) has no such anchor (`grid` is arbitrary
    /// composed content, not one sprite) and does not apply this tint at all; see its own doc.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() {
    /// # fn run() -> Option<()> {
    /// use retroglyph_core::{Grid, Rect, Style, Surface, Tint};
    ///
    /// let mut grid = Grid::new(8, 4);
    /// let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 8, 4), 0);
    ///
    /// // One grass sprite, drawn twice: once as itself, once dimmed into shadow.
    /// let grass = '\u{E000}';
    /// surface.put_span_uniform((0, 0), (2, 1), grass, ' ', Style::default())?;
    /// surface
    ///     .with_tint(Tint::multiply(128, 128, 128))
    ///     .put_span_uniform((2, 0), (2, 1), grass, ' ', Style::default())?;
    ///
    /// assert_eq!(grid.tint(0, 0, 0), Tint::None);
    /// assert_eq!(grid.tint(0, 2, 0), Tint::multiply(128, 128, 128));
    /// # Some(())
    /// # }
    /// # run().unwrap();
    /// # }
    /// ```
    #[must_use]
    pub const fn with_tint(&mut self, tint: Tint) -> Surface<'_> {
        Surface {
            grid: self.grid,
            area: self.area,
            clip: self.clip,
            layer: self.layer,
            tint,
            origin_offset: self.origin_offset,
        }
    }

    /// A new surface over the same grid, layer, and [`area`](Self::area), whose
    /// [`clip_rect`](Self::clip_rect) is narrowed to `rect` intersected with this surface's own
    /// clip. What this surface *represents* is unchanged; only what is visible shrinks.
    ///
    /// `rect` is in absolute grid coordinates (it intersects [`clip_rect`](Self::clip_rect),
    /// itself absolute), not local to this surface's own [`area`](Self::area) the way
    /// [`fill_rect`](Self::fill_rect), [`clear_region`](Self::clear_region), and
    /// [`print_aligned`](Self::print_aligned)'s own `rect` are. Coordinates are otherwise
    /// unchanged: the sub-surface addresses the same space this one does, so a
    /// sub-rect computed against [`Surface::area`] (e.g. by a [`layout`](crate::layout) split)
    /// can be passed straight in. Because the clip is intersected rather than substituted,
    /// narrowing is monotonic: handing a surface down a layout tree can only ever tighten what a
    /// callee is able to draw into, never widen it.
    ///
    /// Clipping is also how the clip-sensitive calls are told what they are drawing into:
    ///
    /// - [`print`](Self::print) wraps overflow onto the next row. Clipped to a one-row bar, the
    ///   wrapped remainder falls outside the clip and is dropped, which is what a single-line
    ///   bar wants.
    /// - [`put_span`](Self::put_span) and [`put_span_uniform`](Self::put_span_uniform) refuse a
    ///   footprint that leaves the clip. Clipped to a content rect, "fits" stops meaning "fits
    ///   the screen" and starts meaning "does not reserve cells in the status bar below".
    ///
    /// A sub-surface that should instead *represent* `rect` (e.g. a widget's own region, laid out
    /// and centered against `rect` rather than the parent's wider area) wants
    /// [`scope`](Self::scope), not this.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{Grid, Pos, Rect, Style, Surface};
    ///
    /// let mut grid = Grid::new(6, 2);
    /// let mut screen = Surface::new(&mut grid, Rect::new(0, 0, 6, 2), 0);
    ///
    /// // A title too long for the one-row bar at the top: the remainder wraps out of the
    /// // clip instead of onto the map below.
    /// screen
    ///     .clip(Rect::new(0, 0, 6, 1))
    ///     .print((0, 0), "retroglyph", Style::default());
    ///
    /// assert_eq!(grid[Pos::new(0, 0)].glyph(), 'r');
    /// assert_eq!(grid[Pos::new(0, 1)].glyph(), ' ');
    /// ```
    #[must_use]
    pub fn clip(&mut self, rect: Rect) -> Surface<'_> {
        Surface {
            area: self.area,
            clip: self.clip.intersect(rect),
            grid: self.grid,
            layer: self.layer,
            tint: self.tint,
            origin_offset: self.origin_offset,
        }
    }

    /// A new surface over the same grid and layer, that *represents* `rect`: its
    /// [`area`](Self::area) becomes `rect`, and its [`clip_rect`](Self::clip_rect) is narrowed to
    /// `rect` intersected with this surface's own clip.
    ///
    /// This is the primitive a widget's own region is built from: a sub-widget laid out against
    /// `rect` should center, align, and measure itself against `rect` (via [`area`](Self::area)),
    /// while still being unable to draw outside whatever was already visible in the parent. A
    /// clip alone cannot do this, because [`clip`](Self::clip) leaves `area` untouched; `scope`
    /// is what a caller reaches for when handing a sub-rect down to something that is going to
    /// read that rect back off the surface.
    ///
    /// Like [`clip`](Self::clip), the clip narrows monotonically: a `rect` that reaches outside
    /// this surface's own clip only ever tightens what the returned surface can draw into, never
    /// widens it, even though `area` itself becomes exactly `rect`.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{Grid, Rect, Surface};
    ///
    /// let mut grid = Grid::new(8, 4);
    /// let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 8, 4), 0);
    ///
    /// let mut clipped = surface.clip(Rect::new(0, 0, 4, 4));
    /// // `scope` widens `area` to a rect the parent's clip does not fully cover...
    /// let scoped = clipped.scope(Rect::new(2, 0, 4, 4));
    /// assert_eq!(scoped.area(), Rect::new(2, 0, 4, 4));
    /// // ...but the visible region still cannot exceed the parent's own clip.
    /// assert_eq!(scoped.clip_rect(), Rect::new(2, 0, 2, 4));
    /// ```
    #[must_use]
    pub fn scope(&mut self, rect: Rect) -> Surface<'_> {
        Surface {
            area: rect,
            clip: self.clip.intersect(rect),
            grid: self.grid,
            layer: self.layer,
            tint: self.tint,
            origin_offset: self.origin_offset,
        }
    }

    /// A view whose `(0, 0)` sits at `origin` relative to this surface's own coordinate space, so
    /// a caller can draw in a shifted (e.g. world/camera) coordinate space and let the surface do
    /// the clipping, rather than subtracting `origin` from every coordinate by hand.
    ///
    /// Every coordinate-taking method on the returned surface ([`put`](Self::put),
    /// [`put_signed`](Self::put_signed), [`print`](Self::print), [`print_line`](Self::print_line),
    /// [`fill_rect`](Self::fill_rect), [`put_offset`](Self::put_offset),
    /// [`put_span`](Self::put_span), [`put_span_uniform`](Self::put_span_uniform), and
    /// [`clear_region`](Self::clear_region)) subtracts `origin` (composed with any outstanding
    /// translate) from the coordinate it is given before applying its usual bounds check. Only
    /// [`clear`](Self::clear), which takes no coordinate and always clears this surface's whole
    /// area, is unaffected.
    ///
    /// This does not touch [`area`](Self::area) or [`clip_rect`](Self::clip_rect), so both, along
    /// with [`width`](Self::width) and [`height`](Self::height), keep reporting the same thing
    /// before and after translating: only the coordinate a caller must pass to land a write
    /// shifts, never what the surface itself covers or what is visible in it. This composes with
    /// [`scope`](Self::scope) the same order it is called in: `scope(...).translate(...)` first
    /// narrows the area and clip, then shifts the coordinate space that still-narrowed area is
    /// addressed in, so a coordinate that goes negative after the shift can land inside the
    /// pre-narrowed area.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{Grid, Pos, Rect, Style, Surface};
    ///
    /// let mut grid = Grid::new(10, 10);
    /// let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 10, 10), 0);
    ///
    /// // Narrow to a 4x4 viewport, then shift its coordinate space by (-5, -5): translating
    /// // does not move or resize the viewport itself.
    /// let mut scoped = surface.scope(Rect::new(5, 5, 4, 4));
    /// let mut view = scoped.translate((-5, -5));
    /// assert_eq!(view.area(), Rect::new(5, 5, 4, 4));
    ///
    /// // (-5, -5) minus the translate offset (-5, -5) is (0, 0): the viewport's own local
    /// // origin, which lands at the viewport's top-left grid cell (5, 5).
    /// view.put_signed((-5, -5), 'X', Style::default());
    ///
    /// assert_eq!(grid[Pos::new(5, 5)].glyph(), 'X');
    /// ```
    #[must_use]
    pub const fn translate(&mut self, origin: (i32, i32)) -> Surface<'_> {
        Surface {
            grid: self.grid,
            area: self.area,
            clip: self.clip,
            layer: self.layer,
            tint: self.tint,
            origin_offset: (
                self.origin_offset.0.saturating_add(origin.0),
                self.origin_offset.1.saturating_add(origin.1),
            ),
        }
    }

    /// [`clip`](Self::clip) to `area`, then [`translate`](Self::translate) by `origin`, in one
    /// call -- except that unlike plain [`clip`](Self::clip), the returned surface's
    /// [`area`](Self::area) is `area` intersected with this surface's own area, not `area`
    /// verbatim.
    ///
    /// Chaining `clip(...).translate(...)` directly works when the result is used right where
    /// it's produced (both `clip` and `translate` return a `Surface<'_>` borrowing the previous
    /// step for exactly that call), but a helper that hands the composed view back to its own
    /// caller (for example [`Camera::surface`](crate::Camera::surface)) needs the two
    /// narrowings applied against a single `&mut self` borrow instead, so the returned surface
    /// can outlive the call. This does that.
    ///
    /// This intersects `area` with this surface's own area rather than replacing it the way
    /// [`scope`](Self::scope) does, so [`area`](Self::area)/[`width`](Self::width)/
    /// [`height`](Self::height) on the result can report something smaller than the `area`
    /// argument. [`Camera::surface`](crate::Camera::surface) relies on exactly this: when the
    /// world is smaller than the viewport, it hands in a viewport-sized `area` and depends on the
    /// intersection to shrink it back down to the world's own size. A caller that wants `area`
    /// to become exactly its argument, even reaching outside the parent's current area, should
    /// use [`scope`](Self::scope) followed by [`translate`](Self::translate) instead.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{Grid, Pos, Rect, Style, Surface};
    ///
    /// let mut grid = Grid::new(10, 10);
    /// let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 10, 10), 0);
    ///
    /// let mut view = surface.clip_translate(Rect::new(5, 5, 4, 4), (-5, -5));
    /// assert_eq!(view.area(), Rect::new(5, 5, 4, 4));
    ///
    /// view.put_signed((-5, -5), 'X', Style::default());
    /// assert_eq!(grid[Pos::new(5, 5)].glyph(), 'X');
    /// ```
    #[must_use]
    pub fn clip_translate(&mut self, area: Rect, origin: (i32, i32)) -> Surface<'_> {
        let area = self.area.intersect(area);
        Surface {
            area,
            clip: self.clip.intersect(area),
            grid: self.grid,
            layer: self.layer,
            tint: self.tint,
            origin_offset: (
                self.origin_offset.0.saturating_add(origin.0),
                self.origin_offset.1.saturating_add(origin.1),
            ),
        }
    }

    /// A styled view over this surface: same area and layer, but every draw call uses `style`
    /// without needing to pass it each time. Handy for a run of same-styled writes (e.g. filling
    /// in a wall glyph over many cells) without repeating the [`Style`] at every call site.
    pub const fn with_style(&mut self, style: Style) -> StyledSurface<'_, 'a> {
        StyledSurface {
            surface: self,
            style,
        }
    }

    /// Borrows the underlying [`Grid`] directly, with no clipping.
    ///
    /// Escape hatch for multi-layer or whole-grid operations (e.g. [`Grid::blit`]) that don't fit
    /// this surface's clipped, single-layer model. Drawing into a sub-rect is not one of those:
    /// [`clip`](Self::clip) and [`scope`](Self::scope) narrow a surface without handing out the
    /// unclipped grid to do it.
    pub const fn grid_mut(&mut self) -> &mut Grid {
        self.grid
    }

    /// Read-only counterpart of [`grid_mut`](Self::grid_mut).
    #[must_use]
    pub const fn grid(&self) -> &Grid {
        self.grid
    }

}
