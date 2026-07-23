//! [`BoxStyle`]: a Lip-Gloss-style box model (padding, border, margin).
//!
//! Renders content into a standalone [`Grid`], independent of any
//! [`Backend`]/[`Terminal`].
//!
//! `BoxStyle` does not word-wrap: it lays out already-broken lines (only
//! `'\n'` is treated specially).
//!
//! For word-wrapping text to a width first, use
//! `Paragraph`/`retroglyph_core::layout::TextLayout` (behind the `egc`
//! feature), then hand the wrapped result to `BoxStyle::render`. Keeping
//! wrapping and box-model layout separate avoids tying every consumer of
//! this module to the `egc` feature.
use retroglyph_core::{Backend, Grid, Rect, Style, Terminal, Tile};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::draw::{BL, BR, H, TL, TR, V};
use crate::text::truncate;
use crate::widget::Widget;

/// CSS-style box-model sides: top/right/bottom/left, in terminal cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sides {
    /// Cells above.
    pub top: u16,
    /// Cells to the right.
    pub right: u16,
    /// Cells below.
    pub bottom: u16,
    /// Cells to the left.
    pub left: u16,
}

impl Sides {
    /// No space on any side.
    pub const ZERO: Self = Self {
        top: 0,
        right: 0,
        bottom: 0,
        left: 0,
    };

    /// The same number of cells on all four sides.
    #[must_use]
    pub const fn all(n: u16) -> Self {
        Self {
            top: n,
            right: n,
            bottom: n,
            left: n,
        }
    }

    /// `vertical` cells top/bottom, `horizontal` cells left/right (CSS
    /// `padding: v h` shorthand).
    #[must_use]
    pub const fn symmetric(vertical: u16, horizontal: u16) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }

    /// Returns `self` with `top` replaced.
    #[must_use]
    pub const fn top(mut self, top: u16) -> Self {
        self.top = top;
        self
    }

    /// Returns `self` with `right` replaced.
    #[must_use]
    pub const fn right(mut self, right: u16) -> Self {
        self.right = right;
        self
    }

    /// Returns `self` with `bottom` replaced.
    #[must_use]
    pub const fn bottom(mut self, bottom: u16) -> Self {
        self.bottom = bottom;
        self
    }

    /// Returns `self` with `left` replaced.
    #[must_use]
    pub const fn left(mut self, left: u16) -> Self {
        self.left = left;
        self
    }

    const fn horizontal(self) -> u16 {
        self.left.saturating_add(self.right)
    }

    const fn vertical(self) -> u16 {
        self.top.saturating_add(self.bottom)
    }
}

/// A box-model wrapper: content, padding, an optional single-line border,
/// and margin, rendered into a standalone [`Grid`] via [`BoxStyle::render`].
///
/// Layers from the inside out: content -> padding -> border -> margin.
/// Margin cells are left empty (transparent, per [`Grid::new`]'s default
/// tiles), matching CSS margin being outside the box's own background.
///
/// # Examples
///
/// ```
/// use retroglyph_core::Style;
/// use retroglyph_widgets::{BoxStyle, Sides};
///
/// let grid = BoxStyle::new(Style::new())
///     .border(true)
///     .padding(Sides::all(1))
///     .render("hi");
/// assert_eq!(grid.get(2, 2).glyph(), 'h'); // 1 border + 1 padding cell in from the corner
/// ```
#[derive(Clone, Copy, Debug)]
pub struct BoxStyle {
    style: Style,
    padding: Sides,
    margin: Sides,
    border: bool,
    width: Option<u16>,
    height: Option<u16>,
}

impl BoxStyle {
    /// A borderless box with no padding/margin, in `style`, sized to fit its
    /// content.
    #[must_use]
    pub const fn new(style: Style) -> Self {
        Self {
            style,
            padding: Sides::ZERO,
            margin: Sides::ZERO,
            border: false,
            width: None,
            height: None,
        }
    }

    /// Sets the padding, between the border (if any) and the content.
    #[must_use]
    pub const fn padding(mut self, padding: Sides) -> Self {
        self.padding = padding;
        self
    }

    /// Sets the margin, outside the border (if any); left transparent.
    #[must_use]
    pub const fn margin(mut self, margin: Sides) -> Self {
        self.margin = margin;
        self
    }

    /// Draws a single-line border, in `style`, around the padding.
    #[must_use]
    pub const fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    /// Sets an explicit content width (excludes padding/border/margin).
    ///
    /// Lines wider than this are clipped; without this, the box sizes to
    /// its widest content line.
    #[must_use]
    pub const fn width(mut self, width: u16) -> Self {
        self.width = Some(width);
        self
    }

    /// Sets an explicit content height (excludes padding/border/margin).
    ///
    /// Lines past this are dropped; without this, the box sizes to the
    /// number of lines in the content.
    #[must_use]
    pub const fn height(mut self, height: u16) -> Self {
        self.height = Some(height);
        self
    }

    /// Renders `text` into a standalone [`Grid`]: content, padding, border,
    /// and margin, in that order from the inside out.
    ///
    /// `text` is split only on `'\n'`; it is not word-wrapped (see the
    /// module docs).
    ///
    /// Content is positioned by display column (via `unicode-width`), so a
    /// wide (2-column) character correctly pushes later characters on the
    /// same line over by 2 columns rather than 1. It is, however, written
    /// without a `WIDE_CHAR_SPACER` reservation on the cell to its right (see
    /// `retroglyph_core::Grid::write_grapheme`, which requires the `egc`
    /// feature this module deliberately does not depend on) -- terminal-
    /// rendering backends may misalign output by one column per wide
    /// character as a result. Fully correct wide-character rendering needs
    /// an `egc`-gated code path; not yet implemented here.
    #[must_use]
    pub fn render(&self, text: &str) -> Grid {
        let lines: Vec<&str> = text.split('\n').collect();
        let content_w = self.width.unwrap_or_else(|| {
            u16::try_from(lines.iter().map(|l| l.width()).max().unwrap_or(0)).unwrap_or(u16::MAX)
        });
        let content_h = self
            .height
            .unwrap_or_else(|| u16::try_from(lines.len()).unwrap_or(u16::MAX));

        let (mut grid, content_x, content_y) = self.scaffold(content_w, content_h);
        for (row, line) in lines.iter().take(usize::from(content_h)).enumerate() {
            let Ok(row) = u16::try_from(row) else { break };
            let clipped = truncate(line, usize::from(content_w));
            let mut col = 0u16;
            for ch in clipped.chars() {
                let w = u16::try_from(ch.width().unwrap_or(0)).unwrap_or(u16::MAX);
                if col.saturating_add(w) > content_w {
                    break;
                }
                grid.put(content_x + col, content_y + row, Tile::new(ch, self.style));
                col = col.saturating_add(w);
            }
        }
        grid
    }

    /// Word-wraps `text` to this box's content width, then renders it the
    /// same way as [`render`](Self::render): content, padding, border, and
    /// margin, from the inside out.
    ///
    /// Requires the `egc` feature: wrapping is delegated to
    /// `retroglyph_core::layout::TextLayout`, which (unlike `render`) also
    /// places wide characters correctly, with a proper `WIDE_CHAR_SPACER`.
    /// If no explicit width was set via [`BoxStyle::width`], `text` is
    /// measured but not wrapped (there is no width to wrap to), matching
    /// `render`'s own natural-width fallback.
    #[cfg(feature = "egc")]
    #[must_use]
    pub fn render_wrapped(&self, text: &str) -> Grid {
        use retroglyph_core::Headless;
        use retroglyph_core::layout::TextLayout;
        use retroglyph_core::text::{Line, Span};

        let content_w = self.width.unwrap_or_else(|| {
            u16::try_from(
                text.split('\n')
                    .map(UnicodeWidthStr::width)
                    .max()
                    .unwrap_or(0),
            )
            .unwrap_or(u16::MAX)
        });
        let line = Line::from(Span::styled(text, self.style));
        let content_h = self.height.unwrap_or_else(|| {
            TextLayout::new(&line)
                .rect(Rect::new(0, 0, content_w, u16::MAX))
                .measure()
                .height
        });

        let (mut grid, content_x, content_y) = self.scaffold(content_w, content_h);

        // TextLayout::render only knows how to draw into a Terminal; render
        // into a scratch headless one sized to the content area, then blit
        // that (layer 0 only) onto the scaffold. This also gets
        // wide-character placement right "for free", since TextLayout uses
        // `Grid::write_grapheme` internally.
        let mut scratch = Terminal::new(Headless::new(content_w.max(1), content_h.max(1)));
        TextLayout::new(&line)
            .rect(Rect::new(0, 0, content_w, content_h))
            .render(&mut scratch);
        let content_rect = Rect::new(0, 0, content_w, content_h);
        grid.blit(0, scratch.grid(), content_rect, content_x, content_y);

        grid
    }

    /// Builds the padding/border/margin scaffold for a `content_w`x`content_h`
    /// content area: a fresh [`Grid`] with the box's background (and border,
    /// if any) already drawn, plus the `(x, y)` offset where content should
    /// be written.
    fn scaffold(&self, content_w: u16, content_h: u16) -> (Grid, u16, u16) {
        let border_wh = u16::from(self.border) * 2;
        let inner_w = content_w
            .saturating_add(self.padding.horizontal())
            .saturating_add(border_wh);
        let inner_h = content_h
            .saturating_add(self.padding.vertical())
            .saturating_add(border_wh);
        let outer_w = inner_w.saturating_add(self.margin.horizontal()).max(1);
        let outer_h = inner_h.saturating_add(self.margin.vertical()).max(1);

        let mut grid = Grid::new(outer_w, outer_h);
        let box_x = self.margin.left;
        let box_y = self.margin.top;

        fill_rect(&mut grid, box_x, box_y, inner_w, inner_h, self.style);
        if self.border {
            // `inner_w`/`inner_h` already include the border's own 2 cells
            // (`border_wh` above), so both are always >= 2 here.
            draw_border(&mut grid, box_x, box_y, inner_w, inner_h, self.style);
        }

        let content_x = box_x
            .saturating_add(u16::from(self.border))
            .saturating_add(self.padding.left);
        let content_y = box_y
            .saturating_add(u16::from(self.border))
            .saturating_add(self.padding.top);
        (grid, content_x, content_y)
    }
}

/// Pairs a [`BoxStyle`] with the text it should render, so the pair can
/// implement [`Widget`] (which has no room for a text parameter). Build one
/// via [`BoxStyle::text`].
///
/// [`Widget::render`] places the box at `area`'s top-left corner, sized to
/// the style's own explicit-or-content-fit dimensions -- it does not stretch
/// or clip to fill `area`. It always uses [`BoxStyle::render`] (not
/// `BoxStyle::render_wrapped`, behind the `egc` feature); for wrapped
/// content, call `render_wrapped` directly and [`crate::blit_into`] the
/// result yourself.
#[derive(Clone, Copy, Debug)]
pub struct Boxed<'a> {
    style: BoxStyle,
    text: &'a str,
}

impl BoxStyle {
    /// Pairs this style with `text`, ready to draw via [`Widget::render`].
    #[must_use]
    pub const fn text(self, text: &str) -> Boxed<'_> {
        Boxed { style: self, text }
    }
}

impl<B: Backend> Widget<B> for Boxed<'_> {
    fn render(self, area: Rect, term: &mut Terminal<B>) {
        let grid = self.style.render(self.text);
        crate::block::blit_into(term, &grid, area.left(), area.top());
    }
}

/// Fill `w`×`h` starting at `(x, y)` with a `style`d space.
fn fill_rect(grid: &mut Grid, x: u16, y: u16, w: u16, h: u16, style: Style) {
    for dy in 0..h {
        for dx in 0..w {
            grid.put(x + dx, y + dy, Tile::new(' ', style));
        }
    }
}

/// Draw a single-line border around the `w`×`h` rect at `(x, y)`, in
/// `style`. Caller must ensure `w >= 2 && h >= 2`.
fn draw_border(grid: &mut Grid, x: u16, y: u16, w: u16, h: u16, style: Style) {
    let right = x + w - 1;
    let bottom = y + h - 1;

    grid.put(x, y, Tile::new(TL, style));
    grid.put(right, y, Tile::new(TR, style));
    grid.put(x, bottom, Tile::new(BL, style));
    grid.put(right, bottom, Tile::new(BR, style));
    for cx in (x + 1)..right {
        grid.put(cx, y, Tile::new(H, style));
        grid.put(cx, bottom, Tile::new(H, style));
    }
    for cy in (y + 1)..bottom {
        grid.put(x, cy, Tile::new(V, style));
        grid.put(right, cy, Tile::new(V, style));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyphs(grid: &Grid) -> Vec<String> {
        (0..grid.height())
            .map(|y| (0..grid.width()).map(|x| grid.get(x, y).glyph()).collect())
            .collect()
    }

    #[test]
    fn sides_helpers() {
        assert_eq!(
            Sides::all(2),
            Sides {
                top: 2,
                right: 2,
                bottom: 2,
                left: 2
            }
        );
        assert_eq!(
            Sides::symmetric(1, 3),
            Sides {
                top: 1,
                right: 3,
                bottom: 1,
                left: 3
            }
        );
    }

    #[test]
    fn sizes_to_content_with_no_padding_or_border() {
        let grid = BoxStyle::new(Style::default()).render("hi");
        assert_eq!((grid.width(), grid.height()), (2, 1));
        assert_eq!(grid.get(0, 0).glyph(), 'h');
        assert_eq!(grid.get(1, 0).glyph(), 'i');
    }

    #[test]
    fn sizes_to_the_widest_of_multiple_lines() {
        let grid = BoxStyle::new(Style::default()).render("a\nbcd\nef");
        assert_eq!((grid.width(), grid.height()), (3, 3));
        assert_eq!(grid.get(0, 0).glyph(), 'a');
        assert_eq!(grid.get(1, 0).glyph(), ' '); // shorter line padded with blanks
        assert_eq!(grid.get(0, 1).glyph(), 'b');
        assert_eq!(grid.get(2, 1).glyph(), 'd');
    }

    #[test]
    fn explicit_width_clips_longer_lines_and_pads_shorter_ones() {
        let grid = BoxStyle::new(Style::default()).width(3).render("hello");
        assert_eq!(grid.width(), 3);
        let row: String = (0..3).map(|x| grid.get(x, 0).glyph()).collect();
        assert_eq!(row, "hel");
    }

    #[test]
    fn explicit_height_drops_extra_lines() {
        let grid = BoxStyle::new(Style::default()).height(1).render("a\nb\nc");
        assert_eq!(grid.height(), 1);
        assert_eq!(grid.get(0, 0).glyph(), 'a');
    }

    #[test]
    fn padding_surrounds_content_with_the_box_style() {
        let grid = BoxStyle::new(Style::default())
            .padding(Sides::all(1))
            .render("x");
        // 1 content col/row + 1 padding on each side = 3x3.
        assert_eq!((grid.width(), grid.height()), (3, 3));
        assert_eq!(grid.get(1, 1).glyph(), 'x');
        assert_eq!(grid.get(0, 0).glyph(), ' ');
    }

    #[test]
    fn border_draws_a_box_around_padding_and_content() {
        let grid = BoxStyle::new(Style::default()).border(true).render("x");
        // 1 content col/row + 2 border = 3x3.
        assert_eq!((grid.width(), grid.height()), (3, 3));
        let rows = glyphs(&grid);
        assert_eq!(rows[0], "┌─┐");
        assert_eq!(rows[1], "│x│");
        assert_eq!(rows[2], "└─┘");
    }

    #[test]
    fn margin_is_left_transparent_outside_the_border() {
        let grid = BoxStyle::new(Style::default())
            .margin(Sides::all(1))
            .render("x");
        // 1x1 content, 1 margin on each side = 3x3; margin cells are never
        // written, so they keep Grid::new's default "empty" tile, which
        // Grid::blit treats as transparent.
        assert_eq!((grid.width(), grid.height()), (3, 3));
        assert!(grid.get(0, 0).is_empty());
        assert_eq!(grid.get(1, 1).glyph(), 'x');
    }

    #[test]
    fn wide_characters_push_later_columns_over_by_their_width() {
        // "あ" (HIRAGANA A) is 2 columns wide: width("aあb") == 4, and 'b'
        // must land at column 3, not column 2 (its char index), or it would
        // collide with あ's second visual column.
        //
        // Note: this only checks *sizing*/*column offset* correctness. The
        // wide glyph itself is still written without a WIDE_CHAR_SPACER (see
        // render()'s doc comment); a real terminal backend may still
        // misrender the cell to its right.
        let grid = BoxStyle::new(Style::default()).render("aあb");
        assert_eq!(grid.width(), 4);
        assert_eq!(grid.get(0, 0).glyph(), 'a');
        assert_eq!(grid.get(1, 0).glyph(), 'あ');
        assert_eq!(grid.get(3, 0).glyph(), 'b');
    }

    #[test]
    fn border_with_empty_content_is_still_at_least_a_2x2_box() {
        // No content, no padding: inner size is exactly the border's own 2
        // cells in each axis (content_w = 0, content_h = 1 line of "").
        let grid = BoxStyle::new(Style::default()).border(true).render("");
        assert_eq!((grid.width(), grid.height()), (2, 3));
        let rows = glyphs(&grid);
        assert_eq!(rows[0], "┌┐");
        assert_eq!(rows[2], "└┘");
    }

    #[test]
    #[cfg(feature = "egc")]
    fn render_wrapped_word_wraps_to_the_explicit_width() {
        // Same text/width Paragraph's own tests use (see widget/paragraph.rs),
        // so this is exercising the same, already-verified TextLayout wrap.
        let grid = BoxStyle::new(Style::default())
            .width(10)
            .render_wrapped("the quick brown fox jumps");
        assert_eq!(grid.width(), 10);
        let rows = glyphs(&grid);
        assert_eq!(rows[0].trim_end(), "the quick");
        assert_eq!(rows[1].trim_end(), "brown fox");
        assert_eq!(rows[2].trim_end(), "jumps");
    }

    #[test]
    #[cfg(feature = "egc")]
    fn render_wrapped_without_an_explicit_width_measures_but_does_not_wrap() {
        // No width set: same natural-width fallback as `render`, so nothing
        // is short enough to need wrapping.
        let grid = BoxStyle::new(Style::default()).render_wrapped("hi");
        assert_eq!((grid.width(), grid.height()), (2, 1));
        assert_eq!(grid.get(0, 0).glyph(), 'h');
        assert_eq!(grid.get(1, 0).glyph(), 'i');
    }

    #[test]
    #[cfg(feature = "egc")]
    fn render_wrapped_respects_padding_and_border_like_render() {
        let grid = BoxStyle::new(Style::default())
            .border(true)
            .padding(Sides::all(1))
            .width(3)
            .render_wrapped("hi");
        // 3 content cols + 2 padding + 2 border = 7; 1 content row + 2
        // padding + 2 border = 5.
        assert_eq!((grid.width(), grid.height()), (7, 5));
        assert_eq!(grid.get(2, 2).glyph(), 'h');
        assert_eq!(grid.get(3, 2).glyph(), 'i');
    }

    #[test]
    fn boxed_widget_places_the_box_at_the_areas_top_left() {
        use retroglyph_core::Headless;

        let styled = BoxStyle::new(Style::default()).border(true).text("hi");
        let mut term = Terminal::new(Headless::new(10, 6));
        styled.render(Rect::new(2, 1, 10, 6), &mut term);

        // 2 content cols + 2 border = 4 wide, 1 content row + 2 border = 3
        // tall, anchored at (2, 1) regardless of the much larger area.
        assert_eq!(term.grid().get(2, 1).glyph(), '┌');
        assert_eq!(term.grid().get(3, 2).glyph(), 'h');
        assert_eq!(term.grid().get(4, 2).glyph(), 'i');
        assert_eq!(term.grid().get(5, 3).glyph(), '┘');
    }
}
