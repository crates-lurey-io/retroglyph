//! Text styling and modifiers.

use crate::color::Color;
use core::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Not};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
/// Text attributes applied to a cell (bold, italic, etc.).
///
/// Implemented as a manual bitflag over `u8`. Combine with `|`.
///
/// # Examples
///
/// ```
/// use rg::style::CellModifier;
///
/// let attrs = CellModifier::BOLD | CellModifier::ITALIC;
/// assert!(attrs.contains(CellModifier::BOLD));
/// assert!(attrs.contains(CellModifier::ITALIC));
/// assert!(!attrs.contains(CellModifier::UNDERLINE));
/// ```
pub struct CellModifier(u8);

impl CellModifier {
    /// No modifiers.
    pub const NONE: Self = Self(0);
    /// Bold text.
    pub const BOLD: Self = Self(1 << 0);
    /// Dim text.
    pub const DIM: Self = Self(1 << 1);
    /// Italic text.
    pub const ITALIC: Self = Self(1 << 2);
    /// Underlined text.
    pub const UNDERLINE: Self = Self(1 << 3);
    /// Blinking text.
    pub const BLINK: Self = Self(1 << 4);
    /// Reversed colors.
    pub const REVERSE: Self = Self(1 << 5);
    /// Hidden text.
    pub const HIDDEN: Self = Self(1 << 6);
    /// Strikethrough text.
    pub const STRIKETHROUGH: Self = Self(1 << 7);

    /// Returns `true` if all bits in `other` are set in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Returns `true` if no modifiers are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl BitOr for CellModifier {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for CellModifier {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl BitAnd for CellModifier {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl BitAndAssign for CellModifier {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl Not for CellModifier {
    type Output = Self;
    fn not(self) -> Self {
        Self(!self.0)
    }
}

impl core::fmt::Debug for CellModifier {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.0 == 0 {
            return write!(f, "NONE");
        }
        let mut sep = "";
        if self.contains(Self::BOLD) {
            write!(f, "{sep}BOLD")?;
            sep = " | ";
        }
        if self.contains(Self::DIM) {
            write!(f, "{sep}DIM")?;
            sep = " | ";
        }
        if self.contains(Self::ITALIC) {
            write!(f, "{sep}ITALIC")?;
            sep = " | ";
        }
        if self.contains(Self::UNDERLINE) {
            write!(f, "{sep}UNDERLINE")?;
            sep = " | ";
        }
        if self.contains(Self::BLINK) {
            write!(f, "{sep}BLINK")?;
            sep = " | ";
        }
        if self.contains(Self::REVERSE) {
            write!(f, "{sep}REVERSE")?;
            sep = " | ";
        }
        if self.contains(Self::HIDDEN) {
            write!(f, "{sep}HIDDEN")?;
            sep = " | ";
        }
        if self.contains(Self::STRIKETHROUGH) {
            write!(f, "{sep}STRIKETHROUGH")?;
            sep = " | ";
        }
        let _ = sep;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
/// A style consisting of foreground, background, and text modifiers.
pub struct Style {
    /// Foreground color.
    pub(crate) fg: Color,
    /// Background color.
    pub(crate) bg: Color,
    /// Text modifiers.
    pub(crate) modifiers: CellModifier,
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

    /// Adds the bold modifier.
    #[must_use]
    pub fn bold(mut self) -> Self {
        self.modifiers |= CellModifier::BOLD;
        self
    }

    /// Adds the italic modifier.
    #[must_use]
    pub fn italic(mut self) -> Self {
        self.modifiers |= CellModifier::ITALIC;
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

    /// Returns the text modifiers.
    #[must_use]
    pub const fn modifiers(&self) -> CellModifier {
        self.modifiers
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
        self.modifiers |= other.modifiers;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modifier_ops() {
        let m = CellModifier::BOLD | CellModifier::ITALIC;
        assert!(m.contains(CellModifier::BOLD));
        assert!(m.contains(CellModifier::ITALIC));
        assert!(!m.contains(CellModifier::UNDERLINE));
    }

    #[test]
    fn test_modifier_debug() {
        assert_eq!(format!("{:?}", CellModifier::NONE), "NONE");
        assert_eq!(
            format!("{:?}", CellModifier::BOLD | CellModifier::ITALIC),
            "BOLD | ITALIC"
        );
        assert_eq!(format!("{:?}", CellModifier::STRIKETHROUGH), "STRIKETHROUGH");
    }

    #[test]
    fn test_style_builder() {
        let s = Style::new().fg(Color::RED).bold();
        assert_eq!(s.foreground(), Color::RED);
        assert!(s.modifiers().contains(CellModifier::BOLD));
    }
}
