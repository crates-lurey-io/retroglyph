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
        b: u8 
    },
}

impl Color {
    /// Standard Red color (ANSI).
    pub const RED: Self = Self::Ansi(AnsiColor::Red);
    /// Standard Green color (ANSI).
    pub const GREEN: Self = Self::Ansi(AnsiColor::Green);
    /// Standard Blue color (ANSI).
    pub const BLUE: Self = Self::Ansi(AnsiColor::Blue);
    /// Standard White color (ANSI).
    pub const WHITE: Self = Self::Ansi(AnsiColor::White);
    /// Standard Black color (ANSI).
    pub const BLACK: Self = Self::Ansi(AnsiColor::Black);
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
}
