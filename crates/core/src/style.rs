//! Text styling: foreground and background color.

use crate::color::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
/// A style consisting of foreground and background color.
///
/// No text modifiers (bold, italic, underline, etc.) by design -- retroglyph is a spiritual
/// remake of `BearLibTerminal`, which doesn't support them either. A pixel/bitmap-font renderer
/// can't fake most of them (no bold font variant, no underline stroke) without real per-style
/// assets, so rather than have them work in a real terminal and silently do nothing in the
/// software backend, they're not part of the API at all. Color and glyph choice are the only two
/// knobs, in every backend.
pub struct Style {
    /// Foreground color.
    pub(crate) fg: Color,
    /// Background color.
    pub(crate) bg: Color,
}

impl Style {
    /// Creates a new style with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the foreground color.
    #[must_use]
    pub const fn fg(mut self, color: Color) -> Self {
        self.fg = color;
        self
    }

    /// Sets the background color.
    #[must_use]
    pub const fn bg(mut self, color: Color) -> Self {
        self.bg = color;
        self
    }

    /// Returns the foreground color.
    #[must_use]
    pub const fn foreground(&self) -> Color {
        self.fg
    }

    /// Returns the background color.
    #[must_use]
    pub const fn background(&self) -> Color {
        self.bg
    }

    /// Overlays another style onto this one, only if fields in `other` are non-default.
    #[must_use]
    pub fn patch(mut self, other: Self) -> Self {
        if other.fg != Color::Default {
            self.fg = other.fg;
        }
        if other.bg != Color::Default {
            self.bg = other.bg;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_style_builder() {
        let s = Style::new().fg(Color::RED).bg(Color::BLUE);
        assert_eq!(s.foreground(), Color::RED);
        assert_eq!(s.background(), Color::BLUE);
    }

    #[test]
    fn test_patch_keeps_non_default_fields() {
        let base = Style::new().fg(Color::RED).bg(Color::BLUE);
        let patched = base.patch(Style::new().fg(Color::GREEN));
        assert_eq!(patched.foreground(), Color::GREEN);
        assert_eq!(patched.background(), Color::BLUE);
    }
}
