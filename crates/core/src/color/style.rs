//! Text styling: foreground and background color.

use super::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// A style consisting of foreground and background color.
///
/// # Examples
///
/// ```
/// use retroglyph_core::color::{Color, Style};
///
/// let style = Style::new().fg(Color::GREEN).bg(Color::BLACK);
/// assert_eq!(style.foreground(), Color::GREEN);
/// assert_eq!(style.background(), Color::BLACK);
/// ```
#[doc(alias = "attr")] // curses-family attribute sets
pub struct Style {
    /// Foreground color.
    ///
    /// Colors the cell's glyph. A cell that a pixel backend draws as a *sprite* is the one
    /// exception: a sprite is composited from its own pixels and `fg` does not tint it. See
    /// [`Surface::put_span`](crate::surface::Surface::put_span).
    pub(crate) fg: Color,
    /// Background color.
    ///
    /// Fills the cell behind the glyph. Behind a sprite it is still painted, so it shows through
    /// the sprite's transparent pixels.
    pub(crate) bg: Color,
}

impl Style {
    /// Creates a new style with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the foreground color, which colors the cell's glyph.
    ///
    /// Does not tint a sprite: on a pixel backend, a cell whose glyph resolves to a sprite is
    /// composited from the sprite's own pixels and ignores this color entirely. The same cell
    /// drawn by a cell backend falls back to its glyph and *is* colored by it, so one value can
    /// read very differently across backends. See
    /// [`Surface::put_span`](crate::surface::Surface::put_span).
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
    /// field back to `Color::Default`; use [`Style::reset_fg`](crate::color::Style::reset_fg) or [`Style::reset_bg`](crate::color::Style::reset_bg) for that.
    ///
    /// ```
    /// use retroglyph_core::color::{Color, Style};
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
    /// Unlike [`Style::patch`](crate::color::Style::patch), which treats `Color::Default` as "leave unset", this explicitly
    /// clears the field. Use this when a caller needs to undo a previously patched-in foreground
    /// color rather than merge in a new one.
    #[must_use]
    pub const fn reset_fg(mut self) -> Self {
        self.fg = Color::Default;
        self
    }

    /// Resets the background color to `Color::Default`.
    ///
    /// Unlike [`Style::patch`](crate::color::Style::patch), which treats `Color::Default` as "leave unset", this explicitly
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

    #[cfg(feature = "serde")]
    #[test]
    fn test_serializes_and_deserializes() {
        let style = Style::new().fg(Color::RED).bg(Color::Indexed(200));
        let json = serde_json::to_string(&style).expect("serialize");
        assert_eq!(json, r#"{"fg":"red","bg":"200"}"#);
        assert_eq!(
            serde_json::from_str::<Style>(&json).expect("deserialize"),
            style
        );
    }
}
