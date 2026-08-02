//! Text layout: measurement, word wrapping, and bounded alignment.
//!
//! [`crate::text::Span`] and [`Line`] provide styled text primitives. The entry point here is
//! [`TextLayout`], a builder that word-wraps a [`Line`] to a bounded [`Rect`], then positions it
//! with independent horizontal ([`HAlign`]) and vertical ([`VAlign`]) alignment. Measure the
//! result before rendering with [`TextLayout::measure`]. [`wrap`] exposes that same word-wrap
//! pass standalone, for callers that need the broken-apart [`Line`]s rather than a rendered
//! surface.
//!
//! Only available when the `egc` feature is enabled (requires `alloc`).

use crate::grid::{Grid, Rect};
use crate::style::Style;
use crate::surface::Surface;
use crate::text::{Line, Span};
use alloc::string::String;
use alloc::vec::Vec;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Horizontal alignment within a bounded rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum HAlign {
    /// Align text to the left edge (default).
    #[default]
    Left,
    /// Centre text horizontally.
    Center,
    /// Align text to the right edge.
    Right,
}

/// Vertical alignment within a bounded rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VAlign {
    /// Align text to the top edge (default).
    #[default]
    Top,
    /// Centre text vertically.
    Middle,
    /// Align text to the bottom edge.
    Bottom,
}

/// The display dimensions of a laid-out block of text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TextMetrics {
    /// Maximum line width in terminal columns.
    pub width: u16,
    /// Number of lines after word-wrapping.
    pub height: u16,
}

// ---------------------------------------------------------------------------
// Internal intermediate types
// ---------------------------------------------------------------------------

/// One grapheme on a wrapped line, ready to be placed or measured.
struct WrappedGlyph {
    /// The grapheme cluster string.
    grapheme: String,
    /// Style inherited from the source span.
    style: Style,
    /// Display width of this grapheme in terminal columns (1 or 2).
    width: u16,
}

/// A line produced by the word-wrap pass.
struct WrappedLine {
    glyphs: Vec<WrappedGlyph>,
    /// Sum of all glyph widths on this line.
    width: u16,
}

// ---------------------------------------------------------------------------
// Word-wrap engine (M3)
// ---------------------------------------------------------------------------

/// Greedy word-wrap over a [`Line`]'s spans.
///
/// Breaks on ASCII space (`' '`): the space is consumed (not placed) at the
/// break point, and overlong words are force-broken at the column boundary.
/// Leading whitespace on soft-wrapped continuation lines is preserved.
///
/// Note: only `\n` and ASCII space are treated specially. Tabs, NBSP, and
/// other whitespace are treated as printable 1-wide characters. Callers
/// should expand tabs before calling if that matters.
fn wrap_line(line: &Line, max_width: u16) -> Vec<WrappedLine> {
    let mut lines: Vec<WrappedLine> = alloc::vec![WrappedLine {
        glyphs: Vec::new(),
        width: 0,
    }];
    let mut col: u16 = 0;

    for span in &line.spans {
        for grapheme in span.content.graphemes(true) {
            // Hard newline.
            if grapheme == "\n" {
                lines.push(WrappedLine {
                    glyphs: Vec::new(),
                    width: 0,
                });
                col = 0;
                continue;
            }

            #[allow(clippy::cast_possible_truncation)]
            let gw = grapheme.width() as u16;
            if gw == 0 {
                continue; // zero-width (combining handled in write_grapheme)
            }

            // Soft wrap: this grapheme would overflow the line.
            if col + gw > max_width && col > 0 {
                let current = lines.last_mut().expect("always at least one line");

                // Try to break at the last space on the current line.
                if let Some(space_idx) = current.glyphs.iter().rposition(|g| g.grapheme == " ") {
                    // Drain everything after the space into a new line.
                    let remainder: Vec<WrappedGlyph> =
                        current.glyphs.drain(space_idx + 1..).collect();
                    // Drop the space itself.
                    current.glyphs.pop();
                    current.width = current.glyphs.iter().map(|g| g.width).sum();

                    let new_width: u16 = remainder.iter().map(|g| g.width).sum();
                    // col will be incremented by gw in the fall-through below.
                    col = new_width;
                    lines.push(WrappedLine {
                        glyphs: remainder,
                        width: new_width,
                    });
                } else {
                    // No space on the line: force-break (overlong word).
                    lines.push(WrappedLine {
                        glyphs: Vec::new(),
                        width: 0,
                    });
                    col = 0;
                    // Drop the space that triggered this break: it would just be
                    // leading whitespace on the new line.
                    if grapheme == " " {
                        continue;
                    }
                }
            }

            let current = lines.last_mut().expect("always at least one line");
            current.width += gw;
            current.glyphs.push(WrappedGlyph {
                grapheme: String::from(grapheme),
                style: span.style,
                width: gw,
            });
            col += gw;
        }
    }

    lines
}

/// Word-wraps `line` to `max_width` columns, returning the broken-apart [`Line`]s.
///
/// This is the same greedy, grapheme-cluster-aware wrap pass [`TextLayout`] runs internally on
/// every render (breaking on ASCII space, honoring hard `\n`s, force-breaking an overlong word
/// at the column boundary); it's exposed standalone for callers that need the wrapped pieces
/// themselves rather than having them written straight to a surface, such as a scrollback log
/// that wraps each message into rows while still addressing its window in whole messages.
///
/// Each returned `Line` is a single unstyled or uniformly-styled run per source span that
/// survived onto that row; adjacent graphemes carrying the same [`Style`] are coalesced back
/// into one [`Span`], so wrapping a plain [`Line::raw`] round-trips to plain `Line::raw` rows.
///
/// # Examples
///
/// ```
/// use retroglyph_core::layout::wrap;
/// use retroglyph_core::text::Line;
///
/// let line = Line::raw("hello world");
/// let rows = wrap(&line, 7);
/// assert_eq!(rows.len(), 2);
/// assert_eq!(rows[0].spans[0].content, "hello");
/// assert_eq!(rows[1].spans[0].content, "world");
/// ```
#[must_use]
pub fn wrap(line: &Line, max_width: u16) -> Vec<Line> {
    wrap_line(line, max_width)
        .into_iter()
        .map(|wrapped| {
            let mut spans: Vec<Span> = Vec::new();
            for glyph in wrapped.glyphs {
                if let Some(last) = spans.last_mut()
                    && last.style == glyph.style
                {
                    last.content.push_str(&glyph.grapheme);
                    continue;
                }
                spans.push(Span::styled(glyph.grapheme, glyph.style));
            }
            Line { spans }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// TextLayout builder (M4)
// ---------------------------------------------------------------------------

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
    /// area (the rect is intersected with [`Surface::area`] first, so text can never escape the
    /// surface it was given even if `rect` extends past it).
    pub fn render_to_surface(&self, surface: &mut Surface<'_>) {
        let clipped = Self {
            line: self.line,
            rect: self.rect.intersect(surface.area()),
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

        let y_offset = match self.v_align {
            VAlign::Top => 0,
            VAlign::Middle => rect.height().saturating_sub(total_lines) / 2,
            VAlign::Bottom => rect.height().saturating_sub(total_lines),
        };

        for (line_idx, wrapped) in lines.into_iter().take(total_lines as usize).enumerate() {
            let x_offset = match self.h_align {
                HAlign::Left => 0,
                HAlign::Center => rect.width().saturating_sub(wrapped.width) / 2,
                HAlign::Right => rect.width().saturating_sub(wrapped.width),
            };

            #[allow(clippy::cast_possible_truncation)]
            let row = rect.top() + y_offset + line_idx as u16;
            let mut cx = rect.left() + x_offset;

            for glyph in wrapped.glyphs {
                if cx >= rect.right() {
                    break;
                }
                grid.write_grapheme(layer, cx, row, &glyph.grapheme, glyph.style);
                cx += glyph.width;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::grid::Pos;
    use crate::style::Style;
    use crate::text::{Line, Span};

    fn red() -> Style {
        Style::new().fg(Color::RED)
    }

    // --- wrap_line ---

    #[test]
    fn test_wrap_no_wrap_needed() {
        let line = Line::raw("hello");
        let lines = wrap_line(&line, 10);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].width, 5);
    }

    #[test]
    fn test_wrap_hard_newline() {
        let line = Line::raw("hi\nthere");
        let lines = wrap_line(&line, 20);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].width, 2);
        assert_eq!(lines[1].width, 5);
    }

    #[test]
    fn test_wrap_soft_break_on_space() {
        // "hello world" in a 7-wide box: "hello" fits, space triggers break.
        let line = Line::raw("hello world");
        let lines = wrap_line(&line, 7);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].width, 5); // "hello", space consumed
        assert_eq!(lines[1].width, 5); // "world"
    }

    #[test]
    fn test_wrap_force_break_no_space() {
        let line = Line::raw("abcdefgh");
        let lines = wrap_line(&line, 4);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].width, 4);
        assert_eq!(lines[1].width, 4);
    }

    #[test]
    fn test_wrap_wide_chars() {
        // Each CJK char is width 2; "中文中" in a 4-wide box wraps after "中文".
        let line = Line::raw("中文中");
        let lines = wrap_line(&line, 4);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].width, 4);
        assert_eq!(lines[1].width, 2);
    }

    #[test]
    fn test_wrap_multi_span() {
        let line = Line::from(vec![Span::raw("foo "), Span::styled("bar", red())]);
        let lines = wrap_line(&line, 20);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].width, 7);
        // The "bar" glyphs should carry the red style.
        let bar_count = lines[0].glyphs.iter().filter(|g| g.style == red()).count();
        assert_eq!(bar_count, 3);
    }

    // --- TextLayout::measure ---

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

    // --- TextLayout::render ---

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
}
