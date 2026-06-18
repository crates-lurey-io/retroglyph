//! Styled text primitives: [`Span`] and [`Line`].

use crate::style::Style;
use alloc::string::String;
use alloc::vec::Vec;
use unicode_width::UnicodeWidthStr;

/// A string with an associated [`Style`].
///
/// The building block of styled terminal output. A [`Line`] is composed of
/// one or more `Span`s, each with its own style.
///
/// # Examples
///
/// ```
/// use rg::text::Span;
/// use rg::style::Style;
/// use rg::color::Color;
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
        self.content.as_str().width()
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
/// use rg::text::{Line, Span};
/// use rg::style::Style;
/// use rg::color::Color;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;

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
