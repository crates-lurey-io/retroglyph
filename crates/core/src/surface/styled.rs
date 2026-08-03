use crate::color::Style;
use crate::grid::{Grid, Offset, Pos, Rect, Size};

use super::Surface;

/// A [`Surface`] with a [`Style`] bound in, returned by [`Surface::with_style`].
///
/// Every draw call omits the `style` argument the underlying [`Surface`] method would otherwise
/// need, using the bound style instead. Reach back to the underlying surface (e.g. to call
/// [`Surface::print_line`], whose per-span styles make a bound style meaningless) via
/// [`StyledSurface::surface`].
pub struct StyledSurface<'s, 'a> {
    pub(super) surface: &'s mut Surface<'a>,
    pub(super) style: Style,
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

    /// [`Surface::put_signed`] using this view's bound style.
    pub fn put_signed(&mut self, pos: (i32, i32), ch: char) {
        self.surface.put_signed(pos, ch, self.style);
    }

    /// [`Surface::blit`]. Takes no style: `grid`'s own tiles carry their own.
    pub fn blit(&mut self, grid: &Grid, x: u16, y: u16) {
        self.surface.blit(grid, x, y);
    }

    /// [`Surface::clear`]. Takes no style: cleared cells reset to [`Tile::default`](crate::tile::Tile::default).
    pub fn clear(&mut self) {
        self.surface.clear();
    }

    /// [`Surface::clear_region`]. Takes no style: cleared cells reset to
    /// [`Tile::default`](crate::tile::Tile::default).
    pub fn clear_region(&mut self, rect: Rect) {
        self.surface.clear_region(rect);
    }

    /// [`Surface::width`] of the underlying surface.
    #[must_use]
    pub const fn width(&self) -> u16 {
        self.surface.width()
    }

    /// [`Surface::height`] of the underlying surface.
    #[must_use]
    pub const fn height(&self) -> u16 {
        self.surface.height()
    }

    /// [`Surface::area`] of the underlying surface.
    #[must_use]
    pub const fn area(&self) -> Rect {
        self.surface.area()
    }

    /// [`Surface::clip_rect`] of the underlying surface.
    #[must_use]
    pub const fn clip_rect(&self) -> Rect {
        self.surface.clip_rect()
    }

    /// [`Surface::local_area`] of the underlying surface.
    #[must_use]
    pub const fn local_area(&self) -> Rect {
        self.surface.local_area()
    }
}
