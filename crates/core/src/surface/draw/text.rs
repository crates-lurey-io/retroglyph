//! [`print`](Surface::print) and friends: text writing, wrapping, and alignment.

use crate::color::Style;
use crate::grid::{Pos, Rect};
use crate::text::Line;
#[cfg(not(feature = "egc"))]
use unicode_width::UnicodeWidthChar;

use super::Surface;

impl Surface<'_> {
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
    /// use retroglyph_core::color::Style;
    /// use retroglyph_core::terminal::Terminal;
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
        // translated surface (a scrolling-camera widget's `surface` method, or a plain `translate`) wraps early by
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
        // translated surface (a scrolling-camera widget's `surface` method, or a plain `translate`) wraps early by
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
    /// use retroglyph_core::terminal::Terminal;
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
        // or a translated surface (a scrolling-camera widget's `surface` method, or a plain `translate`) skips every
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
    /// `rect` is local to this surface's own [`area`](Self::area), the same convention as
    /// [`fill_rect`](Self::fill_rect) and [`clear_region`](Self::clear_region) (not absolute grid
    /// coordinates, the convention [`clip`](Self::clip)/[`scope`](Self::scope) use for their own
    /// `rect`): `(0, 0)` is `area`'s own top-left, so a widget's own `area().at_origin()` can be
    /// passed straight in.
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
    /// use retroglyph_core::color::Style;
    /// use retroglyph_core::grid::Rect;
    /// use retroglyph_core::terminal::Terminal;
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
        let pos = (rect.left().saturating_add(x_offset), rect.top());
        // `rect` (like `pos` here) is local to `self.area` and deliberately independent of any
        // outstanding `translate`, matching a widget's own `area().at_origin()`. `print` itself
        // subtracts `origin_offset` again (via `shift`), so a translated surface would subtract
        // it twice and drop the text entirely unless it's cancelled first: hand `print` a view
        // whose `origin_offset` is zeroed out rather than adjusting `pos` by hand, which would
        // need signed arithmetic that a `u16`-based `Pos` can't always represent losslessly.
        let undo = (
            0i32.saturating_sub(self.origin_offset.0),
            0i32.saturating_sub(self.origin_offset.1),
        );
        self.translate(undo).print(pos, text, style);
    }
}
