//! [`TextLayout`] builder: wraps a [`Line`] to a bounded [`Rect`] and positions it with
//! [`HAlign`]/[`VAlign`].

use super::align::{HAlign, VAlign};
use super::word_wrap::wrap_line;
use crate::grid::{Grid, Rect};
use crate::surface::Surface;
use crate::text::Line;

/// The display dimensions of a laid-out block of text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TextMetrics {
    /// Maximum line width in terminal columns.
    pub width: u16,
    /// Number of lines after word-wrapping.
    pub height: u16,
}

/// Builder for laying out a [`Line`] within a bounded [`Rect`].
///
/// Call [`measure`](TextLayout::measure) to get [`TextMetrics`] without
/// touching any surface, or [`render_to_surface`](TextLayout::render_to_surface) to write
/// directly into a [`Surface`].
///
/// # Examples
///
/// ```
/// use retroglyph_core::layout::{TextLayout, HAlign, VAlign};
/// use retroglyph_core::grid::Rect;
/// use retroglyph_core::text::Line;
///
/// let rect = Rect::new(0, 0, 20, 5);
/// let line = Line::raw("Hello, world!");
///
/// let metrics = TextLayout::new(&line)
///     .rect(rect)
///     .h_align(HAlign::Center)
///     .measure();
///
/// assert_eq!(metrics.height, 1);
/// ```
pub struct TextLayout<'a> {
    line: &'a Line,
    rect: Rect,
    h_align: HAlign,
    v_align: VAlign,
}

impl<'a> TextLayout<'a> {
    /// Creates a new layout builder for `line`.
    ///
    /// Defaults: zero-sized rect at origin, left/top alignment. Call
    /// [`rect`](Self::rect) before [`measure`](Self::measure) or
    /// [`render_to_surface`](Self::render_to_surface).
    #[must_use]
    pub const fn new(line: &'a Line) -> Self {
        Self {
            line,
            rect: Rect::EMPTY,
            h_align: HAlign::Left,
            v_align: VAlign::Top,
        }
    }

    /// Sets the bounding rectangle.
    #[must_use]
    pub const fn rect(mut self, rect: Rect) -> Self {
        self.rect = rect;
        self
    }

    /// Sets the horizontal alignment.
    #[must_use]
    pub const fn h_align(mut self, align: HAlign) -> Self {
        self.h_align = align;
        self
    }

    /// Sets the vertical alignment.
    #[must_use]
    pub const fn v_align(mut self, align: VAlign) -> Self {
        self.v_align = align;
        self
    }

    /// Measures the text without rendering, returning its [`TextMetrics`].
    ///
    /// Uses the rect's `width` for word-wrapping; ignores `height`.
    #[must_use]
    pub fn measure(&self) -> TextMetrics {
        let lines = wrap_line(self.line, self.rect.width());
        let width = lines.iter().map(|l| l.width).max().unwrap_or(0);
        #[allow(clippy::cast_possible_truncation)]
        let height = lines.len().min(u16::MAX as usize) as u16;
        TextMetrics { width, height }
    }

    /// Renders the text into `surface`, clipping to both the rect's bounds and `surface`'s own
    /// clip (the rect is intersected with [`Surface::clip_rect`] first, so text can never escape
    /// whatever clip the caller applied even if `rect` extends past it).
    pub fn render_to_surface(&self, surface: &mut Surface<'_>) {
        let clipped = Self {
            line: self.line,
            rect: self.rect.intersect(surface.clip_rect()),
            h_align: self.h_align,
            v_align: self.v_align,
        };
        let layer = surface.layer();
        clipped.render_to_grid(surface.grid_mut(), layer);
    }

    /// Renders the text into `grid` on `layer`, clipping to the rect's bounds.
    ///
    /// The [`Grid`]-level twin of [`render_to_surface`](Self::render_to_surface), for callers
    /// with no [`Surface`] of their own to hand over.
    pub fn render_to_grid(&self, grid: &mut Grid, layer: u8) {
        let lines = wrap_line(self.line, self.rect.width());
        let rect = self.rect;

        #[allow(clippy::cast_possible_truncation)]
        let total_lines = lines.len().min(usize::from(rect.height())) as u16;

        let y_offset = self.v_align.offset(rect.height(), total_lines);

        for (line_idx, wrapped) in lines.into_iter().take(total_lines as usize).enumerate() {
            let x_offset = self.h_align.offset(rect.width(), wrapped.width);

            #[allow(clippy::cast_possible_truncation)]
            let row = rect.top() + y_offset + line_idx as u16;
            let mut cx = rect.left() + x_offset;

            for glyph in wrapped.glyphs {
                if cx + glyph.width > rect.right() {
                    break;
                }
                grid.write_grapheme(layer, cx, row, &glyph.grapheme, glyph.style);
                cx += glyph.width;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Pos;
    use alloc::string::String;

    #[test]
    fn test_measure_single_line() {
        let line = Line::raw("hello");
        let m = TextLayout::new(&line)
            .rect(Rect::new(0, 0, 20, 5))
            .measure();
        assert_eq!(m.width, 5);
        assert_eq!(m.height, 1);
    }

    #[test]
    fn test_measure_wraps() {
        let line = Line::raw("hello world");
        let m = TextLayout::new(&line)
            .rect(Rect::new(0, 0, 7, 10))
            .measure();
        assert_eq!(m.height, 2);
        assert_eq!(m.width, 5);
    }

    #[test]
    fn test_render_left_top() {
        use crate::backend::Headless;
        use crate::terminal::Terminal;

        let mut term = Terminal::new(Headless::new(20, 5));
        let line = Line::raw("hi");
        TextLayout::new(&line)
            .rect(Rect::new(2, 1, 10, 3))
            .render_to_surface(&mut term.surface());

        assert_eq!(term.grid()[Pos::new(2, 1)].glyph(), 'h');
        assert_eq!(term.grid()[Pos::new(3, 1)].glyph(), 'i');
        assert_eq!(term.grid()[Pos::new(4, 1)].glyph(), ' '); // unchanged
    }

    #[test]
    fn test_render_center_h() {
        use crate::backend::Headless;
        use crate::terminal::Terminal;

        // "hi" (width 2) centred in a 10-wide box: x_offset = (10-2)/2 = 4
        let mut term = Terminal::new(Headless::new(20, 5));
        let line = Line::raw("hi");
        TextLayout::new(&line)
            .rect(Rect::new(0, 0, 10, 3))
            .h_align(HAlign::Center)
            .render_to_surface(&mut term.surface());

        assert_eq!(term.grid()[Pos::new(4, 0)].glyph(), 'h');
        assert_eq!(term.grid()[Pos::new(5, 0)].glyph(), 'i');
    }

    #[test]
    fn test_render_right_h() {
        use crate::backend::Headless;
        use crate::terminal::Terminal;

        // "hi" right-aligned in 10 columns: starts at col 8.
        let mut term = Terminal::new(Headless::new(20, 5));
        let line = Line::raw("hi");
        TextLayout::new(&line)
            .rect(Rect::new(0, 0, 10, 3))
            .h_align(HAlign::Right)
            .render_to_surface(&mut term.surface());

        assert_eq!(term.grid()[Pos::new(8, 0)].glyph(), 'h');
        assert_eq!(term.grid()[Pos::new(9, 0)].glyph(), 'i');
    }

    #[test]
    fn test_render_middle_v() {
        use crate::backend::Headless;
        use crate::terminal::Terminal;

        // 1 line of text, 5-row box: y_offset = (5-1)/2 = 2
        let mut term = Terminal::new(Headless::new(20, 10));
        let line = Line::raw("hi");
        TextLayout::new(&line)
            .rect(Rect::new(0, 0, 10, 5))
            .v_align(VAlign::Middle)
            .render_to_surface(&mut term.surface());

        assert_eq!(term.grid()[Pos::new(0, 2)].glyph(), 'h');
    }

    #[test]
    fn test_render_bottom_v() {
        use crate::backend::Headless;
        use crate::terminal::Terminal;

        // 1 line in a 5-row box bottom-aligned: row 4.
        let mut term = Terminal::new(Headless::new(20, 10));
        let line = Line::raw("hi");
        TextLayout::new(&line)
            .rect(Rect::new(0, 0, 10, 5))
            .v_align(VAlign::Bottom)
            .render_to_surface(&mut term.surface());

        assert_eq!(term.grid()[Pos::new(0, 4)].glyph(), 'h');
    }

    #[test]
    fn test_render_clips_to_height() {
        use crate::backend::Headless;
        use crate::terminal::Terminal;

        // "a b c" wraps to 3 lines in a 1-wide box; height=2 clips to 2.
        let mut term = Terminal::new(Headless::new(10, 10));
        let line = Line::raw("a b c");
        TextLayout::new(&line)
            .rect(Rect::new(0, 0, 1, 2))
            .render_to_surface(&mut term.surface());

        assert_eq!(term.grid()[Pos::new(0, 0)].glyph(), 'a');
        assert_eq!(term.grid()[Pos::new(0, 1)].glyph(), 'b');
        assert_eq!(term.grid()[Pos::new(0, 2)].glyph(), ' '); // clipped
    }

    #[test]
    fn text_layout_render_to_surface_escapes_the_surface_clip() {
        use crate::backend::Headless;
        use crate::terminal::Terminal;

        // The rect (10x4) extends well past the surface's one-row clip: "hello world"
        // wraps to "hello" / "world" at word boundaries, and the wrapped remainder
        // ("world") must not be painted on row 1, outside the clip.
        let mut term = Terminal::new(Headless::new(20, 5));
        {
            let mut surface = term.surface();
            let mut bar = surface.clip(Rect::new(0, 0, 10, 1));
            let line = Line::raw("hello world");
            TextLayout::new(&line)
                .rect(Rect::new(0, 0, 10, 4))
                .render_to_surface(&mut bar);
        }

        let row0: String = (0..10)
            .map(|x| term.grid()[Pos::new(x, 0)].glyph())
            .collect();
        assert_eq!(row0.trim_end(), "hello");
        for x in 0..10 {
            assert_eq!(term.grid()[Pos::new(x, 1)].glyph(), ' ');
        }
    }

    #[test]
    fn text_layout_wide_glyph_stays_inside_the_rect() {
        use crate::backend::Headless;
        use crate::terminal::Terminal;

        // A single wide (2-column) glyph in a rect one column too narrow for it: neither
        // the primary cell nor its spacer may be written, since the spacer would land at
        // column 1, outside the 1-wide rect.
        let mut term = Terminal::new(Headless::new(10, 5));
        let line = Line::raw("\u{3042}"); // 'あ', a wide CJK glyph
        TextLayout::new(&line)
            .rect(Rect::new(0, 0, 1, 1))
            .render_to_surface(&mut term.surface());

        assert_eq!(term.grid()[Pos::new(0, 0)].glyph(), ' ');
        assert_eq!(term.grid()[Pos::new(1, 0)].glyph(), ' ');
    }
}
