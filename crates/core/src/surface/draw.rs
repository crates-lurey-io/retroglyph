use crate::color::Style;
use crate::color::Tint;
use crate::grid::{Grid, Offset, Pos, Rect, Size};
use crate::text::Line;
use crate::tile::Tile;
use ixy::HasSize;
use unicode_width::UnicodeWidthChar;

use super::Surface;

impl Surface<'_> {
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

        if !self.wide_spacer_fits(x, y, grapheme.width()) {
            return;
        }
        self.grid.write_grapheme(self.layer, x, y, grapheme, style);
        self.apply_tint(x, y);
    }

    /// `true` unless `width` is 2 and the spacer cell it would need at `x + 1` falls outside
    /// this surface's clip.
    ///
    /// [`Grid::put_tile`]/[`Grid::write_grapheme`] only refuse a wide write at the *grid*'s own
    /// right edge, not the clip's, so every wide write site (both the `egc` grapheme path via
    /// [`write_grapheme_at`](Self::write_grapheme_at) and the plain-`char` path in
    /// [`put`](Self::put)/[`put_signed`](Self::put_signed)/[`put_offset`](Self::put_offset))
    /// calls this first: without it, a clip narrower than the surface's own area would let a
    /// wide glyph's spacer land one column past the clip, silently overwriting whatever is
    /// there.
    fn wide_spacer_fits(&self, x: u16, y: u16, width: usize) -> bool {
        width != 2 || self.clip.contains(x.saturating_add(1), y)
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
            if !self.wide_spacer_fits(x, y, ch.width().unwrap_or(1)) {
                return;
            }
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
            if !self.wide_spacer_fits(abs_x, abs_y, ch.width().unwrap_or(1)) {
                return;
            }
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

        // `cx` is in the same (possibly translated) space as `pos.x` itself, since `shift()`
        // subtracts `origin_offset` from incoming coordinates before checking them against the
        // area (see `shift`'s doc). The wrap threshold has to live in that same space too, or a
        // translated surface (any `Camera::surface`, or a plain `translate`) wraps early by
        // exactly the offset: fold `origin_offset.0` back in alongside translating
        // `clip.right()` out of absolute grid space.
        let right = i64::from(self.clip.right()) - i64::from(self.area.left())
            + i64::from(self.origin_offset.0);
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
            if i64::from(cx) >= right {
                cx = pos.x;
                cy = cy.saturating_add(1);
            }
        }
    }

    /// [`print`](Self::print) implementation used when `egc` is disabled: splits on `char`.
    #[cfg(not(feature = "egc"))]
    fn print_chars(&mut self, pos: Pos, text: &str, style: Style) {
        // `cx` is in the same (possibly translated) space as `pos.x` itself, since `shift()`
        // subtracts `origin_offset` from incoming coordinates before checking them against the
        // area (see `shift`'s doc). The wrap threshold has to live in that same space too, or a
        // translated surface (any `Camera::surface`, or a plain `translate`) wraps early by
        // exactly the offset: fold `origin_offset.0` back in alongside translating
        // `clip.right()` out of absolute grid space.
        let right = i64::from(self.clip.right()) - i64::from(self.area.left())
            + i64::from(self.origin_offset.0);
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
            if i64::from(cx) >= right {
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
        // `cx` is in the same (possibly translated) space as `pos.x` itself, since `shift()`
        // subtracts `origin_offset` from incoming coordinates before checking them against the
        // area (see `shift`'s doc). The span-skip threshold has to live in that same space too,
        // or a translated surface (any `Camera::surface`, or a plain `translate`) skips every
        // span immediately: fold `origin_offset.0` back in alongside translating `clip.right()`
        // out of absolute grid space.
        let right = i64::from(self.clip.right()) - i64::from(self.area.left())
            + i64::from(self.origin_offset.0);
        let mut cx = pos.x;
        for span in &line.spans {
            if i64::from(cx) >= right {
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
    /// the overflow, for every [`HAlign`](crate::layout::HAlign) (matching how
    /// [`HAlign::Center`](crate::layout::HAlign::Center) itself saturates in
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
    /// use retroglyph_core::layout::HAlign;
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
        align: crate::layout::HAlign,
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
        // `put`'s own per-cell write only when there's no tint to apply and `ch` is a
        // single-column glyph, so `put`'s wide-char spacer bookkeeping never triggers.
        // Anything else (tinted surface, zero/double-width glyph) falls back to the per-cell
        // loop, unchanged from before this method had a fast path.
        let single_width = UnicodeWidthChar::width(ch) == Some(1);

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
    /// Unlike [`put`](Self::put) and the rest of this surface's single-sprite writes, this does
    /// not apply [`with_tint`](Self::with_tint)'s tint: a tint lands on one sprite's anchor cell,
    /// and `grid` is arbitrary composed content with no single anchor to land it on, the same
    /// reason `Grid::blit_cross_layer` (this method's own cross-layer copy, internal to `Grid`)
    /// carries no tint either. A tinted surface's `blit` copies `grid` through unchanged.
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
            if !self.wide_spacer_fits(x, y, ch.width().unwrap_or(1)) {
                return;
            }
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
