//! Styled text primitives: [`Span`] and [`Line`].

use crate::style::Style;
use alloc::string::String;
use alloc::vec::Vec;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// The number of terminal cells `s` occupies, saturating at `u16::MAX`.
///
/// Wide characters (CJK, most emoji) count as two columns; combining marks
/// and most control characters count as zero. [`Span::width`] and
/// [`Line::width`] are built on this function; reach for it directly to
/// measure a borrowed `&str` without constructing either type first.
///
/// See [`width_usize`] for the unsaturated measurement.
///
/// # Examples
///
/// ```
/// use retroglyph_core::text::width;
///
/// assert_eq!(width("hello"), 5);
/// assert_eq!(width("中文"), 4); // each CJK char is 2 columns
/// ```
#[must_use]
pub fn width(s: &str) -> u16 {
    #[allow(clippy::cast_possible_truncation)] // clamped to u16::MAX above
    let w = width_usize(s).min(usize::from(u16::MAX)) as u16;
    w
}

/// The number of terminal cells `s` occupies, without saturating to `u16`.
///
/// Prefer [`width`] when the result feeds a `u16`-based geometry type such
/// as `Rect`/`Size`; use this when the raw `unicode-width` measurement is
/// needed instead.
#[must_use]
pub fn width_usize(s: &str) -> usize {
    s.width()
}

/// The number of terminal cells a single character occupies.
///
/// Returns `0` for combining marks and most control characters, `1` for
/// most characters, and `2` for wide characters (CJK, most emoji).
///
/// # Examples
///
/// ```
/// use retroglyph_core::text::char_width;
///
/// assert_eq!(char_width('a'), 1);
/// assert_eq!(char_width('中'), 2);
/// ```
#[must_use]
pub fn char_width(c: char) -> u16 {
    #[allow(clippy::cast_possible_truncation)] // unicode-width never returns > 2
    let w = c.width().unwrap_or(0) as u16;
    w
}

/// Splits `s` at the byte index where its display width reaches `max_cols`.
///
/// Splits on a whole-character boundary; a character that would push the
/// total over `max_cols` is left in the second half along with the rest of
/// `s`. Returns `(prefix, rest)` such that `width(prefix) <= max_cols` and
/// `prefix` is the longest prefix of `s` for which that holds.
///
/// Each candidate prefix is measured with [`width_usize`] (the same
/// `UnicodeWidthStr` logic behind the postcondition above), not a sum of
/// individual [`char_width`]s: `UnicodeWidthStr` measures some multi-codepoint
/// clusters (emoji presentation/ZWJ/modifier sequences) as a unit whose width
/// differs from the sum of its parts, and per-char summing would let such a
/// cluster violate the postcondition in either direction.
///
/// # Examples
///
/// ```
/// use retroglyph_core::text::split_at_width;
///
/// assert_eq!(split_at_width("hello world", 5), ("hello", " world"));
/// assert_eq!(split_at_width("hi", 10), ("hi", ""));
/// ```
#[must_use]
pub fn split_at_width(s: &str, max_cols: u16) -> (&str, &str) {
    let max_cols = usize::from(max_cols);
    let mut end = 0usize;
    for (i, ch) in s.char_indices() {
        let candidate_end = i + ch.len_utf8();
        if width_usize(&s[..candidate_end]) > max_cols {
            break;
        }
        end = candidate_end;
    }
    s.split_at(end)
}

/// A string with an associated [`Style`].
///
/// The building block of styled terminal output. A [`Line`] is composed of
/// one or more `Span`s, each with its own style.
///
/// # Examples
///
/// ```
/// use retroglyph_core::text::Span;
/// use retroglyph_core::style::Style;
/// use retroglyph_core::color::Color;
///
/// let plain = Span::raw("hello");
/// let colored = Span::styled("world", Style::new().fg(Color::GREEN));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Span {
    /// The text content.
    pub content: String,
    /// The style applied to this span.
    pub style: Style,
}

impl Span {
    /// Creates a span with the given content and no styling.
    #[must_use]
    pub fn raw(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            style: Style::default(),
        }
    }

    /// Creates a span with the given content and style.
    #[must_use]
    pub fn styled(content: impl Into<String>, style: Style) -> Self {
        Self {
            content: content.into(),
            style,
        }
    }

    /// Returns the display width of this span in terminal columns.
    #[must_use]
    pub fn width(&self) -> usize {
        width_usize(&self.content)
    }
}

impl<S: Into<String>> From<S> for Span {
    fn from(s: S) -> Self {
        Self::raw(s)
    }
}

/// A horizontal sequence of [`Span`]s rendered as a single line.
///
/// # Examples
///
/// ```
/// use retroglyph_core::text::{Line, Span};
/// use retroglyph_core::style::Style;
/// use retroglyph_core::color::Color;
///
/// let line = Line::from(vec![
///     Span::raw("HP: "),
///     Span::styled("100", Style::new().fg(Color::GREEN)),
/// ]);
/// assert_eq!(line.width(), 7);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Line {
    /// The spans that make up this line.
    pub spans: Vec<Span>,
}

impl Line {
    /// Creates an empty line.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a line from a single unstyled string.
    #[must_use]
    pub fn raw(content: impl Into<String>) -> Self {
        Self {
            spans: alloc::vec![Span::raw(content)],
        }
    }

    /// Returns the total display width of this line in terminal columns.
    ///
    /// Accounts for wide characters (CJK, emoji) that occupy two columns.
    #[must_use]
    pub fn width(&self) -> usize {
        self.spans.iter().map(Span::width).sum()
    }
}

impl From<&str> for Line {
    fn from(s: &str) -> Self {
        Self::raw(s)
    }
}

impl From<String> for Line {
    fn from(s: String) -> Self {
        Self::raw(s)
    }
}

impl From<Span> for Line {
    fn from(span: Span) -> Self {
        Self {
            spans: alloc::vec![span],
        }
    }
}

impl From<Vec<Span>> for Line {
    fn from(spans: Vec<Span>) -> Self {
        Self { spans }
    }
}

/// Build a [`Line`] from a list of `(Style, text)` pairs.
///
/// Each pair becomes a [`Span`] with the given style. The resulting `Line`
/// is equivalent to calling `Line::from(vec![Span::styled(t, s), ...])` but
/// with less boilerplate for multi-segment event-log text.
///
/// # Examples
///
/// ```
/// # extern crate alloc;
/// use retroglyph_core::spans;
/// use retroglyph_core::style::Style;
/// use retroglyph_core::color::Color;
///
/// let line = spans![
///     (Style::new().fg(Color::CYAN), "snowtroop "),
///     (Style::default(), "→ 1 hit"),
/// ];
/// assert_eq!(line.width(), 17);
/// ```
#[macro_export]
macro_rules! spans {
    ($(($style:expr, $text:expr)),* $(,)?) => {
        $crate::text::Line::from(alloc::vec![
            $($crate::text::Span::styled($text, $style)),*
        ])
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;

    #[test]
    fn width_matches_span_width() {
        assert_eq!(width("hello"), 5);
        assert_eq!(width(""), 0);
    }

    #[test]
    fn width_counts_wide_characters_as_two_columns() {
        assert_eq!(width("中文"), 4);
    }

    #[test]
    fn width_saturates_at_u16_max() {
        let s = "a".repeat(usize::from(u16::MAX) + 100);
        assert_eq!(width(&s), u16::MAX);
        assert_eq!(width_usize(&s), s.len());
    }

    #[test]
    fn char_width_matches_unicode_width() {
        assert_eq!(char_width('a'), 1);
        assert_eq!(char_width('中'), 2);
        assert_eq!(char_width('\u{0301}'), 0); // combining acute accent
    }

    #[test]
    fn split_at_width_stops_at_the_column_budget() {
        assert_eq!(split_at_width("hello world", 5), ("hello", " world"));
        assert_eq!(split_at_width("hi", 10), ("hi", ""));
        assert_eq!(split_at_width("hi", 0), ("", "hi"));
    }

    #[test]
    fn split_at_width_counts_wide_characters_as_two_columns() {
        assert_eq!(split_at_width("aあb", 2), ("a", "あb"));
        assert_eq!(split_at_width("aあb", 3), ("aあ", "b"));
        assert_eq!(split_at_width("ああ", 3), ("あ", "あ"));
    }

    #[test]
    fn split_at_width_prefix_can_exceed_max_cols() {
        // "❤️" (U+2764 heart + U+FE0F variation selector-16): unicode-width
        // measures the pair as 2 columns even though the per-char widths are
        // 1 + 0 = 1, so the whole cluster must not slip through a 1-column
        // budget.
        let (prefix, _rest) = split_at_width("\u{2764}\u{FE0F}", 1);
        assert!(width(prefix) <= 1);
    }

    #[test]
    fn split_at_width_returns_the_longest_prefix_that_fits() {
        // "👍🏽" (U+1F44D thumbs-up + U+1F3FD skin-tone modifier): unicode-width
        // measures the pair as 2 columns, so the whole string fits a 2-column
        // budget intact, even though the per-char widths sum to 2 + 2 = 4.
        let thumbs = "\u{1F44D}\u{1F3FD}";
        assert_eq!(split_at_width(thumbs, width(thumbs)), (thumbs, ""));
    }

    #[test]
    fn test_span_raw() {
        let s = Span::raw("hello");
        assert_eq!(s.content, "hello");
        assert_eq!(s.style, Style::default());
        assert_eq!(s.width(), 5);
    }

    #[test]
    fn test_span_styled() {
        let style = Style::new().fg(Color::RED);
        let s = Span::styled("hi", style);
        assert_eq!(s.content, "hi");
        assert_eq!(s.style, style);
    }

    #[test]
    fn test_span_width_wide_chars() {
        let s = Span::raw("中文"); // each CJK char is 2 columns
        assert_eq!(s.width(), 4);
    }

    #[test]
    fn test_line_from_str() {
        let line = Line::from("hello");
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.width(), 5);
    }

    #[test]
    fn test_line_from_spans() {
        let line = Line::from(vec![
            Span::raw("HP: "),
            Span::styled("100", Style::new().fg(Color::GREEN)),
        ]);
        assert_eq!(line.width(), 7);
    }

    #[test]
    fn test_line_width_wide_chars() {
        let line = Line::from(vec![Span::raw("中"), Span::raw("x")]);
        assert_eq!(line.width(), 3); // 2 + 1
    }

    #[test]
    fn test_line_empty() {
        let line = Line::new();
        assert_eq!(line.width(), 0);
    }
}
