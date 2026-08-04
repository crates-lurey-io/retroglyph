//! `Color`'s string-name/hex constructors: [`Color::from_named`] (the CSS named-color table) and
//! [`Color::from_hex`], both `gem`-backed and gated on the `indexed-quant` feature.

#[cfg(feature = "indexed-quant")]
use gem::space::Srgb;

use super::Color;

impl Color {
    /// Looks up a CSS named color by name (case-insensitive).
    ///
    /// Supports all 147 CSS Color Module Level 4 named colors.
    /// Returns `None` for unrecognized names.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::Color;
    ///
    /// let gold = Color::from_named("gold");
    /// assert_eq!(gold, Some(Color::Rgb { r: 255, g: 215, b: 0 }));
    ///
    /// assert_eq!(Color::from_named("not-a-color"), None);
    /// ```
    #[cfg(feature = "indexed-quant")]
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
    /// # Examples
    ///
    /// ```
    /// # fn main() {
    /// # fn run() -> Option<()> {
    /// use retroglyph_core::Color;
    ///
    /// let c = Color::from_hex("#ff8000")?;
    /// assert_eq!(c, Color::Rgb { r: 255, g: 128, b: 0 });
    ///
    /// assert_eq!(Color::from_hex("not-color"), None);
    /// # Some(())
    /// # }
    /// # run().unwrap();
    /// # }
    /// ```
    #[cfg(feature = "indexed-quant")]
    #[must_use]
    pub fn from_hex(hex: &str) -> Option<Self> {
        Srgb::from_hex(hex).map(Self::from_srgb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "indexed-quant")]
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

    #[cfg(feature = "indexed-quant")]
    #[test]
    fn test_from_named_color_case_insensitive() {
        let red = Color::from_named("RED").expect("should match uppercase");
        assert_eq!(red, Color::Rgb { r: 255, g: 0, b: 0 });
    }

    #[cfg(feature = "indexed-quant")]
    #[test]
    fn test_from_named_color_invalid() {
        assert_eq!(Color::from_named("not-a-color"), None);
    }

    #[cfg(feature = "indexed-quant")]
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

    #[cfg(feature = "indexed-quant")]
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

    #[cfg(feature = "indexed-quant")]
    #[test]
    fn test_from_hex_invalid() {
        assert_eq!(Color::from_hex("xyz"), None);
        assert_eq!(Color::from_hex("#xyz"), None);
    }

    #[cfg(feature = "indexed-quant")]
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
