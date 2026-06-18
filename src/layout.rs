//! Text layout: measurement, word wrapping, and bounded alignment.
//!
//! The entry point is [`TextLayout`], a builder that accepts a [`Line`] and
//! layout parameters, then either measures the result or renders it into a
//! [`Terminal`].
//!
//! Only available when the `egc` feature is enabled (requires `alloc`).

use crate::backend::Backend;
use crate::grid::Rect;
use crate::style::Style;
use crate::terminal::Terminal;
use crate::text::Line;
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
                    // Drop the space that triggered this break — it would just be
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

// ---------------------------------------------------------------------------
// TextLayout builder (M4)
// ---------------------------------------------------------------------------

/// Builder for laying out a [`Line`] within a bounded [`Rect`].
///
/// Call [`measure`](TextLayout::measure) to get [`TextMetrics`] without
/// touching any terminal, or [`render`](TextLayout::render) to write directly
/// into a [`Terminal`].
///
/// # Example
///
/// ```
/// use rg::layout::{TextLayout, HAlign, VAlign};
/// use rg::grid::Rect;
/// use rg::text::Line;
///
/// let rect = Rect { x: 0, y: 0, width: 20, height: 5 };
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
    /// [`render`](Self::render).
    #[must_use]
    pub const fn new(line: &'a Line) -> Self {
        Self {
            line,
            rect: Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
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
        let lines = wrap_line(self.line, self.rect.width);
        let width = lines.iter().map(|l| l.width).max().unwrap_or(0);
        #[allow(clippy::cast_possible_truncation)]
        let height = lines.len().min(u16::MAX as usize) as u16;
        TextMetrics { width, height }
    }

    /// Renders the text into `terminal`, clipping to the rect's bounds.
    pub fn render<B: Backend>(&self, terminal: &mut Terminal<B>) {
        let lines = wrap_line(self.line, self.rect.width);
        let rect = self.rect;

        #[allow(clippy::cast_possible_truncation)]
        let total_lines = lines.len().min(rect.height as usize) as u16;

        let y_offset = match self.v_align {
            VAlign::Top => 0,
            VAlign::Middle => rect.height.saturating_sub(total_lines) / 2,
            VAlign::Bottom => rect.height.saturating_sub(total_lines),
        };

        for (line_idx, wrapped) in lines.into_iter().take(total_lines as usize).enumerate() {
            let x_offset = match self.h_align {
                HAlign::Left => 0,
                HAlign::Center => rect.width.saturating_sub(wrapped.width) / 2,
                HAlign::Right => rect.width.saturating_sub(wrapped.width),
            };

            #[allow(clippy::cast_possible_truncation)]
            let row = rect.y + y_offset + line_idx as u16;
            let mut cx = rect.x + x_offset;

            for glyph in wrapped.glyphs {
                if cx >= rect.x + rect.width {
                    break;
                }
                terminal
                    .grid_mut()
                    .write_grapheme(cx, row, &glyph.grapheme, glyph.style);
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
        assert_eq!(lines[0].width, 5); // "hello" — space consumed
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
            .rect(Rect {
                x: 0,
                y: 0,
                width: 20,
                height: 5,
            })
            .measure();
        assert_eq!(m.width, 5);
        assert_eq!(m.height, 1);
    }

    #[test]
    fn test_measure_wraps() {
        let line = Line::raw("hello world");
        let m = TextLayout::new(&line)
            .rect(Rect {
                x: 0,
                y: 0,
                width: 7,
                height: 10,
            })
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
            .rect(Rect {
                x: 2,
                y: 1,
                width: 10,
                height: 3,
            })
            .render(&mut term);

        assert_eq!(term.grid().get(2, 1).glyph(), 'h');
        assert_eq!(term.grid().get(3, 1).glyph(), 'i');
        assert_eq!(term.grid().get(4, 1).glyph(), ' '); // unchanged
    }

    #[test]
    fn test_render_center_h() {
        use crate::backend::Headless;
        use crate::terminal::Terminal;

        // "hi" (width 2) centred in a 10-wide box: x_offset = (10-2)/2 = 4
        let mut term = Terminal::new(Headless::new(20, 5));
        let line = Line::raw("hi");
        TextLayout::new(&line)
            .rect(Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 3,
            })
            .h_align(HAlign::Center)
            .render(&mut term);

        assert_eq!(term.grid().get(4, 0).glyph(), 'h');
        assert_eq!(term.grid().get(5, 0).glyph(), 'i');
    }

    #[test]
    fn test_render_right_h() {
        use crate::backend::Headless;
        use crate::terminal::Terminal;

        // "hi" right-aligned in 10 columns: starts at col 8.
        let mut term = Terminal::new(Headless::new(20, 5));
        let line = Line::raw("hi");
        TextLayout::new(&line)
            .rect(Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 3,
            })
            .h_align(HAlign::Right)
            .render(&mut term);

        assert_eq!(term.grid().get(8, 0).glyph(), 'h');
        assert_eq!(term.grid().get(9, 0).glyph(), 'i');
    }

    #[test]
    fn test_render_middle_v() {
        use crate::backend::Headless;
        use crate::terminal::Terminal;

        // 1 line of text, 5-row box: y_offset = (5-1)/2 = 2
        let mut term = Terminal::new(Headless::new(20, 10));
        let line = Line::raw("hi");
        TextLayout::new(&line)
            .rect(Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 5,
            })
            .v_align(VAlign::Middle)
            .render(&mut term);

        assert_eq!(term.grid().get(0, 2).glyph(), 'h');
    }

    #[test]
    fn test_render_bottom_v() {
        use crate::backend::Headless;
        use crate::terminal::Terminal;

        // 1 line in a 5-row box bottom-aligned: row 4.
        let mut term = Terminal::new(Headless::new(20, 10));
        let line = Line::raw("hi");
        TextLayout::new(&line)
            .rect(Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 5,
            })
            .v_align(VAlign::Bottom)
            .render(&mut term);

        assert_eq!(term.grid().get(0, 4).glyph(), 'h');
    }

    #[test]
    fn test_render_clips_to_height() {
        use crate::backend::Headless;
        use crate::terminal::Terminal;

        // "a b c" wraps to 3 lines in a 1-wide box; height=2 clips to 2.
        let mut term = Terminal::new(Headless::new(10, 10));
        let line = Line::raw("a b c");
        TextLayout::new(&line)
            .rect(Rect {
                x: 0,
                y: 0,
                width: 1,
                height: 2,
            })
            .render(&mut term);

        assert_eq!(term.grid().get(0, 0).glyph(), 'a');
        assert_eq!(term.grid().get(0, 1).glyph(), 'b');
        assert_eq!(term.grid().get(0, 2).glyph(), ' '); // clipped
    }
}
