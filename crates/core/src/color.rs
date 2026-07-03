//! Styling types for character cells.

#[cfg(feature = "gem")]
use gem::space::Srgb;

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

    // ── gem integration ────────────────────────────────────────────────────

    /// Converts an `Rgb` variant to `gem::space::Srgb`.
    ///
    /// Returns `None` for non-RGB variants (`Default`, `Ansi`, `Indexed`).
    #[cfg(feature = "gem")]
    #[must_use]
    pub fn to_srgb(self) -> Option<Srgb> {
        match self {
            Self::Rgb { r, g, b } => Some(Srgb::new(
                f32::from(r) / 255.0,
                f32::from(g) / 255.0,
                f32::from(b) / 255.0,
            )),
            _ => None,
        }
    }

    /// Constructs an `Rgb` variant from a `gem::space::Srgb` color.
    ///
    /// Channels are clamped to `[0.0, 1.0]` before converting to `u8`.
    #[cfg(feature = "gem")]
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::use_self
    )]
    pub fn from_srgb(srgb: Srgb) -> Self {
        let clamped = srgb.clamp();
        Self::Rgb {
            r: (clamped.r * 255.0) as u8,
            g: (clamped.g * 255.0) as u8,
            b: (clamped.b * 255.0) as u8,
        }
    }

    /// Linearly interpolates between two `Rgb` colors.
    ///
    /// If either color is non-RGB, returns `a` (the first color) unchanged.
    /// Both must be `Rgb` for interpolation to occur.
    #[cfg(feature = "gem")]
    #[must_use]
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        match (a, b) {
            (
                Self::Rgb {
                    r: r1,
                    g: g1,
                    b: b1,
                },
                Self::Rgb {
                    r: r2,
                    g: g2,
                    b: b2,
                },
            ) => {
                let a_srgb = Srgb::new(
                    f32::from(r1) / 255.0,
                    f32::from(g1) / 255.0,
                    f32::from(b1) / 255.0,
                );
                let b_srgb = Srgb::new(
                    f32::from(r2) / 255.0,
                    f32::from(g2) / 255.0,
                    f32::from(b2) / 255.0,
                );
                Self::from_srgb(a_srgb.lerp(b_srgb, t))
            }
            (a, _) => a,
        }
    }

    /// Lightens an `Rgb` color by `amount` (0.0 = no change, 1.0 = white).
    ///
    /// Non-RGB variants are returned unchanged.
    #[cfg(feature = "gem")]
    #[must_use]
    pub fn lighten(self, amount: f32) -> Self {
        fn inner(r: u8, g: u8, b: u8, amount: f32) -> Color {
            let srgb = Srgb::new(
                f32::from(r) / 255.0,
                f32::from(g) / 255.0,
                f32::from(b) / 255.0,
            );
            Color::from_srgb(Srgb::from(gem::space::Hsl::from(srgb).lighten(amount)))
        }
        match self {
            Self::Rgb { r, g, b } => inner(r, g, b, amount),
            other => other,
        }
    }

    /// Darkens an `Rgb` color by `amount` (0.0 = no change, 1.0 = black).
    ///
    /// Non-RGB variants are returned unchanged.
    #[cfg(feature = "gem")]
    #[must_use]
    pub fn darken(self, amount: f32) -> Self {
        fn inner(r: u8, g: u8, b: u8, amount: f32) -> Color {
            let srgb = Srgb::new(
                f32::from(r) / 255.0,
                f32::from(g) / 255.0,
                f32::from(b) / 255.0,
            );
            Color::from_srgb(Srgb::from(gem::space::Hsl::from(srgb).darken(amount)))
        }
        match self {
            Self::Rgb { r, g, b } => inner(r, g, b, amount),
            other => other,
        }
    }

    /// Increases saturation of an `Rgb` color by `amount` (0.0–1.0).
    ///
    /// Non-RGB variants are returned unchanged.
    #[cfg(feature = "gem")]
    #[must_use]
    pub fn saturate(self, amount: f32) -> Self {
        fn inner(r: u8, g: u8, b: u8, amount: f32) -> Color {
            let srgb = Srgb::new(
                f32::from(r) / 255.0,
                f32::from(g) / 255.0,
                f32::from(b) / 255.0,
            );
            Color::from_srgb(Srgb::from(gem::space::Hsl::from(srgb).saturate(amount)))
        }
        match self {
            Self::Rgb { r, g, b } => inner(r, g, b, amount),
            other => other,
        }
    }

    /// Decreases saturation of an `Rgb` color by `amount` (0.0–1.0).
    ///
    /// Non-RGB variants are returned unchanged.
    #[cfg(feature = "gem")]
    #[must_use]
    pub fn desaturate(self, amount: f32) -> Self {
        fn inner(r: u8, g: u8, b: u8, amount: f32) -> Color {
            let srgb = Srgb::new(
                f32::from(r) / 255.0,
                f32::from(g) / 255.0,
                f32::from(b) / 255.0,
            );
            Color::from_srgb(Srgb::from(gem::space::Hsl::from(srgb).desaturate(amount)))
        }
        match self {
            Self::Rgb { r, g, b } => inner(r, g, b, amount),
            other => other,
        }
    }

    /// Returns the complementary color (hue shifted by 180 degrees).
    ///
    /// Non-RGB variants are returned unchanged.
    #[cfg(feature = "gem")]
    #[must_use]
    pub fn complement(self) -> Self {
        fn inner(r: u8, g: u8, b: u8) -> Color {
            let srgb = Srgb::new(
                f32::from(r) / 255.0,
                f32::from(g) / 255.0,
                f32::from(b) / 255.0,
            );
            Color::from_srgb(Srgb::from(gem::space::Hsl::from(srgb).complement()))
        }
        match self {
            Self::Rgb { r, g, b } => inner(r, g, b),
            other => other,
        }
    }

    /// Looks up a CSS named color by name (case-insensitive).
    ///
    /// Supports all 147 CSS Color Module Level 4 named colors.
    /// Returns `None` for unrecognized names.
    ///
    /// ## Examples
    ///
    /// ```rust
    /// use retroglyph_core::Color;
    ///
    /// let gold = Color::from_named("gold");
    /// assert_eq!(gold, Some(Color::Rgb { r: 255, g: 215, b: 0 }));
    ///
    /// assert_eq!(Color::from_named("not-a-color"), None);
    /// ```
    #[cfg(feature = "gem")]
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn from_named(name: &str) -> Option<Self> {
        let lower = name.to_ascii_lowercase();
        let srgb = match lower.as_str() {
            "aliceblue" => gem::named::ALICE_BLUE,
            "antiquewhite" => gem::named::ANTIQUE_WHITE,
            "aqua" => gem::named::AQUA,
            "aquamarine" => gem::named::AQUAMARINE,
            "azure" => gem::named::AZURE,
            "beige" => gem::named::BEIGE,
            "bisque" => gem::named::BISQUE,
            "black" => gem::named::BLACK,
            "blanchedalmond" => gem::named::BLANCHED_ALMOND,
            "blue" => gem::named::BLUE,
            "blueviolet" => gem::named::BLUE_VIOLET,
            "brown" => gem::named::BROWN,
            "burlywood" => gem::named::BURLY_WOOD,
            "cadetblue" => gem::named::CADET_BLUE,
            "chartreuse" => gem::named::CHARTREUSE,
            "chocolate" => gem::named::CHOCOLATE,
            "coral" => gem::named::CORAL,
            "cornflowerblue" => gem::named::CORNFLOWER_BLUE,
            "cornsilk" => gem::named::CORNSILK,
            "crimson" => gem::named::CRIMSON,
            "cyan" => gem::named::CYAN,
            "darkblue" => gem::named::DARK_BLUE,
            "darkcyan" => gem::named::DARK_CYAN,
            "darkgoldenrod" => gem::named::DARK_GOLDENROD,
            "darkgray" | "darkgrey" => gem::named::DARK_GRAY,
            "darkgreen" => gem::named::DARK_GREEN,
            "darkkhaki" => gem::named::DARK_KHAKI,
            "darkmagenta" => gem::named::DARK_MAGENTA,
            "darkolivegreen" => gem::named::DARK_OLIVE_GREEN,
            "darkorange" => gem::named::DARK_ORANGE,
            "darkorchid" => gem::named::DARK_ORCHID,
            "darkred" => gem::named::DARK_RED,
            "darksalmon" => gem::named::DARK_SALMON,
            "darkseagreen" => gem::named::DARK_SEA_GREEN,
            "darkslateblue" => gem::named::DARK_SLATE_BLUE,
            "darkslategray" | "darkslategrey" => gem::named::DARK_SLATE_GRAY,
            "darkturquoise" => gem::named::DARK_TURQUOISE,
            "darkviolet" => gem::named::DARK_VIOLET,
            "deeppink" => gem::named::DEEP_PINK,
            "deepskyblue" => gem::named::DEEP_SKY_BLUE,
            "dimgray" | "dimgrey" => gem::named::DIM_GRAY,
            "dodgerblue" => gem::named::DODGER_BLUE,
            "firebrick" => gem::named::FIREBRICK,
            "floralwhite" => gem::named::FLORAL_WHITE,
            "forestgreen" => gem::named::FOREST_GREEN,
            "fuchsia" => gem::named::FUCHSIA,
            "gainsboro" => gem::named::GAINSBORO,
            "ghostwhite" => gem::named::GHOST_WHITE,
            "gold" => gem::named::GOLD,
            "goldenrod" => gem::named::GOLDENROD,
            "gray" | "grey" => gem::named::GRAY,
            "green" => gem::named::GREEN,
            "greenyellow" => gem::named::GREEN_YELLOW,
            "honeydew" => gem::named::HONEYDEW,
            "hotpink" => gem::named::HOT_PINK,
            "indianred" => gem::named::INDIAN_RED,
            "indigo" => gem::named::INDIGO,
            "ivory" => gem::named::IVORY,
            "khaki" => gem::named::KHAKI,
            "lavender" => gem::named::LAVENDER,
            "lavenderblush" => gem::named::LAVENDER_BLUSH,
            "lawngreen" => gem::named::LAWN_GREEN,
            "lemonchiffon" => gem::named::LEMON_CHIFFON,
            "lightblue" => gem::named::LIGHT_BLUE,
            "lightcoral" => gem::named::LIGHT_CORAL,
            "lightcyan" => gem::named::LIGHT_CYAN,
            "lightgoldenrodyellow" => gem::named::LIGHT_GOLDENROD_YELLOW,
            "lightgray" | "lightgrey" => gem::named::LIGHT_GRAY,
            "lightgreen" => gem::named::LIGHT_GREEN,
            "lightpink" => gem::named::LIGHT_PINK,
            "lightsalmon" => gem::named::LIGHT_SALMON,
            "lightseagreen" => gem::named::LIGHT_SEA_GREEN,
            "lightskyblue" => gem::named::LIGHT_SKY_BLUE,
            "lightslategray" | "lightslategrey" => gem::named::LIGHT_SLATE_GRAY,
            "lightsteelblue" => gem::named::LIGHT_STEEL_BLUE,
            "lightyellow" => gem::named::LIGHT_YELLOW,
            "lime" => gem::named::LIME,
            "limegreen" => gem::named::LIME_GREEN,
            "linen" => gem::named::LINEN,
            "magenta" => gem::named::MAGENTA,
            "maroon" => gem::named::MAROON,
            "mediumaquamarine" => gem::named::MEDIUM_AQUAMARINE,
            "mediumblue" => gem::named::MEDIUM_BLUE,
            "mediumorchid" => gem::named::MEDIUM_ORCHID,
            "mediumpurple" => gem::named::MEDIUM_PURPLE,
            "mediumseagreen" => gem::named::MEDIUM_SEA_GREEN,
            "mediumslateblue" => gem::named::MEDIUM_SLATE_BLUE,
            "mediumspringgreen" => gem::named::MEDIUM_SPRING_GREEN,
            "mediumturquoise" => gem::named::MEDIUM_TURQUOISE,
            "mediumvioletred" => gem::named::MEDIUM_VIOLET_RED,
            "midnightblue" => gem::named::MIDNIGHT_BLUE,
            "mintcream" => gem::named::MINT_CREAM,
            "mistyrose" => gem::named::MISTY_ROSE,
            "moccasin" => gem::named::MOCCASIN,
            "navajowhite" => gem::named::NAVAJO_WHITE,
            "navy" => gem::named::NAVY,
            "oldlace" => gem::named::OLD_LACE,
            "olive" => gem::named::OLIVE,
            "olivedrab" => gem::named::OLIVE_DRAB,
            "orange" => gem::named::ORANGE,
            "orangered" => gem::named::ORANGE_RED,
            "orchid" => gem::named::ORCHID,
            "palegoldenrod" => gem::named::PALE_GOLDENROD,
            "palegreen" => gem::named::PALE_GREEN,
            "paleturquoise" => gem::named::PALE_TURQUOISE,
            "palevioletred" => gem::named::PALE_VIOLET_RED,
            "papayawhip" => gem::named::PAPAYA_WHIP,
            "peachpuff" => gem::named::PEACH_PUFF,
            "peru" => gem::named::PERU,
            "pink" => gem::named::PINK,
            "plum" => gem::named::PLUM,
            "powderblue" => gem::named::POWDER_BLUE,
            "purple" => gem::named::PURPLE,
            "rebeccapurple" => gem::named::REBECCA_PURPLE,
            "red" => gem::named::RED,
            "rosybrown" => gem::named::ROSY_BROWN,
            "royalblue" => gem::named::ROYAL_BLUE,
            "saddlebrown" => gem::named::SADDLE_BROWN,
            "salmon" => gem::named::SALMON,
            "sandybrown" => gem::named::SANDY_BROWN,
            "seagreen" => gem::named::SEA_GREEN,
            "seashell" => gem::named::SEASHELL,
            "sienna" => gem::named::SIENNA,
            "silver" => gem::named::SILVER,
            "skyblue" => gem::named::SKY_BLUE,
            "slateblue" => gem::named::SLATE_BLUE,
            "slategray" | "slategrey" => gem::named::SLATE_GRAY,
            "snow" => gem::named::SNOW,
            "springgreen" => gem::named::SPRING_GREEN,
            "steelblue" => gem::named::STEEL_BLUE,
            "tan" => gem::named::TAN,
            "teal" => gem::named::TEAL,
            "thistle" => gem::named::THISTLE,
            "tomato" => gem::named::TOMATO,
            "turquoise" => gem::named::TURQUOISE,
            "violet" => gem::named::VIOLET,
            "wheat" => gem::named::WHEAT,
            "white" => gem::named::WHITE,
            "whitesmoke" => gem::named::WHITE_SMOKE,
            "yellow" => gem::named::YELLOW,
            "yellowgreen" => gem::named::YELLOW_GREEN,
            _ => return None,
        };
        Some(Self::from_srgb(srgb))
    }

    /// Parses a CSS hex color string into an `Rgb` variant.
    ///
    /// Accepts `#rgb` and `#rrggbb` formats (case-insensitive).
    /// Returns `None` for invalid input.
    ///
    /// ## Examples
    ///
    /// ```rust
    /// use retroglyph_core::Color;
    ///
    /// let c = Color::from_hex("#ff8000").unwrap();
    /// assert_eq!(c, Color::Rgb { r: 255, g: 128, b: 0 });
    ///
    /// assert_eq!(Color::from_hex("not-color"), None);
    /// ```
    #[cfg(feature = "gem")]
    #[must_use]
    pub fn from_hex(hex: &str) -> Option<Self> {
        Srgb::from_hex(hex).map(Self::from_srgb)
    }
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

    #[cfg(feature = "gem")]
    #[test]
    fn test_from_named_color() {
        let gold = Color::from_named("gold").expect("gold is a named color");
        assert_eq!(
            gold,
            Color::Rgb {
                r: 255,
                g: 215,
                b: 0
            }
        );
    }

    #[cfg(feature = "gem")]
    #[test]
    fn test_from_named_color_case_insensitive() {
        let red = Color::from_named("RED").expect("should match uppercase");
        assert_eq!(red, Color::Rgb { r: 255, g: 0, b: 0 });
    }

    #[cfg(feature = "gem")]
    #[test]
    fn test_from_named_color_invalid() {
        assert_eq!(Color::from_named("not-a-color"), None);
    }

    #[cfg(feature = "gem")]
    #[test]
    fn test_from_hex_full() {
        let c = Color::from_hex("#ff8000").expect("valid hex");
        assert_eq!(
            c,
            Color::Rgb {
                r: 255,
                g: 128,
                b: 0
            }
        );
    }

    #[cfg(feature = "gem")]
    #[test]
    fn test_from_hex_short() {
        let c = Color::from_hex("#f80").expect("valid short hex");
        assert_eq!(
            c,
            Color::Rgb {
                r: 255,
                g: 136,
                b: 0
            }
        );
    }

    #[cfg(feature = "gem")]
    #[test]
    fn test_from_hex_invalid() {
        assert_eq!(Color::from_hex("xyz"), None);
        assert_eq!(Color::from_hex("#xyz"), None);
    }

    #[cfg(feature = "gem")]
    #[test]
    fn test_lerp() {
        let red = Color::Rgb { r: 255, g: 0, b: 0 };
        let blue = Color::Rgb { r: 0, g: 0, b: 255 };
        let purple = Color::lerp(red, blue, 0.5);
        // 127.5 truncates to 127 in u8
        assert_eq!(
            purple,
            Color::Rgb {
                r: 127,
                g: 0,
                b: 127
            }
        );
    }

    #[cfg(feature = "gem")]
    #[test]
    fn test_lerp_non_rgb_passthrough() {
        let default = Color::Default;
        let red = Color::Rgb { r: 255, g: 0, b: 0 };
        assert_eq!(Color::lerp(default, red, 0.5), Color::Default);
    }

    #[cfg(feature = "gem")]
    #[test]
    fn test_lighten_rgb() {
        let c = Color::Rgb {
            r: 128,
            g: 64,
            b: 32,
        };
        let lighter = c.lighten(0.2);
        assert_ne!(lighter, c);
    }

    #[cfg(feature = "gem")]
    #[test]
    fn test_lighten_non_rgb_passthrough() {
        assert_eq!(Color::Default.lighten(0.5), Color::Default);
        assert_eq!(
            Color::Ansi(AnsiColor::Red).lighten(0.5),
            Color::Ansi(AnsiColor::Red)
        );
    }

    #[cfg(feature = "gem")]
    #[test]
    fn test_darken_rgb() {
        let c = Color::Rgb {
            r: 128,
            g: 64,
            b: 32,
        };
        let darker = c.darken(0.2);
        assert_ne!(darker, c);
    }

    #[cfg(feature = "gem")]
    #[test]
    fn test_complement_red() {
        let red = Color::Rgb { r: 255, g: 0, b: 0 };
        let cyan = red.complement();
        assert!(cyan.to_srgb().is_some_and(|c| c.g > 0.9));
        assert!(cyan.to_srgb().is_some_and(|c| c.b > 0.9));
    }

    #[cfg(feature = "gem")]
    #[test]
    fn test_to_srgb_conversion() {
        let c = Color::Rgb {
            r: 200,
            g: 100,
            b: 50,
        };
        let srgb = c.to_srgb().expect("Rgb variant should convert");
        assert!((srgb.r - 200.0 / 255.0).abs() < 1e-6);
        assert!((srgb.g - 100.0 / 255.0).abs() < 1e-6);
        assert!((srgb.b - 50.0 / 255.0).abs() < 1e-6);
    }

    #[cfg(feature = "gem")]
    #[test]
    fn test_to_srgb_non_rgb_returns_none() {
        assert_eq!(Color::Default.to_srgb(), None);
        assert_eq!(Color::Ansi(AnsiColor::Red).to_srgb(), None);
        assert_eq!(Color::Indexed(42).to_srgb(), None);
    }

    #[cfg(feature = "gem")]
    #[test]
    fn test_from_srgb_roundtrip() {
        let srgb = Srgb::new(0.8, 0.4, 0.2);
        let c = Color::from_srgb(srgb);
        let back = c.to_srgb().expect("should convert back");
        assert!((back.r - 0.8).abs() < 1.1 / 255.0);
        assert!((back.g - 0.4).abs() < 1.1 / 255.0);
        assert!((back.b - 0.2).abs() < 1.1 / 255.0);
    }

    #[cfg(feature = "gem")]
    #[test]
    fn test_saturate_desaturate() {
        let c = Color::Rgb {
            r: 128,
            g: 128,
            b: 128,
        };
        let saturated = c.saturate(0.5);
        assert_ne!(saturated, c);

        let desaturated = saturated.desaturate(0.5);
        let diff = |a: u8, b: u8| (i16::from(a) - i16::from(b)).unsigned_abs();
        assert!(
            diff(
                match desaturated {
                    Color::Rgb { b, .. } => b,
                    _ => 0,
                },
                128
            ) <= 2
        );
    }

    #[cfg(feature = "gem")]
    #[test]
    fn test_complement_non_rgb_passthrough() {
        assert_eq!(Color::Default.complement(), Color::Default);
    }

    #[cfg(feature = "gem")]
    #[test]
    fn test_lerp_endpoints() {
        let red = Color::Rgb { r: 255, g: 0, b: 0 };
        let blue = Color::Rgb { r: 0, g: 0, b: 255 };
        assert_eq!(Color::lerp(red, blue, 0.0), red);
        assert_eq!(Color::lerp(red, blue, 1.0), blue);
    }

    #[cfg(feature = "gem")]
    #[test]
    fn test_darken_black_is_black() {
        let black = Color::Rgb { r: 0, g: 0, b: 0 };
        assert_eq!(black.darken(0.5), black);
    }

    #[cfg(feature = "gem")]
    #[test]
    fn test_gray_grey_synonyms() {
        assert_eq!(Color::from_named("gray"), Color::from_named("grey"));
        assert_eq!(Color::from_named("darkgray"), Color::from_named("darkgrey"));
        assert_eq!(
            Color::from_named("slategray"),
            Color::from_named("slategrey")
        );
    }
}
