//! Styling types for character cells.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
/// Standard 16-color ANSI palette.
///
/// Prefer `Ansi` colors when you want your game to respect the user's
/// terminal color theme (e.g., Solarized, Nord, or custom themes).
/// Use `Rgb` for fixed colors that must appear identical regardless of
/// the user's terminal configuration.
pub enum AnsiColor {
    #[default]
    /// Black.
    Black = 0,
    /// Red.
    Red,
    /// Green.
    Green,
    /// Yellow.
    Yellow,
    /// Blue.
    Blue,
    /// Magenta.
    Magenta,
    /// Cyan.
    Cyan,
    /// White.
    White,
    /// Bright Black.
    BrightBlack,
    /// Bright Red.
    BrightRed,
    /// Bright Green.
    BrightGreen,
    /// Bright Yellow.
    BrightYellow,
    /// Bright Blue.
    BrightBlue,
    /// Bright Magenta.
    BrightMagenta,
    /// Bright Cyan.
    BrightCyan,
    /// Bright White.
    BrightWhite,
}

impl AnsiColor {
    /// Returns the ANSI color code as a `u8` index.
    #[must_use]
    pub const fn to_index(self) -> u8 {
        self as u8
    }
}

/// Error returned when a `u8` value has no corresponding [`AnsiColor`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidAnsiIndex(pub u8);

impl core::fmt::Display for InvalidAnsiIndex {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "invalid ANSI color index: {}", self.0)
    }
}

impl TryFrom<u8> for AnsiColor {
    type Error = InvalidAnsiIndex;

    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            0 => Ok(Self::Black),
            1 => Ok(Self::Red),
            2 => Ok(Self::Green),
            3 => Ok(Self::Yellow),
            4 => Ok(Self::Blue),
            5 => Ok(Self::Magenta),
            6 => Ok(Self::Cyan),
            7 => Ok(Self::White),
            8 => Ok(Self::BrightBlack),
            9 => Ok(Self::BrightRed),
            10 => Ok(Self::BrightGreen),
            11 => Ok(Self::BrightYellow),
            12 => Ok(Self::BrightBlue),
            13 => Ok(Self::BrightMagenta),
            14 => Ok(Self::BrightCyan),
            15 => Ok(Self::BrightWhite),
            _ => Err(InvalidAnsiIndex(v)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
/// Represents a color in the terminal grid.
pub enum Color {
    #[default]
    /// Backend's default foreground/background color.
    ///
    /// This tells the rendering backend to use the terminal's configured
    /// default colors (e.g., the user's background color preference).
    Default,
    /// One of the 16 standard ANSI colors.
    ///
    /// Use these to respect the user's terminal theme.
    Ansi(AnsiColor),
    /// 256-color palette index.
    Indexed(u8),
    /// 24-bit RGB color.
    ///
    /// Use this for exact color matching regardless of terminal settings.
    Rgb {
        /// Red channel.
        r: u8,
        /// Green channel.
        g: u8,
        /// Blue channel.
        b: u8,
    },
}

impl Color {
    /// Standard Black (ANSI).
    pub const BLACK: Self = Self::Ansi(AnsiColor::Black);
    /// Standard Red (ANSI).
    pub const RED: Self = Self::Ansi(AnsiColor::Red);
    /// Standard Green (ANSI).
    pub const GREEN: Self = Self::Ansi(AnsiColor::Green);
    /// Standard Yellow (ANSI).
    pub const YELLOW: Self = Self::Ansi(AnsiColor::Yellow);
    /// Standard Blue (ANSI).
    pub const BLUE: Self = Self::Ansi(AnsiColor::Blue);
    /// Standard Magenta (ANSI).
    pub const MAGENTA: Self = Self::Ansi(AnsiColor::Magenta);
    /// Standard Cyan (ANSI).
    pub const CYAN: Self = Self::Ansi(AnsiColor::Cyan);
    /// Standard White (ANSI).
    pub const WHITE: Self = Self::Ansi(AnsiColor::White);
    /// Bright Black / dark grey (ANSI).
    pub const BRIGHT_BLACK: Self = Self::Ansi(AnsiColor::BrightBlack);
    /// Bright Red (ANSI).
    pub const BRIGHT_RED: Self = Self::Ansi(AnsiColor::BrightRed);
    /// Bright Green (ANSI).
    pub const BRIGHT_GREEN: Self = Self::Ansi(AnsiColor::BrightGreen);
    /// Bright Yellow (ANSI).
    pub const BRIGHT_YELLOW: Self = Self::Ansi(AnsiColor::BrightYellow);
    /// Bright Blue (ANSI).
    pub const BRIGHT_BLUE: Self = Self::Ansi(AnsiColor::BrightBlue);
    /// Bright Magenta (ANSI).
    pub const BRIGHT_MAGENTA: Self = Self::Ansi(AnsiColor::BrightMagenta);
    /// Bright Cyan (ANSI).
    pub const BRIGHT_CYAN: Self = Self::Ansi(AnsiColor::BrightCyan);
    /// Bright White (ANSI).
    pub const BRIGHT_WHITE: Self = Self::Ansi(AnsiColor::BrightWhite);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_defaults() {
        assert_eq!(Color::default(), Color::Default);
    }

    #[test]
    fn test_ansi_values() {
        assert_eq!(AnsiColor::Red as u8, 1);
        assert_eq!(AnsiColor::BrightWhite as u8, 15);
    }

    #[test]
    fn test_ansi_try_from_roundtrip() {
        for i in 0u8..16 {
            let color = AnsiColor::try_from(i).expect("should be valid");
            assert_eq!(color.to_index(), i);
        }
    }

    #[test]
    fn test_ansi_try_from_invalid() {
        assert_eq!(AnsiColor::try_from(16), Err(InvalidAnsiIndex(16)));
        assert_eq!(AnsiColor::try_from(255), Err(InvalidAnsiIndex(255)));
    }
}
