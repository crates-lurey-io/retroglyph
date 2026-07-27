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
use crate::tint::Tint;
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
    tint: Tint,
    origin_offset: (i32, i32),
}

impl<'a> Surface<'a> {
    /// A surface over `grid`, scoped to `area` on `layer`, tinting nothing.
    pub const fn new(grid: &'a mut Grid, area: Rect, layer: u8) -> Self {
        Self {
            grid,
            area,
            layer,
            tint: Tint::None,
            origin_offset: (0, 0),
        }
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
            tint: self.tint,
            origin_offset: self.origin_offset,
        }
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
            layer: self.layer,
            tint,
            origin_offset: self.origin_offset,
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
            tint: self.tint,
            origin_offset: self.origin_offset,
        }
    }

    /// A view whose `(0, 0)` sits at `origin` relative to this surface's own coordinate space, so
    /// a caller can draw in a shifted (e.g. world/camera) coordinate space and let the surface do
    /// the clipping, rather than subtracting `origin` from every coordinate by hand.
    ///
    /// Every coordinate-taking method on the returned surface -- [`put`](Self::put),
    /// [`put_signed`](Self::put_signed), [`print`](Self::print), [`print_line`](Self::print_line),
    /// [`fill_rect`](Self::fill_rect), [`put_offset`](Self::put_offset),
    /// [`put_span`](Self::put_span), [`put_span_uniform`](Self::put_span_uniform), and
    /// [`clear_region`](Self::clear_region) -- subtracts `origin` (composed with any outstanding
    /// translate) from the coordinate it is given before applying its usual bounds check. Only
    /// [`clear`](Self::clear), which takes no coordinate and always clears this surface's whole
    /// area, is unaffected.
    ///
    /// This does not touch [`area`](Self::area), so [`area`](Self::area), [`width`](Self::width),
    /// and [`height`](Self::height) keep reporting the same thing before and after translating:
    /// only the coordinate a caller must pass to land a write shifts, never what the surface
    /// itself covers. This composes with [`clip`](Self::clip) the same order it is called in:
    /// `clip(...).translate(...)` first narrows the area, then shifts the coordinate space that
    /// still-narrowed area is addressed in, so a coordinate that goes negative after the shift can
    /// land inside the pre-narrowed area.
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
    /// let mut clipped = surface.clip(Rect::new(5, 5, 4, 4));
    /// let mut view = clipped.translate((-5, -5));
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
    /// [`clip`](Self::clip) narrows a surface without handing out the unclipped grid to do it.
    pub const fn grid_mut(&mut self) -> &mut Grid {
        self.grid
    }

    /// Shifts `(x, y)` by this surface's translate offset (see [`translate`](Self::translate)),
    /// returning the coordinate to actually write at if the shift still lands inside this
    /// surface's own area, or `None` otherwise.
    fn shift(&self, x: u16, y: u16) -> Option<(u16, u16)> {
        let sx = i32::from(x).checked_sub(self.origin_offset.0)?;
        let sy = i32::from(y).checked_sub(self.origin_offset.1)?;
        let sx = u16::try_from(sx).ok()?;
        let sy = u16::try_from(sy).ok()?;
        self.area.contains(sx, sy).then_some((sx, sy))
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
    /// out of this surface's area.
    #[cfg(feature = "egc")]
    fn put_grapheme(&mut self, x: u16, y: u16, grapheme: &str, style: Style) {
        let Some((x, y)) = self.shift(x, y) else {
            return;
        };
        self.grid.write_grapheme(self.layer, x, y, grapheme, style);
        self.apply_tint(x, y);
    }

    /// Place `ch` at `pos` in `style`. A no-op if `pos` is outside this surface's area.
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
    /// // Outside the surface's area: silently dropped, not a panic.
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
        let tile = Tile::new(ch, style);
        self.grid.put_tile(self.layer, (abs_x, abs_y), tile);
        self.apply_tint(abs_x, abs_y);
    }

    /// Print `text` starting at `pos` in `style`.
    ///
    /// `\n` advances to the next row at the original column. Text that would extend beyond this
    /// surface's area wraps to the next row at the original column; cells outside the area
    /// (either axis) are clipped. When the `egc` feature is enabled, `text` is split into
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
    /// // surface's own area are touched, the rest is silently clipped.
    /// surface.fill_rect(Rect::new(2, 2, 10, 10), '#', Style::default());
    ///
    /// assert_eq!(grid[Pos::new(3, 3)].glyph(), '#');
    /// assert_eq!(grid[Pos::new(0, 0)].glyph(), ' ');
    /// ```
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
        let (x, y) = self.shift(pos.x, pos.y)?;
        let pos = Pos::new(x, y);
        let size = size.into();
        if !self.span_fits(pos, size.width, size.height) {
            return None;
        }
        self.grid
            .write_span_uniform(self.layer, pos, size, anchor, fill, style)?;
        self.apply_tint(pos.x, pos.y);
        Some(())
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
    /// // Outside the surface's area: silently dropped, matching `put`.
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
        let tile = Tile::new(ch, style).with_offset(offset.dx, offset.dy);
        self.grid.put_tile(self.layer, (x, y), tile);
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
    /// // `rect` extends past the surface's own area; only the overlap is cleared.
    /// surface.clear_region(Rect::new(2, 2, 10, 10));
    ///
    /// assert_eq!(grid[Pos::new(2, 2)].glyph(), ' ');
    /// assert_eq!(grid[Pos::new(1, 1)].glyph(), '#');
    /// ```
    pub fn clear_region(&mut self, rect: Rect) {
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
    fn translate_does_not_change_area_width_or_height() {
        let mut grid = Grid::new(10, 10);
        let mut surface = screen(&mut grid);
        let mut clipped = surface.clip(Rect::new(5, 5, 4, 4));
        let view = clipped.translate((-5, -5));

        assert_eq!(view.area(), Rect::new(5, 5, 4, 4));
        assert_eq!(view.width(), 4);
        assert_eq!(view.height(), 4);
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
    fn translate_composes_with_clip_and_lets_a_negative_signed_coordinate_land() {
        let mut grid = Grid::new(10, 10);
        {
            let mut surface = screen(&mut grid);
            let mut clipped = surface.clip(Rect::new(5, 5, 4, 4));
            let mut view = clipped.translate((-5, -5));

            // -5 minus the translate origin (-5) is 0: the viewport's own local origin, landing
            // at the clipped area's top-left grid cell.
            view.put_signed((-5, -5), 'X', Style::default());
            // -6 minus -5 is still -1: still negative, so still out of bounds.
            view.put_signed((-6, -6), 'Y', Style::default());
        }

        assert_eq!(grid[Pos::new(5, 5)].glyph(), 'X');
        assert_eq!(grid[Pos::new(4, 4)].glyph(), ' ');
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
            assert_eq!(
                view.put_span((4, 4), &["ab"], Style::default()),
                Some(())
            );
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
    fn clip_nests_monotonically() {
        let mut grid = Grid::new(8, 4);
        let mut surface = screen(&mut grid);
        let mut outer = surface.clip(Rect::new(1, 1, 4, 2));
        let inner = outer.clip(Rect::new(0, 0, 8, 4));

        assert_eq!(inner.area(), Rect::new(1, 1, 4, 2));
    }
}
