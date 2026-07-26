//! [`Surface`]: a widget's render target, a [`Grid`] scoped to one layer and area.
#[cfg(not(feature = "egc"))]
use retroglyph_core::Tile;
use retroglyph_core::text::Line;
use retroglyph_core::{Grid, Pos, Rect, Style};
use unicode_width::UnicodeWidthChar;

/// The render target every [`Widget`](crate::Widget)/[`StatefulWidget`](crate::StatefulWidget)
/// draws into: a mutable reference to a [`Grid`] plus a fixed `layer`, scoped to one area.
///
/// A `Surface` is typically created once per frame, scoped to the whole drawing surface (e.g.
/// `Surface::new(grid, grid_area, 0)`), and handed to every widget in turn; each widget's own
/// `area: Rect` argument (a sub-rect of the surface's own area, e.g. one produced by
/// [`crate::split_h`]/[`crate::split_v`]) is in the same coordinate space as
/// [`Surface::area`] itself. [`Surface::put`]/[`Surface::print`]/... take coordinates in that
/// same space and silently clip any write that falls outside [`Surface::area`] -- a widget
/// cannot draw outside the [`Rect`] it was given, matching the rest of the workspace's
/// clip-on-draw policy for out-of-bounds drawing.
///
/// A widget that genuinely needs more than one layer at once (e.g. a modal dimming layer 0 while
/// drawing its own content on layer 1) switches layers with [`Surface::on_layer`] rather than
/// being restricted to the layer it was constructed with.
pub struct Surface<'a> {
    grid: &'a mut Grid,
    area: Rect,
    layer: u8,
}

impl<'a> Surface<'a> {
    /// A surface over `grid`, scoped to `area` on `layer`.
    pub const fn new(grid: &'a mut Grid, area: Rect, layer: u8) -> Self {
        Self { grid, area, layer }
    }

    /// The area this surface clips writes to.
    #[must_use]
    pub const fn area(&self) -> Rect {
        self.area
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

    /// A new surface over the same grid and area, but writing to `layer` instead.
    #[must_use]
    pub const fn on_layer(&mut self, layer: u8) -> Surface<'_> {
        Surface {
            grid: self.grid,
            area: self.area,
            layer,
        }
    }

    /// Borrows the underlying [`Grid`] directly, with no clipping.
    ///
    /// Escape hatch for multi-layer or whole-grid operations (e.g. [`Grid::blit`]) that don't fit
    /// this surface's clipped, single-layer model.
    pub const fn grid_mut(&mut self) -> &mut Grid {
        self.grid
    }

    /// `true` if `(x, y)` falls within this surface's area.
    fn in_bounds(&self, x: u16, y: u16) -> bool {
        self.area.contains(x, y)
    }

    /// Place `ch` at `pos` in `style`. A no-op if `pos` is outside this surface's area.
    pub fn put(&mut self, pos: impl Into<Pos>, ch: char, style: Style) {
        let pos = pos.into();
        if !self.in_bounds(pos.x, pos.y) {
            return;
        }
        #[cfg(feature = "egc")]
        {
            let mut buf = [0u8; 4];
            let s = ch.encode_utf8(&mut buf);
            self.grid.write_grapheme(self.layer, pos.x, pos.y, s, style);
        }
        #[cfg(not(feature = "egc"))]
        {
            let tile = Tile::new(ch, style);
            self.grid.put_tile(self.layer, pos, tile);
        }
    }

    /// Print `text` starting at `pos` in `style`.
    ///
    /// `\n` advances to the next row at the original column. Text that would extend beyond this
    /// surface's area wraps to the next row at the original column; cells outside the area
    /// (either axis) are clipped, matching [`Terminal::print`](retroglyph_core::Terminal::print).
    pub fn print(&mut self, pos: impl Into<Pos>, text: &str, style: Style) {
        let pos = pos.into();
        let right = self.area.right();
        let mut cx = pos.x;
        let mut cy = pos.y;
        for ch in text.chars() {
            if ch == '\n' {
                cx = pos.x;
                cy = cy.saturating_add(1);
                continue;
            }
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
    /// Stops once a span would start past this surface's area.
    pub fn print_line(&mut self, pos: impl Into<Pos>, line: &Line) {
        use unicode_width::UnicodeWidthStr;

        let pos = pos.into();
        let right = self.area.right();
        let mut cx = pos.x;
        for span in &line.spans {
            if cx >= right {
                break;
            }
            self.print((cx, pos.y), &span.content, span.style);
            cx = cx.saturating_add(UnicodeWidthStr::width(span.content.as_str()) as u16);
        }
    }

    /// Fill `rect` (clipped to this surface's own area) with `ch` in `style`.
    pub fn fill_rect(&mut self, rect: Rect, ch: char, style: Style) {
        for y in rect.top()..rect.bottom() {
            for x in rect.left()..rect.right() {
                self.put((x, y), ch, style);
            }
        }
    }
}
