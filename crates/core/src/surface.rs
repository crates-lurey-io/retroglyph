//! [`Surface`]: an area-clipped, single-layer view over a [`Grid`].
//!
//! `Surface` is the workspace's one grid-drawing primitive. [`Terminal`](crate::Terminal)'s
//! [`draw`](crate::Terminal::draw)/[`surface`](crate::Terminal::surface) hand out a `Surface`
//! scoped to the whole grid, and `retroglyph-widgets` renders every widget into a `Surface`
//! scoped to a sub-[`Rect`]: there is no separate stateful drawing API on `Terminal` itself.
//!
//! Place characters directly with [`put`](Surface::put) (or [`print`](Surface::print) for a
//! string, which handles newlines and wide characters), style-aware spans with
//! [`print_line`](Surface::print_line), or a whole styled run with
//! [`with_style`](Surface::with_style) so repeated calls don't need to pass the same [`Style`]
//! each time. [`clear`](Surface::clear)/[`clear_region`](Surface::clear_region) blank the active
//! layer (in full, or a rectangular region); switch layers with
//! [`on_layer`](Surface::on_layer). Or bypass the builder entirely and reach the [`Grid`]
//! directly via [`grid_mut`](Surface::grid_mut).

use crate::color::Color;
use crate::grid::{Grid, Offset, Pos, Rect, Size};
use crate::style::Style;
use crate::text::Line;
use crate::tile::Tile;
use crate::tint::Tint;
use ixy::HasSize;
#[cfg(not(feature = "egc"))]
use unicode_width::UnicodeWidthChar;

/// The render target for every drawing call in the workspace: a mutable reference to a
/// [`Grid`] plus a fixed `layer`, scoped to an `area` and clipped to a `clip` rect.
///
/// A `Surface` is typically created once per frame, scoped to the whole drawing surface (e.g.
/// via [`Terminal::draw`](crate::Terminal::draw)), and handed to every subsystem/widget in turn;
/// each caller's own `area: Rect` (a sub-rect of the surface's own area, e.g. one produced by a
/// layout split) is relative to this surface's own `area` origin, not to the underlying grid.
/// [`Surface::put`]/[`Surface::print`]/... take coordinates in that same local space, where
/// `(0, 0)` is `area`'s top-left corner, and silently drop any write that falls outside
/// [`Surface::clip_rect`], matching the rest of the workspace's clip-on-draw policy for
/// out-of-bounds drawing.
///
/// `area` and `clip_rect` answer two different questions. `area` is the region this surface
/// *represents*: what a widget lays itself out in, and what [`width`](Self::width)/
/// [`height`](Self::height) report. `clip_rect` is the subset of `area` that is actually
/// *visible*: what every write is bounds-checked against. The two start out equal (see
/// [`Surface::new`]) and diverge once [`Surface::clip`] or [`Surface::scope`] is used.
///
/// [`Surface::clip`] narrows what is visible without changing what this surface represents:
/// `clip_rect` is intersected with the given rect, `area` is untouched. [`Surface::scope`] does
/// both: `area` becomes the given rect and `clip_rect` is intersected with it, which is what a
/// widget's own sub-surface needs when it should be laid out against a new rect but still bounded
/// by whatever was already visible. Both narrow monotonically: neither can widen `clip_rect`
/// beyond what the parent surface already allowed.
///
/// A caller that genuinely needs more than one layer at once (e.g. a modal dimming layer 0 while
/// drawing its own content on layer 1) switches layers with [`Surface::on_layer`]/[`Surface::on_tier`]
/// rather than being restricted to the layer it was constructed with.
pub struct Surface<'a> {
    grid: &'a mut Grid,
    area: Rect,
    clip: Rect,
    layer: u8,
    tint: Tint,
    origin_offset: (i32, i32),
}

/// A named z-order tier for [`Surface::on_tier`], covering the split most apps with overlapping
/// UI actually need.
///
/// Layers are how overlapping UI avoids depending on draw order: a caller who paints a dropdown
/// on [`Layer::Overlay`] gets it on top of the active screen regardless of whether the screen or
/// the dropdown drew first this frame, so the two don't have to agree on an ordering (contrast
/// with painting both through the same layer, where whichever call happens to run last wins).
///
/// `Layer` derives [`Ord`] over its declaration order, so `Layer::World < Layer::Hud <
/// Layer::Overlay < Layer::Debug` holds without spelling out the underlying grid layer ids --
/// the same relationship [`Surface::on_tier`] relies on to keep `Layer::Debug` the top-most tier
/// no matter what else is open.
///
/// This is a convention, not a restriction: [`Surface::on_layer`] still accepts any `u8`, and a
/// tile map or sprite-heavy app with its own multi-layer scheme (terrain/items/actors/...) has no
/// reason to route through `Layer` at all. `Layer` exists for the overlapping-*UI* case:
/// chrome, popups, debug HUDs, where a small, shared, named split is worth more than 256 open
/// numeric ids.
///
/// # Examples
///
/// A persistent HUD bar and a dropdown that must paint over it, in either order, because they're
/// on different tiers rather than racing to draw last:
///
/// ```
/// use retroglyph_core::{Grid, Layer, Rect, Style, Surface};
///
/// let area = Rect::new(0, 0, 20, 5);
/// let mut grid = Grid::new(20, 5);
/// let mut surface = Surface::new(&mut grid, area, Layer::World.as_u8());
///
/// // The active screen draws on `World`.
/// surface.print((0, 0), "screen content", Style::default());
///
/// // Chrome draws on `Hud`, above the screen.
/// surface.on_tier(Layer::Hud).print((0, 0), "File  Edit  View", Style::default());
///
/// // A dropdown draws on `Overlay`, above the HUD: painting it before or after the two calls
/// // above makes no difference, because it's on a higher tier, not drawn later.
/// surface.on_tier(Layer::Overlay).print((0, 1), "New", Style::default());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
#[non_exhaustive]
pub enum Layer {
    /// The active screen: terrain, entities, game/app content. Grid layer 0.
    #[default]
    World,
    /// Persistent chrome: menu bars, status lines, HUD. Grid layer 1.
    Hud,
    /// Popups, dropdowns, modals, painted over [`Layer::World`] and [`Layer::Hud`] regardless
    /// of draw order. Grid layer 2.
    Overlay,
    /// Debug and dev tooling. Always the top-most tier, so it stays visible over an open
    /// [`Layer::Overlay`] rather than being hidden underneath one. Grid layer 3.
    ///
    /// [`crate::perf_overlay::DEFAULT_LAYER`] is defined as `Layer::Debug.as_u8()` for exactly
    /// this reason: a perf HUD that a popup could paint over would be useless whenever an app
    /// actually has a popup open.
    Debug,
}

impl Layer {
    /// This tier's underlying grid layer id, for [`Surface::on_layer`]/[`Grid`] APIs that take a
    /// raw `u8`.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

impl From<Layer> for u8 {
    fn from(layer: Layer) -> Self {
        layer.as_u8()
    }
}

impl<'a> Surface<'a> {
    /// A surface over `grid`, scoped to `area` on `layer`, tinting nothing. `area` starts out
    /// fully visible: [`area`](Self::area) and [`clip_rect`](Self::clip_rect) are equal until
    /// [`clip`](Self::clip) or [`scope`](Self::scope) narrows the latter.
    pub const fn new(grid: &'a mut Grid, area: Rect, layer: u8) -> Self {
        Self {
            grid,
            area,
            clip: area,
            layer,
            tint: Tint::None,
            origin_offset: (0, 0),
        }
    }

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
    /// draws the sprite from.
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
    /// Coordinates are unchanged: the sub-surface addresses the same space this one does, so a
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
    /// call.
    ///
    /// Chaining `clip(...).translate(...)` directly works when the result is used right where
    /// it's produced (both `clip` and `translate` return a `Surface<'_>` borrowing the previous
    /// step for exactly that call), but a helper that hands the composed view back to its own
    /// caller (for example [`Camera::surface`](crate::Camera::surface)) needs the two
    /// narrowings applied against a single `&mut self` borrow instead, so the returned surface
    /// can outlive the call. This does that.
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

    /// The tile at `pos` on this surface's layer, if any.
    ///
    /// Respects this surface's layer but not its clip, mirroring [`grid_mut`](Self::grid_mut) in
    /// that sense: a caller wanting a clipped read should check
    /// [`self.clip_rect().contains(...)`](Rect::contains) first.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{Grid, Rect, Style, Surface};
    ///
    /// let mut grid = Grid::new(4, 4);
    /// let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 4, 4), 0);
    /// surface.put((1, 1), 'X', Style::default());
    ///
    /// assert_eq!(surface.tile((1, 1)).map(|t| t.glyph()), Some('X'));
    /// assert_eq!(surface.tile((0, 0)).map(|t| t.glyph()), Some(' '));
    /// ```
    #[must_use]
    pub fn tile(&self, pos: impl Into<Pos>) -> Option<&Tile> {
        self.grid.tile(self.layer, pos.into())
    }

    /// The background colour at `pos` on this surface's layer, or `None` if there's no tile
    /// there.
    ///
    /// A read-only read of a cell's own background lets a caller blend a new draw with what's
    /// already there (e.g. `surface.background(pos).unwrap_or(default)`) without the mutable
    /// borrow [`grid_mut`](Self::grid_mut) would otherwise force.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{Color, Grid, Rect, Style, Surface};
    ///
    /// let mut grid = Grid::new(4, 4);
    /// let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 4, 4), 0);
    /// surface.put((1, 1), 'X', Style::new().bg(Color::RED));
    ///
    /// assert_eq!(surface.background((1, 1)), Some(Color::RED));
    /// // Out of the grid entirely: no tile there to read a background from.
    /// assert_eq!(surface.background((10, 10)), None);
    /// ```
    #[must_use]
    pub fn background(&self, pos: impl Into<Pos>) -> Option<Color> {
        self.tile(pos).map(|t| t.style().background())
    }

    /// Shifts `(x, y)` by this surface's translate offset (see [`translate`](Self::translate)),
    /// returning the coordinate to actually write at if the shift still lands inside this
    /// surface's own clip, or `None` otherwise.
    ///
    /// The subtracted result is a coordinate local to this surface's area (`(0, 0)` is the
    /// area's own top-left, matching [`put_signed`](Self::put_signed)'s convention), not an
    /// absolute grid coordinate: a local check against `(0, 0)..(width, height)` here, followed
    /// by re-adding [`area`](Self::area)'s own top-left, so a clipped area that does not itself
    /// start at grid `(0, 0)` (e.g. [`Camera::surface`](crate::Camera::surface)'s
    /// `clip_translate`) still resolves to the right absolute cell. The result is then checked
    /// against [`clip_rect`](Self::clip_rect), not `area`, since the clip, never the area, is
    /// what decides whether a write lands.
    fn shift(&self, x: u16, y: u16) -> Option<(u16, u16)> {
        let sx = i32::from(x).checked_sub(self.origin_offset.0)?;
        let sy = i32::from(y).checked_sub(self.origin_offset.1)?;
        if sx < 0 || sy < 0 {
            return None;
        }
        let sx = u16::try_from(sx).ok()?;
        let sy = u16::try_from(sy).ok()?;
        if sx >= self.area.width() || sy >= self.area.height() {
            return None;
        }
        let gx = self.area.left() + sx;
        let gy = self.area.top() + sy;
        self.clip.contains(gx, gy).then_some((gx, gy))
    }

    /// The whole-rect counterpart to [`shift`](Self::shift): translates a local `rect` (same
    /// convention as `fill_rect`/`clear_region`, and as `shift`'s own `x`/`y`) into the absolute
    /// grid rect it covers under this surface's own clip, or `None` if none of it lands.
    ///
    /// Only handles `origin_offset == (0, 0)` (true of every surface except one produced by
    /// [`clip_translate`](Self::clip_translate)): with any other offset `shift` still refuses a
    /// negative post-offset coordinate per cell, which a single rect-wide translation can't
    /// reproduce without re-deriving per-cell bounds, so callers fall back to `shift` itself
    /// instead.
    fn local_rect_to_absolute(&self, rect: Rect) -> Option<Rect> {
        if self.origin_offset != (0, 0) {
            return None;
        }
        let local = rect.intersect(Rect::new(0, 0, self.area.width(), self.area.height()));
        if local.is_empty() {
            return None;
        }
        let abs = Rect::new(
            self.area.left() + local.left(),
            self.area.top() + local.top(),
            local.width(),
            local.height(),
        )
        .intersect(self.clip);
        (!abs.is_empty()).then_some(abs)
    }

    /// Applies this surface's tint to the cell just written at `(x, y)`.
    ///
    /// Called after a write rather than as part of one, because a glyph write drops whatever
    /// tint the cell held (see [`Grid::set_tint`]); doing it in the other order would erase the
    /// tint being applied. Untinted surfaces skip the call entirely, so the ordinary text path
    /// never touches the side table.
    fn apply_tint(&mut self, x: u16, y: u16) {
        if self.tint != Tint::None {
            self.grid.set_tint(self.layer, x, y, self.tint);
        }
    }

    /// Writes `grapheme` (already a single extended grapheme cluster) at `(x, y)`. A no-op if
    /// out of this surface's clip.
    ///
    /// A 2-column grapheme also needs its spacer cell (`x + 1`) inside the clip: `shift` only
    /// checks the primary cell, and `Grid::write_grapheme` only refuses the spacer at the
    /// *grid*'s own edge, not the clip's, so without this the spacer would land one column past
    /// the clip. Refusing the whole write here (rather than writing a primary cell with no
    /// spacer) matches [`span_fits`](Self::span_fits)'s reasoning: a footprint half outside the
    /// clip would reserve a cell the caller does not own.
    #[cfg(feature = "egc")]
    fn put_grapheme(&mut self, x: u16, y: u16, grapheme: &str, style: Style) {
        let Some((x, y)) = self.shift(x, y) else {
            return;
        };
        self.write_grapheme_at(x, y, grapheme, style);
    }

    /// Writes `grapheme` at the already-*absolute* grid coordinate `(x, y)` (post-[`shift`],
    /// or an equivalent translation a caller had to do by hand): the width-2 spacer-in-clip
    /// check, [`Grid::write_grapheme`]'s wide-char bookkeeping, and this surface's tint.
    ///
    /// Shared by [`put_grapheme`](Self::put_grapheme) and [`put_signed`](Self::put_signed),
    /// which cannot just call `put_grapheme` with its own local coordinates: `put_signed`
    /// already subtracts `origin_offset` itself (see its doc), so routing through `shift` again
    /// would subtract it twice.
    #[cfg(feature = "egc")]
    fn write_grapheme_at(&mut self, x: u16, y: u16, grapheme: &str, style: Style) {
        use unicode_width::UnicodeWidthStr;

        if grapheme.width() == 2 && !self.clip.contains(x.saturating_add(1), y) {
            return;
        }
        self.grid.write_grapheme(self.layer, x, y, grapheme, style);
        self.apply_tint(x, y);
    }

    /// Place `ch` at `pos` in `style`. A no-op if `pos` is outside this surface's clip.
    ///
    /// If a pixel backend resolves `ch` to a sprite, that sprite is composited from its own
    /// pixels: [`style.fg`](Style::fg) does not tint it, and `style.bg` shows through only where
    /// the sprite is transparent. See [`put_span`](Self::put_span).
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{Grid, Pos, Rect, Style, Surface};
    ///
    /// let mut grid = Grid::new(4, 4);
    /// let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 4, 4), 0);
    ///
    /// surface.put((1, 1), 'X', Style::default());
    /// // Outside the surface's clip: silently dropped, not a panic.
    /// surface.put((10, 10), 'X', Style::default());
    ///
    /// assert_eq!(grid[Pos::new(1, 1)].glyph(), 'X');
    /// ```
    pub fn put(&mut self, pos: impl Into<Pos>, ch: char, style: Style) {
        let pos = pos.into();
        #[cfg(feature = "egc")]
        {
            let mut buf = [0u8; 4];
            let s = ch.encode_utf8(&mut buf);
            self.put_grapheme(pos.x, pos.y, s, style);
        }
        #[cfg(not(feature = "egc"))]
        {
            let Some((x, y)) = self.shift(pos.x, pos.y) else {
                return;
            };
            let tile = Tile::new(ch, style);
            self.grid.put_tile(self.layer, (x, y), tile);
            self.apply_tint(x, y);
        }
    }

    /// [`put`](Self::put), in coordinates relative to this surface's own area origin, where a
    /// negative coordinate is expressible and simply falls outside (a no-op, matching `put`'s
    /// out-of-bounds behavior).
    ///
    /// Scrolling/camera code (e.g. a viewport over a wider world) computes positions in a
    /// coordinate space that can go negative relative to the viewport, which [`Pos`] (backed by
    /// `u16`) cannot even express. `put_signed` takes that arithmetic directly, so a caller no
    /// longer clip-tests by hand before calling `put`.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{Grid, Pos, Rect, Style, Surface};
    ///
    /// let mut grid = Grid::new(4, 4);
    /// let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 4, 4), 0);
    ///
    /// // Negative in either axis: outside this surface's area, silently dropped.
    /// surface.put_signed((-1, 1), 'X', Style::default());
    /// // Non-negative and within bounds: lands like `put`.
    /// surface.put_signed((1, 1), 'X', Style::default());
    ///
    /// assert_eq!(grid[Pos::new(1, 1)].glyph(), 'X');
    /// assert_eq!(grid[Pos::new(0, 1)].glyph(), ' ');
    /// ```
    pub fn put_signed(&mut self, pos: (i32, i32), ch: char, style: Style) {
        let (x, y) = pos;
        let x = x.saturating_sub(self.origin_offset.0);
        let y = y.saturating_sub(self.origin_offset.1);
        if x < 0 || y < 0 {
            return;
        }
        let Ok(x) = u16::try_from(x) else {
            return;
        };
        let Ok(y) = u16::try_from(y) else {
            return;
        };
        if x >= self.width() || y >= self.height() {
            return;
        }
        let abs_x = self.area.left() + x;
        let abs_y = self.area.top() + y;
        if !self.clip.contains(abs_x, abs_y) {
            return;
        }
        #[cfg(feature = "egc")]
        {
            let mut buf = [0u8; 4];
            let s = ch.encode_utf8(&mut buf);
            self.write_grapheme_at(abs_x, abs_y, s, style);
        }
        #[cfg(not(feature = "egc"))]
        {
            let tile = Tile::new(ch, style);
            self.grid.put_tile(self.layer, (abs_x, abs_y), tile);
            self.apply_tint(abs_x, abs_y);
        }
    }

    /// Print `text` starting at `pos` in `style`.
    ///
    /// `\n` advances to the next row at the original column. Text that would extend beyond this
    /// surface's clip wraps to the next row at the original column; cells outside the clip
    /// (either axis) are dropped. When the `egc` feature is enabled, `text` is split into
    /// extended grapheme clusters (so combining marks and ZWJ sequences write as one cell each);
    /// otherwise it is split by `char`.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::backend::Headless;
    /// use retroglyph_core::{Style, Terminal};
    ///
    /// let mut term = Terminal::new(Headless::new(6, 3));
    /// term.draw(|s| s.print((0, 0), "hello wrapped world", Style::default()))
    ///     .unwrap();
    ///
    /// // Wraps back to column 0 every 6 cells; the surface is only 3 rows tall, so
    /// // the remainder past row 2 is clipped rather than growing the grid.
    /// assert_eq!(
    ///     term.backend().format_view(),
    ///     "hello·\nwrappe\nd·worl\n",
    /// );
    /// ```
    pub fn print(&mut self, pos: impl Into<Pos>, text: &str, style: Style) {
        let pos = pos.into();
        #[cfg(feature = "egc")]
        self.print_egc(pos, text, style);
        #[cfg(not(feature = "egc"))]
        self.print_chars(pos, text, style);
    }

    /// [`print`](Self::print) implementation used when `egc` is enabled: splits on extended
    /// grapheme clusters rather than `char`.
    #[cfg(feature = "egc")]
    fn print_egc(&mut self, pos: Pos, text: &str, style: Style) {
        use unicode_segmentation::UnicodeSegmentation;
        use unicode_width::UnicodeWidthStr;

        // `cx` is area-local (it starts at `pos.x`, which `shift()` treats as local), so the
        // wrap threshold must be too: translate `clip.right()` out of absolute grid space.
        let right = self.clip.right().saturating_sub(self.area.left());
        let mut cx = pos.x;
        let mut cy = pos.y;
        for grapheme in text.graphemes(true) {
            if grapheme == "\n" {
                cx = pos.x;
                cy = cy.saturating_add(1);
                continue;
            }
            // A single grapheme's display width is 0, 1, or 2 per `unicode-width` (see
            // `Tile::width`'s doc comment), never anywhere near `u16::MAX`.
            #[allow(clippy::cast_possible_truncation)]
            let w = grapheme.width() as u16;
            if w == 0 {
                continue;
            }
            self.put_grapheme(cx, cy, grapheme, style);
            cx = cx.saturating_add(w);
            if cx >= right {
                cx = pos.x;
                cy = cy.saturating_add(1);
            }
        }
    }

    /// [`print`](Self::print) implementation used when `egc` is disabled: splits on `char`.
    #[cfg(not(feature = "egc"))]
    fn print_chars(&mut self, pos: Pos, text: &str, style: Style) {
        // `cx` is area-local (it starts at `pos.x`, which `shift()` treats as local), so the
        // wrap threshold must be too: translate `clip.right()` out of absolute grid space.
        let right = self.clip.right().saturating_sub(self.area.left());
        let mut cx = pos.x;
        let mut cy = pos.y;
        for ch in text.chars() {
            if ch == '\n' {
                cx = pos.x;
                cy = cy.saturating_add(1);
                continue;
            }
            // A single char's display width is 0, 1, or 2 per `unicode-width` (see `Tile::width`'s
            // doc comment), never anywhere near `u16::MAX`.
            #[allow(clippy::cast_possible_truncation)]
            let w = UnicodeWidthChar::width(ch).unwrap_or(1) as u16;
            if w == 0 {
                continue;
            }
            self.put((cx, cy), ch, style);
            cx = cx.saturating_add(w);
            if cx >= right {
                cx = pos.x;
                cy = cy.saturating_add(1);
            }
        }
    }

    /// Print `line`'s styled spans starting at `pos`, one row, each span in its own style.
    /// Stops once a span would start past this surface's clip.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::backend::Headless;
    /// use retroglyph_core::text::{Line, Span};
    /// use retroglyph_core::Terminal;
    ///
    /// let mut term = Terminal::new(Headless::new(5, 2));
    /// let line = Line::from(vec![Span::raw("hello"), Span::raw("world")]);
    /// term.draw(|s| s.print_line((0, 0), &line)).unwrap();
    ///
    /// // The first span exactly fills the one-row area. The second span would start at
    /// // column 5, past the area, so it is skipped entirely rather than wrapped onto the
    /// // next row the way `print` would wrap.
    /// assert_eq!(term.backend().format_view(), "hello\n·····\n");
    /// ```
    pub fn print_line(&mut self, pos: impl Into<Pos>, line: &Line) {
        use unicode_width::UnicodeWidthStr;

        let pos = pos.into();
        // `cx` is area-local (it starts at `pos.x`, which `shift()` treats as local), so the
        // span-skip threshold must be too: translate `clip.right()` out of absolute grid space.
        let right = self.clip.right().saturating_sub(self.area.left());
        let mut cx = pos.x;
        for span in &line.spans {
            if cx >= right {
                break;
            }
            self.print((cx, pos.y), &span.content, span.style);
            // A single span wider than `u16::MAX` columns would already be unaddressable in this
            // crate's `u16` coordinate space; `cx` still saturates rather than overflowing even if
            // this cast wraps.
            #[allow(clippy::cast_possible_truncation)]
            let w = UnicodeWidthStr::width(span.content.as_str()) as u16;
            cx = cx.saturating_add(w);
        }
    }

    /// [`print`](Self::print), horizontally aligned within `rect` (clipped to this surface's own
    /// clip) and measured in display columns (via `unicode_width`), not bytes.
    ///
    /// Wants a per-frame redrawn UI label (a status line, a centred title bar) that should not
    /// allocate: unlike [`TextLayout`](crate::layout::TextLayout), which only accepts a
    /// [`Line`] (forcing an allocation to build one for every call), this
    /// takes `&str` directly.
    ///
    /// The starting column is computed with saturating arithmetic, so `text` wider than `rect`
    /// does not panic or underflow: it simply left-aligns and lets [`print`](Self::print) clip
    /// the overflow, for every [`HAlign`](crate::align::HAlign) (matching how
    /// [`HAlign::Center`](crate::align::HAlign::Center) itself saturates in
    /// [`TextLayout`](crate::layout::TextLayout)).
    ///
    /// Not gated behind the `egc` feature: unlike `TextLayout`, this needs nothing from it, so
    /// it's reachable from any crate that only measures with `unicode-width`, including
    /// `retroglyph-widgets` without opting into `egc`.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::backend::Headless;
    /// use retroglyph_core::align::HAlign;
    /// use retroglyph_core::{Rect, Style, Terminal};
    ///
    /// let mut term = Terminal::new(Headless::new(6, 1));
    /// term.draw(|s| {
    ///     s.print_aligned(Rect::new(0, 0, 6, 1), "hi", HAlign::Center, Style::default())
    /// })
    /// .unwrap();
    ///
    /// // "hi" is 2 columns wide in a 6-column rect: (6 - 2) / 2 == 2 columns of left padding.
    /// assert_eq!(term.backend().format_view(), "··hi··\n");
    /// ```
    pub fn print_aligned(
        &mut self,
        rect: Rect,
        text: &str,
        align: crate::align::HAlign,
        style: Style,
    ) {
        use unicode_width::UnicodeWidthStr;

        // A single line's display width is never anywhere near `u16::MAX` (see `print_line`'s
        // own use of this same cast for a single span).
        #[allow(clippy::cast_possible_truncation)]
        let text_width = UnicodeWidthStr::width(text) as u16;
        let x_offset = align.offset(rect.width(), text_width);
        // `clip` treats `rect` as absolute (it intersects `self.clip`, itself absolute), but
        // `print` treats its `pos` as local to `self.area` (see `shift`). Compute the aligned
        // start column in `rect`'s absolute space, then translate it into `self.area`-local
        // space before handing it to `print`, or it silently clips away on any surface whose
        // `area` doesn't start at grid column/row 0.
        let pos = (
            rect.left()
                .saturating_add(x_offset)
                .saturating_sub(self.area.left()),
            rect.top().saturating_sub(self.area.top()),
        );
        self.clip(rect).print(pos, text, style);
    }

    /// Fill `rect` (clipped to this surface's own clip) with `ch` in `style`.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{Grid, Pos, Rect, Style, Surface};
    ///
    /// let mut grid = Grid::new(4, 4);
    /// let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 4, 4), 0);
    ///
    /// // `rect` extends well past the grid on both axes; only the cells inside the
    /// // surface's own clip are touched, the rest is silently clipped.
    /// surface.fill_rect(Rect::new(2, 2, 10, 10), '#', Style::default());
    ///
    /// assert_eq!(grid[Pos::new(3, 3)].glyph(), '#');
    /// assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
    /// ```
    pub fn fill_rect(&mut self, rect: Rect, ch: char, style: Style) {
        // The batch path below writes a plain `Tile::new(ch, style)` per cell, which matches
        // `put`'s own per-cell write only when there's no tint to apply and (under `egc`) `ch`
        // is a single-column glyph, so `put`'s wide-char spacer bookkeeping never triggers.
        // Anything else (tinted surface, zero/double-width glyph) falls back to the per-cell
        // loop, unchanged from before this method had a fast path.
        #[cfg(feature = "egc")]
        let single_width = {
            use unicode_width::UnicodeWidthChar;
            UnicodeWidthChar::width(ch) == Some(1)
        };
        #[cfg(not(feature = "egc"))]
        let single_width = true;

        if self.tint == Tint::None
            && single_width
            && let Some(abs) = self.local_rect_to_absolute(rect)
        {
            self.grid.fill_region(self.layer, abs, Tile::new(ch, style));
            return;
        }

        for y in rect.top()..rect.bottom() {
            for x in rect.left()..rect.right() {
                self.put((x, y), ch, style);
            }
        }
    }

    /// Stamps `grid`'s layer 0 onto this surface's own layer, with its top-left cell at `(x, y)`
    /// (local to this surface's area, matching [`put`](Self::put)'s convention), clipped to this
    /// surface's clip.
    ///
    /// Always reads `grid`'s layer 0, regardless of which layer this surface itself is currently
    /// writing to: `grid` is typically a standalone buffer composed elsewhere (e.g.
    /// `BoxStyle::render`'s output, or `retroglyph-widgets`' `join_h`/`join_v`), and per their own
    /// docs those only ever populate layer 0. Reading this surface's own layer off `grid` instead
    /// (what [`Grid::blit`]'s single `layer` parameter would do if called directly) finds nothing
    /// there whenever this surface isn't on layer 0, and the copy silently does nothing.
    ///
    /// Unlike a single-cell [`put`](Self::put), a write that starts outside this surface's clip is
    /// not necessarily dropped whole: the part of `grid` that does land inside the clip is copied,
    /// matching [`fill_rect`](Self::fill_rect)'s per-cell clipping rather than
    /// [`put_span`](Self::put_span)'s all-or-nothing footprint check, since `grid` is arbitrary
    /// composed content rather than one indivisible sprite.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{Grid, Layer, Rect, Style, Surface, Tile};
    ///
    /// let mut src = Grid::new(2, 2);
    /// src.put_tile(0, (0, 0), Tile::new('x', Style::default()));
    ///
    /// let mut dst = Grid::new(4, 4);
    /// let mut surface = Surface::new(&mut dst, Rect::new(0, 0, 4, 4), Layer::World.as_u8());
    ///
    /// // `surface` is on the overlay tier; `src` only ever has layer 0, but `blit` reads that
    /// // layer regardless, so the copy still lands (unlike `Grid::blit(surface.layer(), ...)`).
    /// surface.on_tier(Layer::Overlay).blit(&src, 1, 1);
    ///
    /// assert_eq!(dst.tile(Layer::Overlay.as_u8(), (1, 1)).map(Tile::glyph), Some('x'));
    /// ```
    pub fn blit(&mut self, grid: &Grid, x: u16, y: u16) {
        let w = grid.width();
        let h = grid.height();
        if w == 0 || h == 0 {
            return;
        }

        // Local `(x, y)` shifted by this surface's translate offset, same subtraction `shift`
        // does for a single cell, but a footprint that starts left of/above the origin crops its
        // near edge instead of being dropped whole (there is no single `(x, y)` for `shift` to
        // reject: only part of the footprint may be off-screen).
        let sx = i64::from(x) - i64::from(self.origin_offset.0);
        let sy = i64::from(y) - i64::from(self.origin_offset.1);
        let crop_left = u16::try_from(sx.min(0).unsigned_abs()).unwrap_or(u16::MAX);
        let crop_top = u16::try_from(sy.min(0).unsigned_abs()).unwrap_or(u16::MAX);
        if crop_left >= w || crop_top >= h {
            return;
        }
        let Ok(local_x) = u16::try_from(sx.max(0)) else {
            return;
        };
        let Ok(local_y) = u16::try_from(sy.max(0)) else {
            return;
        };

        let abs_x = self.area.left().saturating_add(local_x);
        let abs_y = self.area.top().saturating_add(local_y);
        let visible_w = (w - crop_left).min(u16::MAX - abs_x);
        let visible_h = (h - crop_top).min(u16::MAX - abs_y);

        let dst_rect = Rect::new(abs_x, abs_y, visible_w, visible_h).intersect(self.clip);
        if dst_rect.is_empty() {
            return;
        }

        let src_rect = Rect::new(
            crop_left + (dst_rect.left() - abs_x),
            crop_top + (dst_rect.top() - abs_y),
            dst_rect.width(),
            dst_rect.height(),
        );
        self.grid.blit_cross_layer(
            self.layer,
            grid,
            0,
            src_rect,
            dst_rect.left(),
            dst_rect.top(),
        );
    }

    /// Writes a multi-cell span at `pos` on this surface's layer in `style`: one piece of
    /// artwork occupying a block of cells rather than one, the [`Surface`] twin of
    /// [`Grid::write_span`].
    ///
    /// `rows` holds one string per row of the footprint. Its first character is the **anchor**
    /// glyph, which a pixel backend looks up in its sprite cache; the rest are the span's **text
    /// fallback**, printed by cell backends and skipped by pixel backends. Any `AsRef<str>` row
    /// works, so a literal footprint (`&["[==]", "|__|"]`) and a computed one (`&Vec<String>`)
    /// both pass without a borrowing pass over the rows; for the uniform case, see
    /// [`put_span_uniform`](Self::put_span_uniform).
    ///
    /// See [`Grid::write_span`] for the full write semantics, and [`Grid::span_owner`] to
    /// hit-test the whole footprint.
    ///
    /// # `style` applies to the text fallback, not to the sprite
    ///
    /// A sprite is composited from its own pixels. [`style.fg`](Style::fg) does not tint it;
    /// `style.bg` is still painted behind it, so it shows through wherever the sprite is
    /// transparent. Recoloring a shared sprite per cell is therefore not possible: draw a
    /// variant of the artwork instead, which is the usual tileset idiom.
    ///
    /// `style` is not dead on such a cell, because the same span drawn by a *cell* backend
    /// renders the text fallback in it. The consequence is that `fg` reads very differently
    /// depending on the backend, and that a glyph missing from the sprite cache silently falls
    /// back to a font glyph that *is* `fg`-colored, which looks a lot like a tint working.
    ///
    /// # Returns
    ///
    /// `Some(())` once the whole span is written, or `None` having written nothing at all when
    /// `rows` is empty or ragged, either axis exceeds 255 cells, or the footprint does not fit
    /// entirely within this surface's own clip (not just the grid) at `pos`. The surface has
    /// strictly more ways to refuse a span than [`Grid::write_span`] does, so a sprite that did
    /// not draw is answered here rather than in the backend.
    pub fn put_span<S: AsRef<str>>(
        &mut self,
        pos: impl Into<Pos>,
        rows: &[S],
        style: Style,
    ) -> Option<()> {
        let pos = pos.into();
        let (x, y) = self.shift(pos.x, pos.y)?;
        let cols = rows.first()?.as_ref().chars().count();
        let w = u16::try_from(cols).ok()?;
        let h = u16::try_from(rows.len()).ok()?;
        if !self.span_fits(Pos::new(x, y), w, h) {
            return None;
        }
        self.grid.write_span(self.layer, x, y, rows, style)?;
        // The anchor only: a pixel backend draws the whole footprint from that one cell, so the
        // covered cells have no sprite of their own to recolour.
        self.apply_tint(x, y);
        Some(())
    }

    /// Writes a `size` multi-cell span at `pos` on this surface's layer in `style`: `anchor` in
    /// the anchor cell, `fill` in every other cell of the footprint, the [`Surface`] twin of
    /// [`Grid::write_span_uniform`].
    ///
    /// The uniform case of [`put_span`](Self::put_span), and what a sheet-driven renderer usually
    /// wants: one sprite, chosen at runtime, with the cells it covers blanked so nothing shows
    /// through its transparent pixels. `fill` is the text fallback a *cell* backend prints for
    /// those covered cells, so `' '` blanks them and a visible character keeps the footprint
    /// legible in a terminal.
    ///
    /// `style` reads exactly as it does for [`put_span`](Self::put_span): it applies to the text
    /// fallback, never to the sprite.
    ///
    /// # Returns
    ///
    /// `Some(())` once the whole span is written, or `None` having written nothing at all when
    /// either axis of `size` is `0` or exceeds 255 cells, or the footprint does not fit entirely
    /// within this surface's own clip at `pos`.
    ///
    /// # Examples
    ///
    /// ```
    /// # fn main() {
    /// # fn run() -> Option<()> {
    /// use retroglyph_core::{Grid, Rect, Style, Surface};
    ///
    /// let mut grid = Grid::new(8, 4);
    /// let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 8, 4), 0);
    ///
    /// // A 16x16 sprite over a 2x1 block of 8x16 cells, anchored at a runtime glyph.
    /// let anchor = '\u{E000}';
    /// surface.put_span_uniform((1, 1), (2, 1), anchor, ' ', Style::default())?;
    /// # Some(())
    /// # }
    /// # run().unwrap();
    /// # }
    /// ```
    pub fn put_span_uniform(
        &mut self,
        pos: impl Into<Pos>,
        size: impl Into<Size>,
        anchor: char,
        fill: char,
        style: Style,
    ) -> Option<()> {
        let pos = pos.into();
        let (x, y) = self.shift(pos.x, pos.y)?;
        let pos = Pos::new(x, y);
        let size = size.into();
        if !self.span_fits(pos, size.width(), size.height()) {
            return None;
        }
        self.grid
            .write_span_uniform(self.layer, pos, size, anchor, fill, style)?;
        self.apply_tint(pos.x, pos.y);
        Some(())
    }

    /// `true` if a `w` x `h` footprint at `pos` lies entirely within this surface's clip.
    ///
    /// A span is all-or-nothing rather than clipped like the per-cell writes, because a
    /// footprint half outside the clip would reserve cells the caller does not own.
    fn span_fits(&self, pos: Pos, w: u16, h: u16) -> bool {
        pos.x >= self.clip.left()
            && pos.y >= self.clip.top()
            && pos.x.saturating_add(w) <= self.clip.right()
            && pos.y.saturating_add(h) <= self.clip.bottom()
    }

    /// Place `ch` at `pos` with a sub-cell pixel `offset`, in `style`.
    ///
    /// Sub-cell offsets are visual only: they do not affect grid logic or hit-testing.
    /// Backends that cannot represent pixel offsets (e.g. `CrosstermBackend`) ignore them. A
    /// no-op if `pos` is outside this surface's clip.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{Grid, Offset, Pos, Rect, Style, Surface};
    ///
    /// let mut grid = Grid::new(4, 4);
    /// let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 4, 4), 0);
    ///
    /// // A large offset still lands the glyph in cell (1, 1): the offset is a pixel nudge
    /// // for a pixel backend, never a coordinate shift.
    /// surface.put_offset((1, 1), Offset::new(12, -12), 'X', Style::default());
    /// // Outside the surface's clip: silently dropped, matching `put`.
    /// surface.put_offset((10, 10), Offset::default(), 'X', Style::default());
    ///
    /// assert_eq!(grid[Pos::new(1, 1)].glyph(), 'X');
    /// ```
    pub fn put_offset(
        &mut self,
        pos: impl Into<Pos>,
        offset: impl Into<Offset>,
        ch: char,
        style: Style,
    ) {
        let pos = pos.into();
        let Some((x, y)) = self.shift(pos.x, pos.y) else {
            return;
        };
        let offset = offset.into();
        #[cfg(feature = "egc")]
        {
            let mut buf = [0u8; 4];
            let s = ch.encode_utf8(&mut buf);
            self.write_grapheme_at(x, y, s, style);
        }
        #[cfg(not(feature = "egc"))]
        {
            let tile = Tile::new(ch, style);
            self.grid.put_tile(self.layer, (x, y), tile);
            self.apply_tint(x, y);
        }
        // The offset is a pixel nudge on the tile the write above just landed, not part of
        // `write_grapheme`'s contract (it has no offset parameter): set it directly via
        // `tile_mut` rather than widening `Grid`'s public write API for a `Surface`-only concern.
        if let Some(tile) = self.grid.tile_mut(self.layer, (x, y)) {
            tile.dx = offset.dx;
            tile.dy = offset.dy;
        }
    }

    /// Clears this surface's own area, intersected with its clip (on its own layer), back to
    /// [`Tile::default`].
    pub fn clear(&mut self) {
        let region = self.area.intersect(self.clip);
        self.grid.fill_region(self.layer, region, Tile::default());
    }

    /// Clears `rect` (clipped to this surface's own clip, on its own layer) back to
    /// [`Tile::default`].
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{Grid, Pos, Rect, Style, Surface};
    ///
    /// let mut grid = Grid::new(4, 4);
    /// let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 4, 4), 0);
    /// surface.fill_rect(Rect::new(0, 0, 4, 4), '#', Style::default());
    ///
    /// // `rect` extends past the surface's own clip; only the overlap is cleared.
    /// surface.clear_region(Rect::new(2, 2, 10, 10));
    ///
    /// assert_eq!(grid[Pos::new(2, 2)].glyph(), ' ');
    /// assert_eq!(grid[Pos::new(1, 1)].glyph(), '#');
    /// ```
    pub fn clear_region(&mut self, rect: Rect) {
        if let Some(abs) = self.local_rect_to_absolute(rect) {
            self.grid.fill_region(self.layer, abs, Tile::default());
            return;
        }

        for y in rect.top()..rect.bottom() {
            for x in rect.left()..rect.right() {
                if let Some((x, y)) = self.shift(x, y) {
                    self.grid.put_tile(self.layer, (x, y), Tile::default());
                }
            }
        }
    }
}

/// A [`Surface`] with a [`Style`] bound in, returned by [`Surface::with_style`].
///
/// Every draw call omits the `style` argument the underlying [`Surface`] method would otherwise
/// need, using the bound style instead. Reach back to the underlying surface (e.g. to call
/// [`Surface::print_line`], whose per-span styles make a bound style meaningless) via
/// [`StyledSurface::surface`].
pub struct StyledSurface<'s, 'a> {
    surface: &'s mut Surface<'a>,
    style: Style,
}

impl<'a> StyledSurface<'_, 'a> {
    /// The style every draw call on this view uses.
    #[must_use]
    pub const fn style(&self) -> Style {
        self.style
    }

    /// Borrows the underlying [`Surface`] directly, for calls that need an explicit style (e.g.
    /// [`Surface::print_line`]) or a capability [`StyledSurface`] doesn't expose.
    pub const fn surface(&mut self) -> &mut Surface<'a> {
        self.surface
    }

    /// [`Surface::put`] using this view's bound style.
    pub fn put(&mut self, pos: impl Into<Pos>, ch: char) {
        self.surface.put(pos, ch, self.style);
    }

    /// [`Surface::print`] using this view's bound style.
    pub fn print(&mut self, pos: impl Into<Pos>, text: &str) {
        self.surface.print(pos, text, self.style);
    }

    /// [`Surface::fill_rect`] using this view's bound style.
    pub fn fill_rect(&mut self, rect: Rect, ch: char) {
        self.surface.fill_rect(rect, ch, self.style);
    }

    /// [`Surface::put_span`] using this view's bound style.
    pub fn put_span<S: AsRef<str>>(&mut self, pos: impl Into<Pos>, rows: &[S]) -> Option<()> {
        self.surface.put_span(pos, rows, self.style)
    }

    /// [`Surface::put_span_uniform`] using this view's bound style.
    pub fn put_span_uniform(
        &mut self,
        pos: impl Into<Pos>,
        size: impl Into<Size>,
        anchor: char,
        fill: char,
    ) -> Option<()> {
        self.surface
            .put_span_uniform(pos, size, anchor, fill, self.style)
    }

    /// [`Surface::put_offset`] using this view's bound style.
    pub fn put_offset(&mut self, pos: impl Into<Pos>, offset: impl Into<Offset>, ch: char) {
        self.surface.put_offset(pos, offset, ch, self.style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(grid: &mut Grid) -> Surface<'_> {
        let area = Rect::new(0, 0, grid.width(), grid.height());
        Surface::new(grid, area, 0)
    }

    /// Renders a `'w' x 'h'` block of `ch` at this surface's own local top-left corner, i.e.
    /// the way a widget written against `local_area()`/`width()`/`height()` (rather than
    /// `area()`'s absolute `left()`/`top()`) is supposed to place itself: correctly regardless
    /// of where this surface's own `area` sits on the underlying grid.
    fn render_local_top_left(surface: &mut Surface<'_>, ch: char) {
        let local = surface.local_area();
        surface.put((local.left(), local.top()), ch, Style::default());
    }

    #[test]
    fn local_area_is_always_zero_origin_regardless_of_where_area_sits() {
        let mut grid = Grid::new(10, 10);
        let mut surface = screen(&mut grid);
        let scoped = surface.scope(Rect::new(3, 4, 5, 6));

        assert_eq!(scoped.area(), Rect::new(3, 4, 5, 6));
        assert_eq!(scoped.local_area(), Rect::new(0, 0, 5, 6));
    }

    #[test]
    fn a_widget_placed_via_local_area_lands_the_same_way_at_the_origin_and_at_an_offset() {
        // At the grid origin, local and absolute coordinates coincide: this is the case #697
        // warned degenerates into never exercising the mismatch.
        let mut grid_origin = Grid::new(4, 4);
        render_local_top_left(&mut screen(&mut grid_origin), 'X');

        // Scoped away from the origin: a widget built on `local_area()`/`put`'s local coordinate
        // space should draw identically relative to its own area, unlike one built on
        // `area().left()`/`area().top()`, which would silently miss here.
        let mut grid_offset = Grid::new(10, 10);
        {
            let mut surface = screen(&mut grid_offset);
            let mut scoped = surface.scope(Rect::new(3, 3, 4, 4));
            render_local_top_left(&mut scoped, 'X');
        }

        assert_eq!(grid_origin[Pos::new(0, 0)].glyph(), 'X');
        assert_eq!(grid_offset[Pos::new(3, 3)].glyph(), 'X');
    }

    #[test]
    fn blit_reads_the_sources_layer_0_even_when_this_surface_is_on_a_different_layer() {
        // The retroglyph#824 regression: `src` (a standalone, layer-0-only `Grid`, like
        // `BoxStyle::render`'s output) must still land when the destination surface is on a
        // layer other than 0.
        let mut src = Grid::new(2, 2);
        src.put_tile(0, (0, 0), Tile::new('x', Style::default()));

        let mut dst = Grid::new(4, 4);
        let mut surface = Surface::new(&mut dst, Rect::new(0, 0, 4, 4), 0);
        surface.on_layer(3).blit(&src, 1, 1);

        assert_eq!(dst.tile(3, (1, 1)).map(Tile::glyph), Some('x'));
        // Never touched layer 0 (always allocated, but untouched cells stay their default).
        assert_eq!(dst.tile(0, (1, 1)).map(Tile::glyph), Some(' '));
    }

    #[test]
    fn blit_stamps_a_grid_at_a_local_offset() {
        let mut src = Grid::new(2, 2);
        src.put_tile(0, (0, 0), Tile::new('x', Style::default()));
        src.put_tile(0, (1, 1), Tile::new('y', Style::default()));

        let mut dst = Grid::new(5, 5);
        screen(&mut dst).blit(&src, 2, 1);

        assert_eq!(dst[Pos::new(2, 1)].glyph(), 'x');
        assert_eq!(dst[Pos::new(3, 2)].glyph(), 'y');
        // Untouched cells stay whatever the destination started with.
        assert_eq!(dst[Pos::new(0, 0)].glyph(), ' ');
    }

    #[test]
    fn blit_clips_to_this_surfaces_clip_instead_of_skipping_the_whole_call() {
        let mut src = Grid::new(3, 3);
        for y in 0..3 {
            for x in 0..3 {
                src.put_tile(0, (x, y), Tile::new('#', Style::default()));
            }
        }

        let mut dst = Grid::new(5, 5);
        {
            let mut surface = screen(&mut dst);
            // Clip to a 2x2 window starting at (1, 1): only the top-left quadrant of `src`'s
            // footprint at (0, 0) is visible.
            surface.clip(Rect::new(1, 1, 2, 2)).blit(&src, 0, 0);
        }

        assert_eq!(dst[Pos::new(1, 1)].glyph(), '#');
        assert_eq!(dst[Pos::new(2, 2)].glyph(), '#');
        // Outside the clip: never written, even though it's inside `src`'s own footprint.
        assert_eq!(dst[Pos::new(2, 0)].glyph(), ' ');
        assert_eq!(dst[Pos::new(0, 2)].glyph(), ' ');
    }

    #[test]
    fn blit_is_a_no_op_for_a_zero_sized_grid() {
        let src = Grid::new(1, 0);
        let mut dst = Grid::new(4, 4);
        screen(&mut dst).blit(&src, 0, 0);

        assert_eq!(dst[Pos::new(0, 0)].glyph(), ' ');
    }

    #[test]
    fn blit_is_a_no_op_when_the_whole_footprint_is_cropped_before_the_translated_origin() {
        let mut src = Grid::new(2, 2);
        src.put_tile(0, (0, 0), Tile::new('x', Style::default()));

        let mut dst = Grid::new(4, 4);
        {
            let mut surface = screen(&mut dst);
            // Translating by +100 columns shifts `(0, 0)` to local `-100`: the whole 2-wide
            // footprint falls left of the origin, so `crop_left` (100) reaches past `w` (2).
            let mut view = surface.translate((100, 0));
            view.blit(&src, 0, 0);
        }

        assert_eq!(dst[Pos::new(0, 0)].glyph(), ' ');
    }

    #[test]
    fn blit_is_a_no_op_when_the_shifted_x_origin_overflows_u16() {
        let mut src = Grid::new(2, 2);
        src.put_tile(0, (0, 0), Tile::new('x', Style::default()));

        let mut dst = Grid::new(4, 4);
        {
            let mut surface = screen(&mut dst);
            // Translating by -100_000 pushes the shifted local x past `u16::MAX`, which
            // `u16::try_from` refuses (distinct from the ordinary negative-crop path above).
            let mut view = surface.translate((-100_000, 0));
            view.blit(&src, 0, 0);
        }

        assert_eq!(dst[Pos::new(0, 0)].glyph(), ' ');
    }

    #[test]
    fn blit_is_a_no_op_when_the_shifted_y_origin_overflows_u16() {
        let mut src = Grid::new(2, 2);
        src.put_tile(0, (0, 0), Tile::new('x', Style::default()));

        let mut dst = Grid::new(4, 4);
        {
            let mut surface = screen(&mut dst);
            // Same as the x-overflow case above, but on the y axis, which `blit` checks
            // separately (only after the x shift already succeeded).
            let mut view = surface.translate((0, -100_000));
            view.blit(&src, 0, 0);
        }

        assert_eq!(dst[Pos::new(0, 0)].glyph(), ' ');
    }

    #[test]
    fn blit_is_a_no_op_when_the_destination_footprint_misses_the_clip_entirely() {
        let mut src = Grid::new(2, 2);
        src.put_tile(0, (0, 0), Tile::new('x', Style::default()));

        let mut dst = Grid::new(5, 5);
        {
            let mut surface = screen(&mut dst);
            // Clipped to the top-left cell only; the blit lands at (3, 3), whose footprint
            // doesn't overlap the clip at all (unlike the partial-overlap case tested above).
            let mut clipped = surface.clip(Rect::new(0, 0, 1, 1));
            clipped.blit(&src, 3, 3);
        }

        assert_eq!(dst[Pos::new(3, 3)].glyph(), ' ');
    }

    #[test]
    fn put_span_takes_any_as_ref_str_row() {
        let mut grid = Grid::new(4, 4);
        // A footprint computed at runtime: owned rows, no borrowing pass over them.
        let rows: Vec<String> = (0..2)
            .map(|row| {
                (0..2)
                    .map(|col| if (row, col) == (0, 0) { 'C' } else { ' ' })
                    .collect()
            })
            .collect();

        assert_eq!(
            screen(&mut grid).put_span((0, 0), &rows, Style::default()),
            Some(())
        );
        assert_eq!(grid[Pos::new(0, 0)].span(), (2, 2));
    }

    #[test]
    fn put_span_reports_why_a_span_did_not_draw() {
        let mut grid = Grid::new(4, 4);
        let area = Rect::new(0, 0, 2, 2);
        let mut surface = Surface::new(&mut grid, area, 0);
        let style = Style::default();

        assert_eq!(surface.put_span((0, 0), &[] as &[&str], style), None);
        assert_eq!(surface.put_span((0, 0), &[""], style), None);
        // Ragged rows are refused by the grid, and that answer is passed through.
        assert_eq!(surface.put_span((0, 0), &["ab", "c"], style), None);
        // Fits the grid, but leaves the surface's own area.
        assert_eq!(surface.put_span((1, 1), &["ab"], style), None);
        assert_eq!(surface.put_span((0, 0), &["ab"], style), Some(()));
    }

    #[test]
    fn put_span_uniform_writes_the_anchor_once_and_fills_the_rest() {
        let mut grid = Grid::new(4, 4);
        assert_eq!(
            screen(&mut grid).put_span_uniform((1, 1), (2, 2), 'C', '.', Style::default()),
            Some(())
        );

        assert_eq!(grid[Pos::new(1, 1)].glyph(), 'C');
        assert_eq!(grid[Pos::new(1, 1)].span(), (2, 2));
        assert_eq!(grid[Pos::new(2, 2)].glyph(), '.');
        assert_eq!(grid.span_owner(0, 2, 2), Some(Pos::new(1, 1)));
    }

    #[test]
    fn put_span_uniform_writes_to_this_surfaces_layer() {
        let mut grid = Grid::new(4, 4);
        {
            let mut surface = screen(&mut grid);
            surface
                .on_layer(2)
                .put_span_uniform((0, 0), (2, 1), 'C', ' ', Style::default())
                .expect("span write");
        }

        assert_eq!(grid.span_owner(2, 1, 0), Some(Pos::new(0, 0)));
        assert_eq!(grid.span_owner(0, 1, 0), None);
    }

    #[test]
    fn put_span_uniform_refuses_a_footprint_that_leaves_the_surfaces_area() {
        let mut grid = Grid::new(4, 4);
        let area = Rect::new(0, 0, 2, 2);
        let mut surface = Surface::new(&mut grid, area, 0);
        let style = Style::default();

        // Both fit the grid; neither fits the area.
        assert_eq!(
            surface.put_span_uniform((1, 0), (2, 1), 'C', ' ', style),
            None
        );
        assert_eq!(
            surface.put_span_uniform((0, 1), (1, 2), 'C', ' ', style),
            None
        );
        assert_eq!(
            surface.put_span_uniform((0, 0), (0, 1), 'C', ' ', style),
            None
        );
        assert_eq!(
            surface.put_span_uniform((0, 0), (2, 2), 'C', ' ', style),
            Some(())
        );
    }

    #[test]
    fn styled_surface_forwards_both_span_calls() {
        let mut grid = Grid::new(4, 4);
        {
            let mut surface = screen(&mut grid);
            let mut styled = surface.with_style(Style::new().fg(Color::RED));
            styled.put_span((0, 0), &["ab"]).expect("span write");
            styled
                .put_span_uniform((0, 1), (2, 1), 'C', ' ')
                .expect("span write");
        }

        assert_eq!(grid[Pos::new(0, 0)].style().foreground(), Color::RED);
        assert_eq!(grid[Pos::new(0, 1)].style().foreground(), Color::RED);
        assert_eq!(grid[Pos::new(0, 1)].span(), (2, 1));
    }

    #[test]
    fn with_tint_applies_to_the_cell_it_writes() {
        let mut grid = Grid::new(4, 4);
        {
            let mut surface = screen(&mut grid);
            surface
                .with_tint(Tint::multiply(128, 64, 32))
                .put((1, 1), '@', Style::default());
        }

        assert_eq!(grid[Pos::new(1, 1)].glyph(), '@');
        assert_eq!(grid.tint(0, 1, 1), Tint::multiply(128, 64, 32));
    }

    #[test]
    fn an_untinted_surface_leaves_the_side_table_alone() {
        let mut grid = Grid::new(4, 4);
        screen(&mut grid).put((1, 1), '@', Style::default());

        assert_eq!(grid.tint(0, 1, 1), Tint::None);
    }

    /// `fill_rect`'s batch fast path only applies when the surface is untinted: a tinted surface
    /// must fall back to the per-cell loop, or every cell `fill_rect` touches would silently lose
    /// its tint.
    #[test]
    fn with_tint_applies_to_every_cell_fill_rect_touches() {
        let mut grid = Grid::new(4, 4);
        {
            let mut surface = screen(&mut grid);
            surface.with_tint(Tint::multiply(128, 64, 32)).fill_rect(
                Rect::new(1, 1, 2, 2),
                '#',
                Style::default(),
            );
        }

        for (x, y) in [(1, 1), (2, 1), (1, 2), (2, 2)] {
            assert_eq!(grid[Pos::new(x, y)].glyph(), '#');
            assert_eq!(grid.tint(0, x, y), Tint::multiply(128, 64, 32));
        }
    }

    /// `fill_rect`'s batch fast path only applies to a single-column glyph: a wide glyph must
    /// fall back to the same per-cell `put` loop used before this method had a fast path, not the
    /// batch `Tile::new` write (which carries no wide-char bookkeeping at all).
    #[test]
    #[cfg(feature = "egc")]
    fn fill_rect_with_a_wide_glyph_falls_back_to_the_put_loop() {
        let rect = Rect::new(0, 0, 6, 1);

        let mut via_fill_rect = Grid::new(8, 1);
        screen(&mut via_fill_rect).fill_rect(rect, '\u{6f22}', Style::default());

        let mut via_put = Grid::new(8, 1);
        {
            let mut surface = screen(&mut via_put);
            for y in rect.top()..rect.bottom() {
                for x in rect.left()..rect.right() {
                    surface.put((x, y), '\u{6f22}', Style::default());
                }
            }
        }

        for x in 0..8 {
            assert_eq!(
                via_fill_rect[Pos::new(x, 0)],
                via_put[Pos::new(x, 0)],
                "cell ({x}, 0)"
            );
        }
    }

    /// `fill_rect`, `clear`, and `clear_region` all route through `Grid::fill_region`, which must
    /// clear any span the fill partially overwrites the same way the per-cell loop it replaced
    /// did, or the surviving span's anchor would keep claiming cells the fill just overwrote.
    #[test]
    fn fill_rect_clears_a_span_it_partially_overwrites() {
        use crate::tile::TileFlags;

        let mut grid = Grid::new(4, 4);
        {
            let mut surface = screen(&mut grid);
            surface
                .put_span((0, 0), &["C=", "[]"], Style::default())
                .expect("2x2 span fits in a 4x4 grid");
            surface.fill_rect(Rect::new(1, 0, 3, 3), '#', Style::default());
        }

        assert!(
            !grid
                .tile(0, (0, 0))
                .unwrap()
                .flags()
                .contains(TileFlags::SPAN_ANCHOR)
        );
    }

    #[test]
    fn clear_region_clears_a_span_it_partially_overwrites() {
        use crate::tile::TileFlags;

        let mut grid = Grid::new(4, 4);
        {
            let mut surface = screen(&mut grid);
            surface
                .put_span((0, 0), &["C=", "[]"], Style::default())
                .expect("2x2 span fits in a 4x4 grid");
            surface.clear_region(Rect::new(1, 0, 3, 3));
        }

        assert!(
            !grid
                .tile(0, (0, 0))
                .unwrap()
                .flags()
                .contains(TileFlags::SPAN_ANCHOR)
        );
    }

    #[test]
    fn with_tint_lands_on_the_span_anchor_only() {
        let mut grid = Grid::new(4, 4);
        {
            let mut surface = screen(&mut grid);
            surface
                .with_tint(Tint::multiply(200, 200, 200))
                .put_span((0, 0), &["ab", "cd"], Style::default())
                .expect("span write");
        }

        // A pixel backend draws the whole footprint from the anchor, so that is the only cell
        // with a sprite to recolour.
        assert_eq!(grid.tint(0, 0, 0), Tint::multiply(200, 200, 200));
        assert_eq!(grid.tint(0, 1, 0), Tint::None);
        assert_eq!(grid.tint(0, 1, 1), Tint::None);
    }

    #[test]
    fn with_tint_applies_to_a_uniform_span_anchor() {
        let mut grid = Grid::new(4, 4);
        {
            let mut surface = screen(&mut grid);
            surface
                .with_tint(Tint::mix(255, 0, 0, 128))
                .put_span_uniform((1, 1), (2, 2), 'C', '.', Style::default())
                .expect("span write");
        }

        assert_eq!(grid.tint(0, 1, 1), Tint::mix(255, 0, 0, 128));
        assert_eq!(grid.tint(0, 2, 2), Tint::None);
    }

    #[test]
    fn with_tint_is_not_applied_to_a_refused_span() {
        let mut grid = Grid::new(4, 4);
        let area = Rect::new(0, 0, 2, 2);
        {
            let mut surface = Surface::new(&mut grid, area, 0);
            // Fits the grid, leaves the area: nothing is written, so nothing is tinted.
            assert_eq!(
                surface.with_tint(Tint::multiply(1, 2, 3)).put_span(
                    (1, 1),
                    &["ab"],
                    Style::default()
                ),
                None
            );
        }

        assert_eq!(grid.tint(0, 1, 1), Tint::None);
    }

    #[test]
    fn with_tint_survives_clip_and_on_layer() {
        let mut grid = Grid::new(8, 4);
        {
            let mut surface = screen(&mut grid);
            let mut tinted = surface.with_tint(Tint::multiply(9, 9, 9));
            assert_eq!(tinted.tint(), Tint::multiply(9, 9, 9));
            assert_eq!(
                tinted.clip(Rect::new(0, 0, 4, 4)).tint(),
                Tint::multiply(9, 9, 9)
            );
            assert_eq!(tinted.on_layer(2).tint(), Tint::multiply(9, 9, 9));

            tinted.on_layer(2).put((1, 1), '@', Style::default());
        }

        assert_eq!(grid.tint(2, 1, 1), Tint::multiply(9, 9, 9));
    }

    #[test]
    fn with_tint_replaces_rather_than_composes() {
        let mut grid = Grid::new(4, 4);
        {
            let mut surface = screen(&mut grid);
            let mut outer = surface.with_tint(Tint::multiply(128, 128, 128));
            // Unlike `clip`, a nested tint substitutes: two tints have no meaningful product.
            outer
                .with_tint(Tint::mix(255, 0, 0, 64))
                .put((0, 0), '@', Style::default());
        }

        assert_eq!(grid.tint(0, 0, 0), Tint::mix(255, 0, 0, 64));
    }

    #[test]
    fn clip_narrows_the_visible_rect_but_not_the_area() {
        let mut grid = Grid::new(8, 4);
        let area = Rect::new(0, 0, 8, 4);
        let mut surface = screen(&mut grid);
        let sub = surface.clip(Rect::new(2, 1, 4, 2));

        assert_eq!(sub.clip_rect(), Rect::new(2, 1, 4, 2));
        // `area` reports what the surface represents, unaffected by `clip`.
        assert_eq!(sub.area(), area);
        assert_eq!(sub.width(), 8);
        assert_eq!(sub.height(), 4);
    }

    #[test]
    fn clip_keeps_the_layer() {
        let mut grid = Grid::new(4, 4);
        let mut surface = screen(&mut grid);
        let mut layer1 = surface.on_layer(1);

        assert_eq!(layer1.clip(Rect::new(0, 0, 2, 2)).layer(), 1);
    }

    #[test]
    fn layer_variants_order_low_to_high() {
        assert!(Layer::World < Layer::Hud);
        assert!(Layer::Hud < Layer::Overlay);
        assert!(Layer::Overlay < Layer::Debug);
    }

    #[test]
    fn layer_as_u8_matches_the_documented_grid_layer_ids() {
        assert_eq!(Layer::World.as_u8(), 0);
        assert_eq!(Layer::Hud.as_u8(), 1);
        assert_eq!(Layer::Overlay.as_u8(), 2);
        assert_eq!(Layer::Debug.as_u8(), 3);
    }

    #[test]
    fn layer_default_is_world() {
        assert_eq!(Layer::default(), Layer::World);
    }

    #[test]
    fn layer_into_u8_matches_as_u8() {
        assert_eq!(u8::from(Layer::Overlay), Layer::Overlay.as_u8());
    }

    #[test]
    fn on_tier_matches_on_layer_with_the_tiers_u8() {
        let mut grid = Grid::new(4, 4);
        let mut surface = screen(&mut grid);

        assert_eq!(surface.on_tier(Layer::Overlay).layer(), 2);
        assert_eq!(
            surface.on_layer(2).layer(),
            surface.on_tier(Layer::Overlay).layer()
        );
    }

    #[test]
    fn on_tier_writes_land_on_the_tiers_grid_layer() {
        let mut grid = Grid::new(4, 4);
        {
            let mut surface = screen(&mut grid);
            surface
                .on_tier(Layer::Overlay)
                .put((0, 0), '@', Style::default());
        }

        assert_eq!(grid.tile(2, Pos::new(0, 0)).map(Tile::glyph), Some('@'));
        // Untouched on lower tiers: `on_tier` switches layers, it doesn't also write there.
        // Layer 0 is always allocated (empty), layer 1 was never written so stays unallocated.
        assert_eq!(grid.tile(0, Pos::new(0, 0)).map(Tile::glyph), Some(' '));
        assert_eq!(grid.tile(1, Pos::new(0, 0)), None);
    }

    #[test]
    #[cfg(feature = "egc")]
    fn a_wide_char_at_the_clip_edge_writes_its_spacer_outside_the_clip() {
        let mut grid = Grid::new(8, 1);
        let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 8, 1), 0);

        // Clip is columns 0..4; the wide char's primary cell (column 3) is inside the clip, but
        // its spacer would land at column 4, outside it. The whole write is refused.
        surface
            .clip(Rect::new(0, 0, 4, 1))
            .put((3, 0), '\u{6f22}', Style::default());

        assert_eq!(grid.tile(0, Pos::new(3, 0)).map(Tile::glyph), Some(' '));
        assert!(
            !grid
                .tile(0, Pos::new(4, 0))
                .is_some_and(|t| t.flags().contains(crate::tile::TileFlags::WIDE_CHAR_SPACER)),
            "spacer must not be written outside the clip"
        );
    }

    #[test]
    fn clip_intersects_rather_than_replaces_so_it_cannot_widen() {
        let mut grid = Grid::new(8, 4);
        let area = Rect::new(2, 1, 4, 2);
        let mut surface = Surface::new(&mut grid, area, 0);

        // A rect reaching outside the surface's own clip only ever tightens it.
        assert_eq!(surface.clip(Rect::new(0, 0, 8, 4)).clip_rect(), area);
        assert_eq!(
            surface.clip(Rect::new(0, 0, 4, 4)).clip_rect(),
            Rect::new(2, 1, 2, 2)
        );
    }

    #[test]
    fn clip_does_not_widen_the_visible_region_after_scope_narrows_it() {
        let mut grid = Grid::new(8, 4);
        let mut surface = screen(&mut grid);
        let mut scoped = surface.scope(Rect::new(1, 1, 2, 2));

        // `clip` on an already-scoped surface can only tighten what was already visible, even
        // when asked for a rect that reaches back out past it.
        assert_eq!(
            scoped.clip(Rect::new(0, 0, 8, 4)).clip_rect(),
            Rect::new(1, 1, 2, 2)
        );
    }

    #[test]
    fn scope_sets_the_area_and_intersects_the_clip() {
        let mut grid = Grid::new(8, 4);
        let mut surface = screen(&mut grid);
        let mut clipped = surface.clip(Rect::new(0, 0, 4, 4));

        // `scope` widens `area` to a rect the parent's clip does not fully cover...
        let scoped = clipped.scope(Rect::new(2, 0, 4, 4));
        assert_eq!(scoped.area(), Rect::new(2, 0, 4, 4));
        // ...but the visible region still cannot exceed the parent's own clip.
        assert_eq!(scoped.clip_rect(), Rect::new(2, 0, 2, 4));
    }

    #[test]
    fn scope_inside_a_clipped_surface_cannot_widen_the_visible_region() {
        let mut grid = Grid::new(8, 4);
        let mut surface = screen(&mut grid);
        let mut clipped = surface.clip(Rect::new(2, 1, 2, 2));

        // Even a `scope` rect that reaches well outside the parent's clip only ever tightens
        // what is visible; `area` still becomes exactly the requested rect.
        let scoped = clipped.scope(Rect::new(0, 0, 8, 4));
        assert_eq!(scoped.area(), Rect::new(0, 0, 8, 4));
        assert_eq!(scoped.clip_rect(), Rect::new(2, 1, 2, 2));
    }

    #[test]
    fn scope_writes_outside_the_inherited_clip_are_dropped() {
        let mut grid = Grid::new(8, 4);
        {
            let mut surface = screen(&mut grid);
            let mut clipped = surface.clip(Rect::new(0, 0, 4, 4));
            let mut scoped = clipped.scope(Rect::new(0, 0, 8, 4));
            // Inside the requested `area`, outside the inherited clip.
            scoped.put((5, 0), 'a', Style::default());
            scoped.put((1, 0), 'b', Style::default());
        }

        assert_eq!(grid[Pos::new(5, 0)].glyph(), ' ');
        assert_eq!(grid[Pos::new(1, 0)].glyph(), 'b');
    }

    #[test]
    fn clip_writes_outside_the_sub_rect_are_dropped() {
        let mut grid = Grid::new(4, 2);
        {
            let mut surface = screen(&mut grid);
            let mut top = surface.clip(Rect::new(0, 0, 4, 1));
            top.put((1, 0), 'a', Style::default());
            // Inside the surface's own area, outside the clip.
            top.put((1, 1), 'b', Style::default());
        }

        assert_eq!(grid[Pos::new(1, 0)].glyph(), 'a');
        assert_eq!(grid[Pos::new(1, 1)].glyph(), ' ');
    }

    #[test]
    fn clip_to_one_row_drops_print_overflow_instead_of_wrapping_it() {
        let mut grid = Grid::new(4, 2);
        {
            let mut surface = screen(&mut grid);
            surface
                .clip(Rect::new(0, 0, 4, 1))
                .print((0, 0), "abcdef", Style::default());
        }

        assert_eq!(grid[Pos::new(3, 0)].glyph(), 'd');
        // "ef" wrapped onto row 1, which the clip excludes.
        assert_eq!(grid[Pos::new(0, 1)].glyph(), ' ');
    }

    #[test]
    fn print_wraps_at_the_surfaces_own_width_not_at_clip_right() {
        // `area` starts at column 4, so the surface-local wrap column (4) is smaller than the
        // absolute grid column `clip.right()` resolves to (8). Wrapping must use the former.
        let mut grid = Grid::new(8, 2);
        let mut surface = Surface::new(&mut grid, Rect::new(4, 0, 4, 2), 0);
        surface.print((0, 0), "abcdef", Style::default());

        assert_eq!(grid[Pos::new(4, 0)].glyph(), 'a');
        assert_eq!(grid[Pos::new(7, 0)].glyph(), 'd');
        assert_eq!(grid[Pos::new(4, 1)].glyph(), 'e');
        assert_eq!(grid[Pos::new(5, 1)].glyph(), 'f');
    }

    #[test]
    fn print_line_measures_its_span_skip_threshold_against_the_surfaces_own_width() {
        // `area` starts at column 4, so the surface-local skip column (4) is smaller than the
        // absolute grid column `clip.right()` resolves to (8). The second span starts at local
        // column 3, which is inside the 4-wide area, so it must still print.
        use crate::text::Span;

        let mut grid = Grid::new(8, 1);
        let mut surface = Surface::new(&mut grid, Rect::new(4, 0, 4, 1), 0);
        let line = Line::from(vec![Span::raw("ab"), Span::raw("cd")]);
        surface.print_line((0, 0), &line);

        assert_eq!(grid[Pos::new(4, 0)].glyph(), 'a');
        assert_eq!(grid[Pos::new(5, 0)].glyph(), 'b');
        assert_eq!(grid[Pos::new(6, 0)].glyph(), 'c');
        assert_eq!(grid[Pos::new(7, 0)].glyph(), 'd');
    }

    #[test]
    fn clip_makes_put_span_measure_its_footprint_against_the_sub_rect() {
        let mut grid = Grid::new(4, 3);
        {
            let mut surface = screen(&mut grid);
            // Fits the grid, but reserves a cell on the bottom row the clip excludes.
            surface
                .clip(Rect::new(0, 0, 4, 2))
                .put_span((0, 1), &["ab", "cd"], Style::default());
        }

        assert_eq!(grid[Pos::new(0, 1)].glyph(), ' ');

        let mut surface = screen(&mut grid);
        surface
            .clip(Rect::new(0, 0, 4, 2))
            .put_span((0, 0), &["ab", "cd"], Style::default());

        assert_eq!(grid[Pos::new(0, 0)].span(), (2, 2));
    }

    #[test]
    fn clip_makes_put_span_uniform_measure_its_footprint_against_the_sub_rect() {
        let mut grid = Grid::new(4, 3);
        let style = Style::default();
        {
            let mut surface = screen(&mut grid);
            let mut content = surface.clip(Rect::new(0, 0, 4, 2));
            // Fits the grid, but reserves a cell on the bottom row the clip excludes.
            assert_eq!(
                content.put_span_uniform((0, 1), (2, 2), 'C', '.', style),
                None
            );
            assert_eq!(
                content.put_span_uniform((0, 0), (2, 2), 'C', '.', style),
                Some(())
            );
        }

        assert_eq!(grid[Pos::new(0, 0)].span(), (2, 2));
        assert_eq!(grid[Pos::new(0, 2)].glyph(), ' ');
    }

    #[test]
    fn clip_to_a_disjoint_rect_is_empty_and_drops_every_write() {
        let mut grid = Grid::new(8, 4);
        {
            let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 4, 4), 0);
            let mut sub = surface.clip(Rect::new(4, 0, 4, 4));
            assert_eq!(sub.clip_rect(), Rect::EMPTY);
            sub.print((0, 0), "abc", Style::default());
        }

        assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
    }

    #[test]
    fn put_signed_drops_a_negative_coordinate() {
        let mut grid = Grid::new(4, 4);
        let mut surface = screen(&mut grid);

        surface.put_signed((-1, 0), 'X', Style::default());
        surface.put_signed((0, -1), 'X', Style::default());

        assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
    }

    #[test]
    fn put_signed_lands_a_valid_coordinate_at_the_area_origin() {
        let mut grid = Grid::new(4, 4);
        let area = Rect::new(1, 1, 2, 2);
        let mut surface = Surface::new(&mut grid, area, 0);

        // (0, 0) relative to the area's own origin is grid position (1, 1).
        surface.put_signed((0, 0), 'X', Style::default());

        assert_eq!(grid[Pos::new(1, 1)].glyph(), 'X');
    }

    #[test]
    fn put_signed_drops_a_coordinate_past_this_surfaces_width_or_height() {
        let mut grid = Grid::new(4, 4);
        let area = Rect::new(0, 0, 2, 2);
        let mut surface = Surface::new(&mut grid, area, 0);

        // Fits the grid, but not this surface's own (relative) width/height.
        surface.put_signed((2, 0), 'X', Style::default());
        surface.put_signed((0, 2), 'X', Style::default());

        assert_eq!(grid[Pos::new(2, 0)].glyph(), ' ');
        assert_eq!(grid[Pos::new(0, 2)].glyph(), ' ');
    }

    #[test]
    #[cfg(feature = "egc")]
    fn put_signed_does_wide_char_bookkeeping_like_put() {
        use crate::tile::TileFlags;

        let mut grid = Grid::new(8, 2);
        {
            let mut surface = screen(&mut grid);

            // A wide char via `put`: primary at (0, 0), spacer at (1, 0).
            surface.put((0, 0), '\u{6f22}', Style::default());
            // Overwriting the primary cell via `put_signed` must clear the orphaned spacer too.
            surface.put_signed((0, 0), 'a', Style::default());
        }
        assert!(
            !grid[Pos::new(1, 0)]
                .flags()
                .contains(TileFlags::WIDE_CHAR_SPACER)
        );

        // Writing a wide char via `put_signed` must set the flags and spacer `put` would.
        screen(&mut grid).put_signed((3, 0), '\u{6f22}', Style::default());
        assert!(grid[Pos::new(3, 0)].flags().contains(TileFlags::WIDE_CHAR));
        assert!(
            grid[Pos::new(4, 0)]
                .flags()
                .contains(TileFlags::WIDE_CHAR_SPACER)
        );
    }

    #[test]
    fn put_offset_applies_the_surfaces_tint() {
        let mut grid = Grid::new(4, 4);
        {
            let mut surface = screen(&mut grid);
            surface.with_tint(Tint::multiply(128, 64, 32)).put_offset(
                (1, 1),
                Offset::new(2, -2),
                'X',
                Style::default(),
            );
        }

        assert_eq!(grid.tint(0, 1, 1), Tint::multiply(128, 64, 32));
    }

    #[test]
    fn put_offset_still_carries_the_pixel_offset() {
        let mut grid = Grid::new(4, 4);
        let mut surface = screen(&mut grid);

        surface.put_offset((1, 1), Offset::new(2, -2), 'X', Style::default());

        let tile = grid.tile(0, Pos::new(1, 1)).unwrap();
        assert_eq!((tile.dx(), tile.dy()), (2, -2));
    }

    #[test]
    fn translate_does_not_change_area_width_or_height() {
        let mut grid = Grid::new(10, 10);
        let mut surface = screen(&mut grid);
        let mut scoped = surface.scope(Rect::new(5, 5, 4, 4));
        let view = scoped.translate((-5, -5));

        assert_eq!(view.area(), Rect::new(5, 5, 4, 4));
        assert_eq!(view.clip_rect(), Rect::new(5, 5, 4, 4));
        assert_eq!(view.width(), 4);
        assert_eq!(view.height(), 4);
    }

    #[test]
    fn translate_composes_with_scope_and_clip_rect() {
        let mut grid = Grid::new(10, 10);
        let mut surface = screen(&mut grid);
        let mut clipped = surface.clip(Rect::new(0, 0, 6, 6));
        // `scope` widens `area` past the parent's clip; `translate` on top must not change
        // either `area` or the still-narrower `clip_rect` it composed with.
        let mut scoped = clipped.scope(Rect::new(4, 4, 4, 4));
        let view = scoped.translate((-4, -4));

        assert_eq!(view.area(), Rect::new(4, 4, 4, 4));
        assert_eq!(view.clip_rect(), Rect::new(4, 4, 2, 2));
    }

    #[test]
    fn translate_shifts_put_by_subtracting_the_origin() {
        let mut grid = Grid::new(10, 10);
        {
            let mut surface = screen(&mut grid);
            let mut view = surface.translate((3, 3));

            // (3, 3) minus the translate origin (3, 3) is (0, 0).
            view.put((3, 3), 'A', Style::default());
            // (2, 3) minus (3, 3) is negative on the x axis: out of bounds, dropped.
            view.put((2, 3), 'B', Style::default());
        }

        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'A');
        assert_eq!(grid[Pos::new(0, 3)].glyph(), ' ');
    }

    #[test]
    fn translate_composes_with_scope_and_lets_a_negative_signed_coordinate_land() {
        let mut grid = Grid::new(10, 10);
        {
            let mut surface = screen(&mut grid);
            let mut scoped = surface.scope(Rect::new(5, 5, 4, 4));
            let mut view = scoped.translate((-5, -5));

            // -5 minus the translate origin (-5) is 0: the viewport's own local origin, landing
            // at the scoped area's top-left grid cell.
            view.put_signed((-5, -5), 'X', Style::default());
            // -6 minus -5 is still -1: still negative, so still out of bounds.
            view.put_signed((-6, -6), 'Y', Style::default());
        }

        assert_eq!(grid[Pos::new(5, 5)].glyph(), 'X');
        assert_eq!(grid[Pos::new(4, 4)].glyph(), ' ');
    }

    #[test]
    fn translate_composes_with_clip_and_shifts_put_the_same_way_as_put_signed() {
        // A regression test for a `shift` bug: `clip`'s area does not start at the grid's own
        // `(0, 0)` (unlike every other `clip` + `translate` test above), and the translate origin
        // is not simply `-area.left()`, so `put` must re-derive the same absolute cell
        // `put_signed` already did rather than landing on the clipped area's raw absolute
        // coordinates.
        let mut grid = Grid::new(20, 20);
        {
            let mut surface = screen(&mut grid);
            let mut view = surface.clip_translate(Rect::new(5, 5, 10, 10), (45, 45));

            // (50, 50) minus the origin (45, 45) is (5, 5): the clipped area's local (5, 5),
            // landing at absolute grid (10, 10), not at (5, 5), which is what the pre-fix
            // `shift` incorrectly produced by using the clip's raw absolute bounds instead of
            // re-adding the area's own top-left.
            view.put((50, 50), '@', Style::default());
        }

        assert_eq!(grid[Pos::new(10, 10)].glyph(), '@');
        assert_eq!(grid[Pos::new(5, 5)].glyph(), ' ');
    }

    #[test]
    fn translate_composes_additively_across_two_calls() {
        let mut grid = Grid::new(10, 10);
        {
            let mut surface = screen(&mut grid);
            let mut once = surface.translate((2, 0));
            let mut twice = once.translate((1, 0));

            // Composed origin is (3, 0): (3, 0) minus (3, 0) is (0, 0).
            twice.put((3, 0), 'A', Style::default());
        }

        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'A');
    }

    #[test]
    fn translate_shifts_fill_rect_print_and_clear_region_via_put() {
        let mut grid = Grid::new(10, 10);
        {
            let mut surface = screen(&mut grid);
            let mut view = surface.translate((5, 5));
            view.fill_rect(Rect::new(5, 5, 2, 2), '#', Style::default());
            view.print((5, 6), "a", Style::default());
        }

        assert_eq!(grid[Pos::new(0, 0)].glyph(), '#');
        assert_eq!(grid[Pos::new(1, 1)].glyph(), '#');
        assert_eq!(grid[Pos::new(0, 1)].glyph(), 'a');
    }

    #[test]
    fn translate_shifts_clear_region() {
        let mut grid = Grid::new(10, 10);
        {
            let mut surface = screen(&mut grid);
            surface.fill_rect(Rect::new(0, 0, 4, 4), '#', Style::default());
            let mut view = surface.translate((2, 2));
            // Clears grid (0..2, 0..2) once shifted by the translate origin.
            view.clear_region(Rect::new(2, 2, 2, 2));
        }

        assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
        assert_eq!(grid[Pos::new(1, 1)].glyph(), ' ');
        assert_eq!(grid[Pos::new(2, 2)].glyph(), '#');
    }

    #[test]
    fn translate_shifts_put_span_and_put_span_uniform() {
        let mut grid = Grid::new(10, 10);
        {
            let mut surface = screen(&mut grid);
            let mut view = surface.translate((4, 4));
            assert_eq!(view.put_span((4, 4), &["ab"], Style::default()), Some(()));
            assert_eq!(
                view.put_span_uniform((6, 4), (2, 1), 'C', ' ', Style::default()),
                Some(())
            );
        }

        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'a');
        assert_eq!(grid[Pos::new(2, 0)].glyph(), 'C');
    }

    #[test]
    fn clear_is_unaffected_by_translate() {
        let mut grid = Grid::new(4, 4);
        {
            let mut surface = screen(&mut grid);
            surface.fill_rect(Rect::new(0, 0, 4, 4), '#', Style::default());
            let mut view = surface.translate((100, 100));
            // `clear` takes no coordinate, so the translate offset does not apply to it: it
            // always clears this surface's own area.
            view.clear();
        }

        assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
        assert_eq!(grid[Pos::new(3, 3)].glyph(), ' ');
    }

    #[test]
    fn grid_is_the_read_only_counterpart_of_grid_mut() {
        let mut grid = Grid::new(4, 4);
        let mut surface = screen(&mut grid);
        surface.put((1, 1), 'X', Style::default());

        assert_eq!(surface.grid()[Pos::new(1, 1)].glyph(), 'X');
    }

    #[test]
    fn tile_reads_a_written_cell_without_a_mutable_borrow() {
        let mut grid = Grid::new(4, 4);
        let mut surface = screen(&mut grid);
        surface.put((1, 1), 'X', Style::default());

        assert_eq!(surface.tile((1, 1)).map(Tile::glyph), Some('X'));
        assert_eq!(surface.tile((0, 0)).map(Tile::glyph), Some(' '));
        assert_eq!(surface.tile((10, 10)), None);
    }

    #[test]
    fn background_reads_the_styles_background_colour() {
        let mut grid = Grid::new(4, 4);
        let mut surface = screen(&mut grid);
        surface.put((1, 1), 'X', Style::new().bg(Color::RED));

        assert_eq!(surface.background((1, 1)), Some(Color::RED));
        assert_eq!(surface.background((10, 10)), None);
    }

    #[test]
    fn print_aligned_left_aligns_by_default() {
        let mut grid = Grid::new(8, 1);
        {
            let mut surface = screen(&mut grid);
            surface.print_aligned(
                Rect::new(0, 0, 8, 1),
                "hi",
                crate::align::HAlign::Left,
                Style::default(),
            );
        }

        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'h');
        assert_eq!(grid[Pos::new(1, 0)].glyph(), 'i');
        assert_eq!(grid[Pos::new(2, 0)].glyph(), ' ');
    }

    #[test]
    fn print_aligned_centers_matching_text_layouts_own_saturating_formula() {
        let mut grid = Grid::new(6, 1);
        {
            let mut surface = screen(&mut grid);
            surface.print_aligned(
                Rect::new(0, 0, 6, 1),
                "hi",
                crate::align::HAlign::Center,
                Style::default(),
            );
        }

        // (6 - 2) / 2 == 2 columns of left padding, matching `HAlign::Center` in `align.rs`.
        assert_eq!(grid[Pos::new(2, 0)].glyph(), 'h');
        assert_eq!(grid[Pos::new(3, 0)].glyph(), 'i');
    }

    #[test]
    fn print_aligned_right_aligns_flush_to_the_rects_right_edge() {
        let mut grid = Grid::new(6, 1);
        {
            let mut surface = screen(&mut grid);
            surface.print_aligned(
                Rect::new(0, 0, 6, 1),
                "hi",
                crate::align::HAlign::Right,
                Style::default(),
            );
        }

        assert_eq!(grid[Pos::new(4, 0)].glyph(), 'h');
        assert_eq!(grid[Pos::new(5, 0)].glyph(), 'i');
    }

    #[test]
    fn print_aligned_does_not_panic_or_underflow_on_text_wider_than_the_rect() {
        let mut grid = Grid::new(4, 1);
        {
            let mut surface = screen(&mut grid);
            // "hello" is wider than the 4-column rect on every alignment: this must not panic
            // (a plain `rect.width() - text_width` would underflow) and instead left-aligns and
            // lets `print` clip the overflow.
            surface.print_aligned(
                Rect::new(0, 0, 4, 1),
                "hello",
                crate::align::HAlign::Center,
                Style::default(),
            );
        }

        assert_eq!(grid[Pos::new(0, 0)].glyph(), 'h');
        assert_eq!(grid[Pos::new(3, 0)].glyph(), 'l');
    }

    #[test]
    fn print_aligned_clips_to_this_surfaces_own_area_as_well_as_rect() {
        let mut grid = Grid::new(4, 1);
        {
            let mut surface = Surface::new(&mut grid, Rect::new(0, 0, 2, 1), 0);
            // `rect` extends past this surface's own area; the write is still clipped to it.
            surface.print_aligned(
                Rect::new(0, 0, 4, 1),
                "hi",
                crate::align::HAlign::Right,
                Style::default(),
            );
        }

        assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
        assert_eq!(grid[Pos::new(1, 0)].glyph(), ' ');
    }

    #[test]
    fn print_aligned_right_on_an_offset_surface_drops_the_text() {
        let mut grid = Grid::new(8, 1);
        {
            // `area` starts at column 2, not 0: local and absolute coordinates now differ.
            let mut surface = Surface::new(&mut grid, Rect::new(2, 0, 6, 1), 0);
            surface.print_aligned(
                Rect::new(0, 0, 6, 1),
                "hi",
                crate::align::HAlign::Right,
                Style::default(),
            );
        }

        // `rect` (0, 0, 6, 1) intersected with the surface's own clip (2, 0, 6, 1) leaves
        // columns 2..6 visible; right-aligning "hi" within `rect` puts it at columns 4..6.
        assert_eq!(grid[Pos::new(4, 0)].glyph(), 'h');
        assert_eq!(grid[Pos::new(5, 0)].glyph(), 'i');
    }

    #[test]
    fn print_aligned_center_on_an_offset_surface_drops_the_text() {
        let mut grid = Grid::new(8, 1);
        {
            let mut surface = Surface::new(&mut grid, Rect::new(2, 0, 6, 1), 0);
            surface.print_aligned(
                Rect::new(0, 0, 6, 1),
                "hi",
                crate::align::HAlign::Center,
                Style::default(),
            );
        }

        // (6 - 2) / 2 == 2 columns of left padding within `rect`, so "hi" lands at absolute
        // columns 2..4, which is inside the surface's own visible columns 2..6.
        assert_eq!(grid[Pos::new(2, 0)].glyph(), 'h');
        assert_eq!(grid[Pos::new(3, 0)].glyph(), 'i');
    }

    #[test]
    fn clip_nests_monotonically() {
        let mut grid = Grid::new(8, 4);
        let mut surface = screen(&mut grid);
        let mut outer = surface.clip(Rect::new(1, 1, 4, 2));
        let inner = outer.clip(Rect::new(0, 0, 8, 4));

        assert_eq!(inner.clip_rect(), Rect::new(1, 1, 4, 2));
    }
}
