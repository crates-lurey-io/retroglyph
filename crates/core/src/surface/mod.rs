//! [`Surface`](crate::surface::Surface): an area-clipped, single-layer view over a [`Grid`](crate::grid::Grid).
//!
//! `Surface` is the workspace's one grid-drawing primitive. [`Terminal`](crate::terminal::Terminal)'s
//! [`draw`](crate::terminal::Terminal::draw)/[`surface`](crate::terminal::Terminal::surface) hand out a `Surface`
//! scoped to the whole grid, and `retroglyph-widgets` renders every widget into a `Surface`
//! scoped to a sub-[`Rect`](crate::grid::Rect): there is no separate stateful drawing API on `Terminal` itself.
//!
//! Place characters directly with [`put`](crate::surface::Surface::put) (or [`print`](crate::surface::Surface::print) for a
//! string, which handles newlines and wide characters), style-aware spans with
//! [`print_line`](crate::surface::Surface::print_line), or a whole styled run with
//! [`with_style`](crate::surface::Surface::with_style) so repeated calls don't need to pass the same [`Style`](crate::color::Style)
//! each time. [`clear`](crate::surface::Surface::clear)/[`clear_region`](crate::surface::Surface::clear_region) blank the active
//! layer (in full, or a rectangular region); switch layers with
//! [`on_layer`](crate::surface::Surface::on_layer). Or bypass the builder entirely and reach the [`Grid`](crate::grid::Grid)
//! directly via [`grid_mut`](crate::surface::Surface::grid_mut).

use crate::color::Tint;
use crate::grid::{Grid, Rect};

mod draw;
mod geometry;
mod styled;

#[cfg(test)]
mod tests;

pub use styled::StyledSurface;

/// The render target for every drawing call in the workspace: a mutable reference to a
/// [`Grid`](crate::grid::Grid) plus a fixed `layer`, scoped to an `area` and clipped to a `clip` rect.
///
/// A `Surface` is typically created once per frame, scoped to the whole drawing surface (e.g.
/// via [`Terminal::draw`](crate::terminal::Terminal::draw)), and handed to every subsystem/widget in turn;
/// each caller's own `area: Rect` (a sub-rect of the surface's own area, e.g. one produced by a
/// layout split) is relative to this surface's own `area` origin, not to the underlying grid.
/// [`Surface::put`](crate::surface::Surface::put)/[`Surface::print`](crate::surface::Surface::print)/... take coordinates in that same local space, where
/// `(0, 0)` is `area`'s top-left corner, and silently drop any write that falls outside
/// [`Surface::clip_rect`](crate::surface::Surface::clip_rect), matching the rest of the workspace's clip-on-draw policy for
/// out-of-bounds drawing.
///
/// `area` and `clip_rect` answer two different questions. `area` is the region this surface
/// *represents*: what a widget lays itself out in, and what [`width`](Self::width)/
/// [`height`](Self::height) report. `clip_rect` is the subset of `area` that is actually
/// *visible*: what every write is bounds-checked against. The two start out equal (see
/// [`Surface::new`](crate::surface::Surface::new)) and diverge once [`Surface::clip`](crate::surface::Surface::clip) or [`Surface::scope`](crate::surface::Surface::scope) is used.
///
/// [`Surface::clip`](crate::surface::Surface::clip) narrows what is visible without changing what this surface represents:
/// `clip_rect` is intersected with the given rect, `area` is untouched. [`Surface::scope`](crate::surface::Surface::scope) does
/// both: `area` becomes the given rect and `clip_rect` is intersected with it, which is what a
/// widget's own sub-surface needs when it should be laid out against a new rect but still bounded
/// by whatever was already visible. Both narrow monotonically: neither can widen `clip_rect`
/// beyond what the parent surface already allowed.
///
/// A caller that genuinely needs more than one layer at once (e.g. a modal dimming layer 0 while
/// drawing its own content on layer 1) switches layers with [`Surface::on_layer`](crate::surface::Surface::on_layer)/[`Surface::on_tier`](crate::surface::Surface::on_tier)
/// rather than being restricted to the layer it was constructed with.
pub struct Surface<'a> {
    grid: &'a mut Grid,
    area: Rect,
    clip: Rect,
    layer: u8,
    tint: Tint,
    origin_offset: (i32, i32),
}

/// A named z-order tier for [`Surface::on_tier`](crate::surface::Surface::on_tier), covering the split most apps with overlapping
/// UI actually need.
///
/// Layers are how overlapping UI avoids depending on draw order: a caller who paints a dropdown
/// on [`Layer::Overlay`](crate::surface::Layer::Overlay) gets it on top of the active screen regardless of whether the screen or
/// the dropdown drew first this frame, so the two don't have to agree on an ordering (contrast
/// with painting both through the same layer, where whichever call happens to run last wins).
///
/// `Layer` derives [`Ord`] over its declaration order, so `Layer::World < Layer::Hud <
/// Layer::Overlay < Layer::Debug` holds without spelling out the underlying grid layer ids --
/// the same relationship [`Surface::on_tier`](crate::surface::Surface::on_tier) relies on to keep `Layer::Debug` the top-most tier
/// no matter what else is open.
///
/// This is a convention, not a restriction: [`Surface::on_layer`](crate::surface::Surface::on_layer) still accepts any `u8`, and a
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
/// use retroglyph_core::color::Style;
/// use retroglyph_core::grid::{Grid, Rect};
/// use retroglyph_core::surface::{Layer, Surface};
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
    /// Popups, dropdowns, modals, painted over [`Layer::World`](crate::surface::Layer::World) and [`Layer::Hud`](crate::surface::Layer::Hud) regardless
    /// of draw order. Grid layer 2.
    Overlay,
    /// Debug and dev tooling. Always the top-most tier, so it stays visible over an open
    /// [`Layer::Overlay`](crate::surface::Layer::Overlay) rather than being hidden underneath one. Grid layer 3.
    ///
    /// `retroglyph-widgets`' `PerfOverlayApp` default layer is defined as `Layer::Debug.as_u8()`
    /// for exactly this reason: a perf HUD that a popup could paint over would be useless
    /// whenever an app actually has a popup open.
    Debug,
}

impl Layer {
    /// This tier's underlying grid layer id, for [`Surface::on_layer`](crate::surface::Surface::on_layer)/[`Grid`](crate::grid::Grid) APIs that take a
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
}
