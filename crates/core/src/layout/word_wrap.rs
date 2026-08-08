//! Greedy grapheme-cluster-aware word-wrap engine, shared by [`super::TextLayout`] and [`wrap`].

use crate::color::Style;
use crate::text::{Line, Span};
use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// One grapheme on a wrapped line, ready to be placed or measured.
pub(super) struct WrappedGlyph {
    /// The grapheme cluster string.
    pub(super) grapheme: String,
    /// Style inherited from the source span.
    pub(super) style: Style,
    /// Display width of this grapheme in terminal columns (1 or 2).
    pub(super) width: u16,
}

/// A line produced by the word-wrap pass.
pub(super) struct WrappedLine {
    pub(super) glyphs: Vec<WrappedGlyph>,
    /// Sum of all glyph widths on this line.
    pub(super) width: u16,
}

/// Greedy word-wrap over a [`Line`](crate::text::Line)'s spans.
///
/// Breaks on ASCII space (`' '`): the space is consumed (not placed) at the
/// break point, and overlong words are force-broken at the column boundary.
/// Leading whitespace on soft-wrapped continuation lines is preserved.
///
/// Note: only `\n` and ASCII space are treated specially. Tabs, NBSP, and
/// other whitespace are treated as printable 1-wide characters. Callers
/// should expand tabs before calling if that matters.
pub(super) fn wrap_line(line: &Line, max_width: u16) -> Vec<WrappedLine> {
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

/// Word-wraps `line` to `max_width` columns, returning the broken-apart [`Line`](crate::text::Line)s.
///
/// This is the same greedy, grapheme-cluster-aware wrap pass [`TextLayout`](super::TextLayout)
/// runs internally on every render (breaking on ASCII space, honoring hard `\n`s, force-breaking
/// an overlong word at the column boundary); it's exposed standalone for callers that need the
/// wrapped pieces themselves rather than having them written straight to a surface, such as a
/// scrollback log that wraps each message into rows while still addressing its window in whole
/// messages.
///
/// Each returned `Line` is a single unstyled or uniformly-styled run per source span that
/// survived onto that row; adjacent graphemes carrying the same [`Style`](crate::color::Style) are coalesced back
/// into one [`Span`](crate::text::Span), so wrapping a plain [`Line::raw`](crate::text::Line::raw) round-trips to plain `Line::raw` rows.
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

/// Word-wraps plain `text` to `max_width` columns, returning the broken-apart rows.
///
/// This runs the same greedy, grapheme-cluster-aware wrap pass as [`wrap`] (breaking on ASCII
/// space, honoring hard `\n`s, force-breaking an overlong word at the column boundary), for
/// callers that just have a plain string and would otherwise hand-roll a wrap loop to get it.
/// Reach for [`wrap`] instead when the text carries per-[`Span`](crate::text::Span)
/// [`Style`](crate::color::Style): `text: &str` converts to a plain [`Line`] via `.into()`, so
/// `wrap(&text.into(), w)` covers that case without constructing spans by hand.
///
/// Each row is grapheme-rejoined during wrapping, so this always returns
/// [`Cow::Owned`](alloc::borrow::Cow::Owned); the `Cow` return type is kept so a row that turns
/// out not to need wrapping can borrow `text` directly if a future revision adds that
/// fast path.
///
/// # Examples
///
/// ```
/// use retroglyph_core::layout::wrap_str;
///
/// let rows = wrap_str("hello world", 7);
/// assert_eq!(rows, vec!["hello", "world"]);
/// ```
#[must_use]
pub fn wrap_str(text: &str, max_width: u16) -> Vec<Cow<'_, str>> {
    wrap_line(&Line::raw(text), max_width)
        .into_iter()
        .map(|wrapped| {
            let mut row = String::new();
            for glyph in wrapped.glyphs {
                row.push_str(&glyph.grapheme);
            }
            Cow::Owned(row)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;

    fn red() -> Style {
        Style::new().fg(Color::RED)
    }

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
    fn test_wrap_force_break_drops_the_triggering_space() {
        // "abcd" fills the 4-wide box exactly; the following space has no room and no
        // earlier space on the line to break at, so it force-breaks and is itself dropped
        // rather than becoming leading whitespace on the new line.
        let line = Line::raw("abcd e");
        let lines = wrap_line(&line, 4);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].width, 4);
        assert_eq!(lines[1].width, 1); // "e", not " e"
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

    #[test]
    fn test_wrap_str_soft_break_on_space() {
        let rows = wrap_str("hello world", 7);
        assert_eq!(rows, alloc::vec!["hello", "world"]);
    }

    #[test]
    fn test_wrap_str_hard_newline() {
        let rows = wrap_str("hi\nthere", 20);
        assert_eq!(rows, alloc::vec!["hi", "there"]);
    }

    #[test]
    fn test_wrap_str_wide_chars() {
        // Each CJK char is width 2; "中文中" in a 4-wide box wraps after "中文".
        let rows = wrap_str("中文中", 4);
        assert_eq!(rows, alloc::vec!["中文", "中"]);
    }

    #[test]
    fn test_wrap_str_no_wrap_needed_returns_owned() {
        let rows = wrap_str("hello", 10);
        assert_eq!(rows.len(), 1);
        assert!(matches!(rows[0], Cow::Owned(_)));
    }
}
