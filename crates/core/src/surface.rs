//! [`Surface`]: an area-clipped, single-layer view over a [`Grid`].
//!
//! `Surface` is the workspace's one grid-drawing primitive. [`Terminal`](crate::Terminal)'s
//! [`draw`](crate::Terminal::draw)/[`surface`](crate::Terminal::surface) hand out a `Surface`
//! scoped to the whole grid, and `retroglyph-widgets` renders every widget into a `Surface`
//! scoped to a sub-[`Rect`]: there is no separate stateful drawing API on `Terminal` itself.

use crate::grid::{Grid, Offset, Pos, Rect, Size};
use crate::style::Style;
use crate::text::Line;
use crate::tile::Tile;
#[cfg(not(feature = "egc"))]
use unicode_width::UnicodeWidthChar;

/// The render target for every drawing call in the workspace: a mutable reference to a
/// [`Grid`] plus a fixed `layer`, scoped to one area.
///
/// A `Surface` is typically created once per frame, scoped to the whole drawing surface (e.g.
/// via [`Terminal::draw`](crate::Terminal::draw)), and handed to every subsystem/widget in turn;
/// each caller's own `area: Rect` (a sub-rect of the surface's own area, e.g. one produced by a
/// layout split) is in the same coordinate space as [`Surface::area`] itself.
/// [`Surface::put`]/[`Surface::print`]/... take coordinates in that same space and silently clip
/// any write that falls outside [`Surface::area`], matching the rest of the workspace's
/// clip-on-draw policy for out-of-bounds drawing.
///
/// [`Surface::clip`] turns a sub-rect into a surface of its own, so a subsystem that should not
/// draw outside one is bounded by the type rather than trusted to respect an `area` handed to it
/// alongside a wider surface. The clip is intersected, never substituted, so narrowing only ever
/// tightens.
///
/// A caller that genuinely needs more than one layer at once (e.g. a modal dimming layer 0 while
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

    /// A new surface over the same grid and layer, clipped to `area` intersected with this
    /// surface's own area.
    ///
    /// Coordinates are unchanged: the sub-surface addresses the same space this one does, so a
    /// sub-rect computed against [`Surface::area`] (e.g. by a [`layout`](crate::layout) split)
    /// can be passed straight in. Because `area` is intersected rather than substituted,
    /// narrowing is monotonic: handing a surface down a layout tree can only ever tighten what a
    /// callee is able to touch.
    ///
    /// Clipping is also how the area-sensitive calls are told what they are drawing into:
    ///
    /// - [`print`](Self::print) wraps overflow onto the next row. Clipped to a one-row bar, the
    ///   wrapped remainder falls outside the area and is dropped, which is what a single-line
    ///   bar wants.
    /// - [`put_span`](Self::put_span) and [`put_span_uniform`](Self::put_span_uniform) refuse a
    ///   footprint that leaves the area. Clipped to a content rect, "fits" stops meaning "fits
    ///   the screen" and starts meaning "does not reserve cells in the status bar below".
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
    pub fn clip(&mut self, area: Rect) -> Surface<'_> {
        Surface {
            area: self.area.intersect(area),
            grid: self.grid,
            layer: self.layer,
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
    /// [`clip`](Self::clip) narrows a surface without handing out the unclipped grid to do it.
    pub const fn grid_mut(&mut self) -> &mut Grid {
        self.grid
    }

    /// `true` if `(x, y)` falls within this surface's area.
    fn in_bounds(&self, x: u16, y: u16) -> bool {
        self.area.contains(x, y)
    }

    /// Writes `grapheme` (already a single extended grapheme cluster) at `(x, y)`. A no-op if
    /// out of this surface's area.
    #[cfg(feature = "egc")]
    fn put_grapheme(&mut self, x: u16, y: u16, grapheme: &str, style: Style) {
        if !self.in_bounds(x, y) {
            return;
        }
        self.grid.write_grapheme(self.layer, x, y, grapheme, style);
    }

    /// Place `ch` at `pos` in `style`. A no-op if `pos` is outside this surface's area.
    ///
    /// If a pixel backend resolves `ch` to a sprite, that sprite is composited from its own
    /// pixels: [`style.fg`](Style::fg) does not tint it, and `style.bg` shows through only where
    /// the sprite is transparent. See [`put_span`](Self::put_span).
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
            if !self.in_bounds(pos.x, pos.y) {
                return;
            }
            let tile = Tile::new(ch, style);
            self.grid.put_tile(self.layer, pos, tile);
        }
    }

    /// Print `text` starting at `pos` in `style`.
    ///
    /// `\n` advances to the next row at the original column. Text that would extend beyond this
    /// surface's area wraps to the next row at the original column; cells outside the area
    /// (either axis) are clipped. When the `egc` feature is enabled, `text` is split into
    /// extended grapheme clusters (so combining marks and ZWJ sequences write as one cell each);
    /// otherwise it is split by `char`.
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

        let right = self.area.right();
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
        let right = self.area.right();
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
            // A single span wider than `u16::MAX` columns would already be unaddressable in this
            // crate's `u16` coordinate space; `cx` still saturates rather than overflowing even if
            // this cast wraps.
            #[allow(clippy::cast_possible_truncation)]
            let w = UnicodeWidthStr::width(span.content.as_str()) as u16;
            cx = cx.saturating_add(w);
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
    /// entirely within this surface's own area (not just the grid) at `pos`. The surface has
    /// strictly more ways to refuse a span than [`Grid::write_span`] does, so a sprite that did
    /// not draw is answered here rather than in the backend.
    pub fn put_span<S: AsRef<str>>(
        &mut self,
        pos: impl Into<Pos>,
        rows: &[S],
        style: Style,
    ) -> Option<()> {
        let pos = pos.into();
        let cols = rows.first()?.as_ref().chars().count();
        let w = u16::try_from(cols).ok()?;
        let h = u16::try_from(rows.len()).ok()?;
        if !self.span_fits(pos, w, h) {
            return None;
        }
        self.grid.write_span(self.layer, pos.x, pos.y, rows, style)
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
    /// within this surface's own area at `pos`.
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
        let size = size.into();
        if !self.span_fits(pos, size.width, size.height) {
            return None;
        }
        self.grid
            .write_span_uniform(self.layer, pos, size, anchor, fill, style)
    }

    /// `true` if a `w` x `h` footprint at `pos` lies entirely within this surface's area.
    ///
    /// A span is all-or-nothing rather than clipped like the per-cell writes, because a
    /// footprint half outside the area would reserve cells the caller does not own.
    fn span_fits(&self, pos: Pos, w: u16, h: u16) -> bool {
        pos.x >= self.area.left()
            && pos.y >= self.area.top()
            && pos.x.saturating_add(w) <= self.area.right()
            && pos.y.saturating_add(h) <= self.area.bottom()
    }

    /// Place `ch` at `pos` with a sub-cell pixel `offset`, in `style`.
    ///
    /// Sub-cell offsets are visual only: they do not affect grid logic or hit-testing.
    /// Backends that cannot represent pixel offsets (e.g. `CrosstermBackend`) ignore them. A
    /// no-op if `pos` is outside this surface's area.
    pub fn put_offset(
        &mut self,
        pos: impl Into<Pos>,
        offset: impl Into<Offset>,
        ch: char,
        style: Style,
    ) {
        let pos = pos.into();
        if !self.in_bounds(pos.x, pos.y) {
            return;
        }
        let offset = offset.into();
        let tile = Tile::new(ch, style).with_offset(offset.dx, offset.dy);
        self.grid.put_tile(self.layer, pos, tile);
    }

    /// Clears this surface's entire area (on its own layer) back to [`Tile::default`].
    pub fn clear(&mut self) {
        let area = self.area;
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                self.grid.put_tile(self.layer, (x, y), Tile::default());
            }
        }
    }

    /// Clears `rect` (clipped to this surface's own area, on its own layer) back to
    /// [`Tile::default`].
    pub fn clear_region(&mut self, rect: Rect) {
        let rect = rect.intersect(self.area);
        for y in rect.top()..rect.bottom() {
            for x in rect.left()..rect.right() {
                self.grid.put_tile(self.layer, (x, y), Tile::default());
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
            let mut styled = surface.with_style(Style::new().fg(crate::Color::RED));
            styled.put_span((0, 0), &["ab"]).expect("span write");
            styled
                .put_span_uniform((0, 1), (2, 1), 'C', ' ')
                .expect("span write");
        }

        assert_eq!(grid[Pos::new(0, 0)].style().foreground(), crate::Color::RED);
        assert_eq!(grid[Pos::new(0, 1)].style().foreground(), crate::Color::RED);
        assert_eq!(grid[Pos::new(0, 1)].span(), (2, 1));
    }

    #[test]
    fn clip_narrows_the_area_and_keeps_the_coordinate_space() {
        let mut grid = Grid::new(8, 4);
        let mut surface = screen(&mut grid);
        let sub = surface.clip(Rect::new(2, 1, 4, 2));

        assert_eq!(sub.area(), Rect::new(2, 1, 4, 2));
        assert_eq!(sub.width(), 4);
        assert_eq!(sub.height(), 2);
    }

    #[test]
    fn clip_keeps_the_layer() {
        let mut grid = Grid::new(4, 4);
        let mut surface = screen(&mut grid);
        let mut layer1 = surface.on_layer(1);

        assert_eq!(layer1.clip(Rect::new(0, 0, 2, 2)).layer(), 1);
    }

    #[test]
    fn clip_intersects_rather_than_replaces_so_it_cannot_widen() {
        let mut grid = Grid::new(8, 4);
        let area = Rect::new(2, 1, 4, 2);
        let mut surface = Surface::new(&mut grid, area, 0);

        // A rect reaching outside the surface's own area only ever tightens it.
        assert_eq!(surface.clip(Rect::new(0, 0, 8, 4)).area(), area);
        assert_eq!(
            surface.clip(Rect::new(0, 0, 4, 4)).area(),
            Rect::new(2, 1, 2, 2)
        );
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
            assert_eq!(sub.area(), Rect::EMPTY);
            sub.print((0, 0), "abc", Style::default());
        }

        assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
    }

    #[test]
    fn clip_nests_monotonically() {
        let mut grid = Grid::new(8, 4);
        let mut surface = screen(&mut grid);
        let mut outer = surface.clip(Rect::new(1, 1, 4, 2));
        let inner = outer.clip(Rect::new(0, 0, 8, 4));

        assert_eq!(inner.area(), Rect::new(1, 1, 4, 2));
    }
}
