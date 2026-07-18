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
    ///
    /// `Color::Default` in `other` means "unset", not "reset to default": a field left at
    /// `Color::Default` is skipped, and `self`'s existing value for that field is kept. This
    /// mirrors ratatui's `Style::patch` convention, so `Style::new().fg(Color::Default)` is a
    /// no-op when patched onto anything, and there is no way to use `patch` to explicitly clear a
    /// field back to `Color::Default` -- use [`Style::reset_fg`] or [`Style::reset_bg`] for that.
    ///
    /// ```
    /// use retroglyph_core::{Color, Style};
    ///
    /// let base = Style::new().fg(Color::RED).bg(Color::BLUE);
    ///
    /// // Patching with a default `fg` leaves `base`'s red foreground untouched.
    /// let patched = base.patch(Style::new().bg(Color::GREEN));
    /// assert_eq!(patched.foreground(), Color::RED);
    /// assert_eq!(patched.background(), Color::GREEN);
    /// ```
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

    /// Resets the foreground color to `Color::Default`.
    ///
    /// Unlike [`Style::patch`], which treats `Color::Default` as "leave unset", this explicitly
    /// clears the field. Use this when a caller needs to undo a previously patched-in foreground
    /// color rather than merge in a new one.
    #[must_use]
    pub const fn reset_fg(mut self) -> Self {
        self.fg = Color::Default;
        self
    }

    /// Resets the background color to `Color::Default`.
    ///
    /// Unlike [`Style::patch`], which treats `Color::Default` as "leave unset", this explicitly
    /// clears the field. Use this when a caller needs to undo a previously patched-in background
    /// color rather than merge in a new one.
    #[must_use]
    pub const fn reset_bg(mut self) -> Self {
        self.bg = Color::Default;
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

    #[test]
    fn test_patch_cannot_reset_a_field_to_default() {
        let base = Style::new().fg(Color::RED).bg(Color::BLUE);
        let patched = base.patch(Style::new());
        assert_eq!(patched.foreground(), Color::RED);
        assert_eq!(patched.background(), Color::BLUE);
    }

    #[test]
    fn test_reset_fg_and_reset_bg_clear_to_default() {
        let s = Style::new().fg(Color::RED).bg(Color::BLUE);
        assert_eq!(s.reset_fg().foreground(), Color::Default);
        assert_eq!(s.reset_bg().background(), Color::Default);
    }
}
