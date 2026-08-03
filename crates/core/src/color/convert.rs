//! `Color`'s inherent methods: named constants, RGB resolution, and the `gem`-backed color-space
//! conversions (`indexed-quant` feature).

#[cfg(feature = "indexed-quant")]
use gem::Mix as _;
#[cfg(feature = "indexed-quant")]
use gem::rgb::Rgb888;
#[cfg(feature = "indexed-quant")]
use gem::space::Srgb;

use super::Color;
use super::ansi::AnsiColor;
use super::ansi::indexed_to_rgb;
#[cfg(feature = "indexed-quant")]
use super::ansi::rgb_to_srgb;
use super::ansi::{rgb_to_ansi, rgb_to_indexed};

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

    /// Resolves this color to a concrete 24-bit `(r, g, b)` triple, substituting `default` for
    /// [`Color::Default`].
    ///
    /// This is the canonical color-to-RGB resolution every graphical backend shares, so that a
    /// glyph drawn through the CPU rasterizer (`retroglyph-software`) and the GPU atlas
    /// (`retroglyph-gl`) comes out the same pixel color:
    ///
    /// - [`Rgb`](Self::Rgb) passes through unchanged.
    /// - [`Ansi`](Self::Ansi) resolves through [`AnsiColor::to_rgb`](super::AnsiColor::to_rgb) (the one canonical ANSI
    ///   palette).
    /// - [`Indexed`](Self::Indexed) resolves through the 256-color palette (16 ANSI + 6×6×6 cube
    ///   + grayscale ramp).
    /// - [`Default`](Self::Default) (and any future non-exhaustive variant this crate can't yet
    ///   resolve) returns `default`, which the caller picks per channel (foreground vs
    ///   background).
    ///
    /// Terminal backends do *not* use this: they emit ANSI/indexed colors as-is and let the
    /// terminal apply the user's theme. It exists specifically for pixel/GPU backends that must
    /// produce real RGB.
    #[must_use]
    pub const fn resolve_rgb(self, default: (u8, u8, u8)) -> (u8, u8, u8) {
        match self {
            Self::Rgb { r, g, b } => (r, g, b),
            Self::Ansi(ansi) => ansi.to_rgb(),
            Self::Indexed(index) => indexed_to_rgb(index),
            // `Color::Default` plus any future `#[non_exhaustive]` variant this crate doesn't yet
            // know how to resolve to RGB.
            _ => default,
        }
    }

    // ── gem integration ────────────────────────────────────────────────────

    /// Converts an `Rgb` variant to `gem::space::Srgb`.
    ///
    /// Returns `None` for non-RGB variants (`Default`, `Ansi`, `Indexed`).
    #[cfg(feature = "indexed-quant")]
    #[must_use]
    pub fn to_srgb(self) -> Option<Srgb> {
        match self {
            Self::Rgb { r, g, b } => Some(rgb_to_srgb(r, g, b)),
            _ => None,
        }
    }

    /// Constructs an `Rgb` variant from a `gem::space::Srgb` color.
    ///
    /// Channels are clamped to `[0.0, 1.0]` and rounded to the nearest `u8` (ties away from
    /// zero), via `gem::rgb::Rgb888`'s own `Srgb` conversion, the same round-to-nearest rule
    /// every other integer channel operation in this crate follows (see
    /// `tests/rounding_conformance.rs`).
    #[cfg(feature = "indexed-quant")]
    #[must_use]
    pub fn from_srgb(srgb: Srgb) -> Self {
        let (r, g, b) = Rgb888::from(srgb).to_rgb();
        Self::Rgb { r, g, b }
    }

    /// Linearly interpolates between two colors, always returning a concrete `Rgb` result.
    ///
    /// Both inputs are resolved to `(r, g, b)` via [`Color::resolve_rgb`] before blending, so
    /// non-`Rgb` variants (`Ansi`, `Indexed`) contribute their real color rather than being
    /// skipped. [`Color::Default`] has no intrinsic RGB value, so it falls back to
    /// `(0, 0, 0)` when it appears as `a` and `(255, 255, 255)` when it appears as `b`.
    #[cfg(feature = "indexed-quant")]
    #[must_use]
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let (r1, g1, b1) = a.resolve_rgb((0, 0, 0));
        let (r2, g2, b2) = b.resolve_rgb((255, 255, 255));
        let a_srgb = rgb_to_srgb(r1, g1, b1);
        let b_srgb = rgb_to_srgb(r2, g2, b2);
        Self::from_srgb(a_srgb.mix(b_srgb, t))
    }

    /// Applies `f` to this color's HSL representation and converts the result back to `Rgb`.
    ///
    /// Shared by [`Color::lighten`], [`Color::darken`], [`Color::saturate`],
    /// [`Color::desaturate`], and [`Color::complement`], which differ only in which
    /// `gem::space::Hsl` method `f` calls. Non-`Rgb` variants are resolved to `(r, g, b)` via
    /// [`Color::resolve_rgb`] before the transform is applied, rather than being returned
    /// unchanged. [`Color::Default`] has no intrinsic RGB value, so it resolves to `(0, 0, 0)`.
    #[cfg(feature = "indexed-quant")]
    fn map_hsl(self, f: impl FnOnce(gem::space::Hsl) -> gem::space::Hsl) -> Self {
        let (r, g, b) = self.resolve_rgb((0, 0, 0));
        let hsl = gem::space::Hsl::from(rgb_to_srgb(r, g, b));
        Self::from_srgb(Srgb::from(f(hsl)))
    }

    /// Lightens a color by `amount` (0.0 = no change, 1.0 = white).
    ///
    /// Non-`Rgb` variants are resolved to `(r, g, b)` via [`Color::resolve_rgb`] before the
    /// transform is applied, rather than being returned unchanged. [`Color::Default`] has no
    /// intrinsic RGB value, so it resolves to `(0, 0, 0)`.
    #[cfg(feature = "indexed-quant")]
    #[must_use]
    pub fn lighten(self, amount: f32) -> Self {
        self.map_hsl(|hsl| hsl.lighten(amount))
    }

    /// Darkens a color by `amount` (0.0 = no change, 1.0 = black).
    ///
    /// Non-`Rgb` variants are resolved to `(r, g, b)` via [`Color::resolve_rgb`] before the
    /// transform is applied, rather than being returned unchanged. [`Color::Default`] has no
    /// intrinsic RGB value, so it resolves to `(0, 0, 0)`.
    #[cfg(feature = "indexed-quant")]
    #[must_use]
    pub fn darken(self, amount: f32) -> Self {
        self.map_hsl(|hsl| hsl.darken(amount))
    }

    /// Increases saturation of a color by `amount` (0.0–1.0).
    ///
    /// Non-`Rgb` variants are resolved to `(r, g, b)` via [`Color::resolve_rgb`] before the
    /// transform is applied, rather than being returned unchanged. [`Color::Default`] has no
    /// intrinsic RGB value, so it resolves to `(0, 0, 0)`.
    #[cfg(feature = "indexed-quant")]
    #[must_use]
    pub fn saturate(self, amount: f32) -> Self {
        self.map_hsl(|hsl| hsl.saturate(amount))
    }

    /// Decreases saturation of a color by `amount` (0.0–1.0).
    ///
    /// Non-`Rgb` variants are resolved to `(r, g, b)` via [`Color::resolve_rgb`] before the
    /// transform is applied, rather than being returned unchanged. [`Color::Default`] has no
    /// intrinsic RGB value, so it resolves to `(0, 0, 0)`.
    #[cfg(feature = "indexed-quant")]
    #[must_use]
    pub fn desaturate(self, amount: f32) -> Self {
        self.map_hsl(|hsl| hsl.desaturate(amount))
    }

    /// Returns the complementary color (hue shifted by 180 degrees).
    ///
    /// Non-`Rgb` variants are resolved to `(r, g, b)` via [`Color::resolve_rgb`] before the
    /// transform is applied, rather than being returned unchanged. [`Color::Default`] has no
    /// intrinsic RGB value, so it resolves to `(0, 0, 0)`.
    #[cfg(feature = "indexed-quant")]
    #[must_use]
    pub fn complement(self) -> Self {
        self.map_hsl(gem::space::Hsl::complement)
    }

    /// Quantizes an RGB color to the nearest entry in the standard 256-color palette.
    ///
    /// - `Color::Rgb` inputs are converted to the nearest 256-color palette index (0–255).
    ///   With the `indexed-quant` feature (default), perceptual distance in the Oklab color space is
    ///   used, which better matches human color perception than raw RGB distance. Without
    ///   `gem`, euclidean RGB distance is used instead, computed against the 6×6×6 color
    ///   cube (indices 16–231), supplemented by the grayscale ramp (232–255) and the 16
    ///   ANSI colors (0–15).
    /// - `Color::Default`, `Color::Ansi`, and `Color::Indexed` are returned unchanged: this
    ///   method only downgrades `Rgb` colors.
    /// - Ties (multiple equidistant palette entries) are resolved by preferring the lower
    ///   index.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::Color;
    ///
    /// let black = Color::Rgb { r: 0, g: 0, b: 0 };
    /// assert_eq!(black.to_indexed(), Color::Indexed(0));
    ///
    /// // Non-RGB colors pass through unchanged.
    /// assert_eq!(Color::Default.to_indexed(), Color::Default);
    /// ```
    ///
    /// Backends that render to terminals without full RGB support can call this method to
    /// downgrade colors before emitting them; `retroglyph-core` never downgrades colors on
    /// its own. See [`Color::to_ansi`] to quantize to the smaller 16-color ANSI palette.
    #[must_use]
    pub fn to_indexed(self) -> Self {
        match self {
            Self::Rgb { r, g, b } => Self::Indexed(rgb_to_indexed(r, g, b)),
            other => other,
        }
    }

    /// Quantizes an RGB color to the nearest of the 16 standard ANSI palette colors.
    ///
    /// - `Color::Rgb` inputs are converted to the nearest of the 16 standard ANSI colors.
    ///   With the `indexed-quant` feature (default), perceptual distance in the Oklab color space is
    ///   used. Without `gem`, euclidean RGB distance is used instead.
    /// - `Color::Default`, `Color::Ansi`, and `Color::Indexed` are returned unchanged: this
    ///   method only downgrades `Rgb` colors.
    /// - Ties (multiple equidistant palette entries) are resolved by preferring the lower
    ///   ANSI index.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{AnsiColor, Color};
    ///
    /// let pure_red = Color::Rgb { r: 255, g: 0, b: 0 };
    /// assert_eq!(pure_red.to_ansi(), Color::Ansi(AnsiColor::BrightRed));
    ///
    /// // Non-RGB colors pass through unchanged.
    /// assert_eq!(Color::Default.to_ansi(), Color::Default);
    /// ```
    ///
    /// Use this method when rendering to terminals limited to 16 colors, or when a caller
    /// otherwise needs to reduce color depth. See [`Color::to_indexed`] to quantize to the
    /// larger 256-color palette instead.
    #[must_use]
    pub fn to_ansi(self) -> Self {
        match self {
            Self::Rgb { r, g, b } => Self::Ansi(rgb_to_ansi(r, g, b)),
            other => other,
        }
    }

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
    use super::super::ansi::ANSI_COLORS;
    use super::*;

    #[test]
    fn test_color_defaults() {
        assert_eq!(Color::default(), Color::Default);
    }

    #[test]
    fn test_resolve_rgb() {
        // Rgb passes through; Default uses the supplied fallback (per channel).
        assert_eq!(
            Color::Rgb {
                r: 10,
                g: 20,
                b: 30
            }
            .resolve_rgb((1, 2, 3)),
            (10, 20, 30)
        );
        assert_eq!(Color::Default.resolve_rgb((1, 2, 3)), (1, 2, 3));
        // Ansi resolves through the canonical palette.
        assert_eq!(
            Color::Ansi(AnsiColor::Red).resolve_rgb((0, 0, 0)),
            AnsiColor::Red.to_rgb()
        );
        // Indexed: 0..16 == ANSI, the cube and grayscale ramp resolve too.
        assert_eq!(
            Color::Indexed(1).resolve_rgb((0, 0, 0)),
            AnsiColor::Red.to_rgb()
        );
        assert_eq!(Color::Indexed(16).resolve_rgb((0, 0, 0)), (0, 0, 0));
        assert_eq!(Color::Indexed(231).resolve_rgb((0, 0, 0)), (255, 255, 255));
        assert_eq!(Color::Indexed(232).resolve_rgb((0, 0, 0)), (8, 8, 8));
    }

    // ── to_indexed / to_ansi (non-RGB passthrough) ─────────────────────────

    #[test]
    fn test_to_indexed_non_rgb_passthrough() {
        assert_eq!(Color::Default.to_indexed(), Color::Default);
        assert_eq!(
            Color::Ansi(AnsiColor::Red).to_indexed(),
            Color::Ansi(AnsiColor::Red)
        );
        assert_eq!(Color::Indexed(42).to_indexed(), Color::Indexed(42));
    }

    #[test]
    fn test_to_ansi_non_rgb_passthrough() {
        assert_eq!(Color::Default.to_ansi(), Color::Default);
        assert_eq!(
            Color::Ansi(AnsiColor::Red).to_ansi(),
            Color::Ansi(AnsiColor::Red)
        );
        assert_eq!(Color::Indexed(42).to_ansi(), Color::Indexed(42));
    }

    #[test]
    fn test_to_indexed_returns_indexed_variant() {
        let c = Color::Rgb {
            r: 10,
            g: 20,
            b: 30,
        };
        assert!(matches!(c.to_indexed(), Color::Indexed(_)));
    }

    #[test]
    fn test_to_ansi_returns_ansi_variant() {
        let c = Color::Rgb {
            r: 10,
            g: 20,
            b: 30,
        };
        assert!(matches!(c.to_ansi(), Color::Ansi(_)));
    }

    #[test]
    fn test_to_indexed_black_and_white() {
        let black = Color::Rgb { r: 0, g: 0, b: 0 };
        assert_eq!(black.to_indexed(), Color::Indexed(0));

        let white = Color::Rgb {
            r: 255,
            g: 255,
            b: 255,
        };
        assert_eq!(white.to_indexed(), Color::Indexed(15));
    }

    #[test]
    fn test_to_ansi_pure_primaries() {
        let red = Color::Rgb { r: 255, g: 0, b: 0 };
        assert_eq!(red.to_ansi(), Color::Ansi(AnsiColor::BrightRed));

        let green = Color::Rgb { r: 0, g: 255, b: 0 };
        assert_eq!(green.to_ansi(), Color::Ansi(AnsiColor::BrightGreen));

        // Pure (0, 0, 255) is closer to the standard Blue reference (0, 0, 238) than to
        // BrightBlue (92, 92, 255), whose red/green components pull it further away.
        let blue = Color::Rgb { r: 0, g: 0, b: 255 };
        assert_eq!(blue.to_ansi(), Color::Ansi(AnsiColor::Blue));

        let black = Color::Rgb { r: 0, g: 0, b: 0 };
        assert_eq!(black.to_ansi(), Color::Ansi(AnsiColor::Black));
    }

    #[test]
    fn test_to_ansi_all_16_roundtrip() {
        // Each ANSI reference color, when quantized back to ANSI, should resolve to
        // itself (it is by definition its own nearest neighbor in the ANSI palette).
        for ansi in ANSI_COLORS {
            let (r, g, b) = ansi.to_rgb();
            let c = Color::Rgb { r, g, b };
            assert_eq!(c.to_ansi(), Color::Ansi(ansi), "ansi color {ansi:?}");
        }
    }

    #[test]
    fn test_to_indexed_mid_gray() {
        let gray = Color::Rgb {
            r: 128,
            g: 128,
            b: 128,
        };
        // Should land in the grayscale ramp or cube, never panics or overflows.
        assert!(matches!(gray.to_indexed(), Color::Indexed(_)));
    }

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
    fn test_lerp() {
        let red = Color::Rgb { r: 255, g: 0, b: 0 };
        let blue = Color::Rgb { r: 0, g: 0, b: 255 };
        let purple = Color::lerp(red, blue, 0.5);
        // 127.5 rounds to 128 (round-to-nearest, ties away from zero).
        assert_eq!(
            purple,
            Color::Rgb {
                r: 128,
                g: 0,
                b: 128
            }
        );
    }

    #[cfg(feature = "indexed-quant")]
    #[test]
    fn test_lerp_resolves_non_rgb() {
        let red = Color::Rgb { r: 255, g: 0, b: 0 };

        // `Color::BLACK` (an `Ansi` variant) resolves to real black and blends normally, rather
        // than short-circuiting to itself.
        assert_eq!(Color::lerp(Color::BLACK, red, 1.0), red);
        assert_eq!(
            Color::lerp(Color::BLACK, red, 0.0),
            Color::Rgb { r: 0, g: 0, b: 0 }
        );

        // `Color::Ansi(AnsiColor::Black)` behaves identically to `Color::BLACK` (they're the same
        // variant).
        assert_eq!(Color::lerp(Color::Ansi(AnsiColor::Black), red, 1.0), red);

        // `Color::Default` resolves to `(0, 0, 0)` as `a` and `(255, 255, 255)` as `b`.
        assert_eq!(
            Color::lerp(Color::Default, red, 0.0),
            Color::Rgb { r: 0, g: 0, b: 0 }
        );
        assert_eq!(
            Color::lerp(red, Color::Default, 1.0),
            Color::Rgb {
                r: 255,
                g: 255,
                b: 255
            }
        );
    }

    #[cfg(feature = "indexed-quant")]
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

    #[cfg(feature = "indexed-quant")]
    #[test]
    fn test_lighten_resolves_non_rgb() {
        assert_ne!(Color::Default.lighten(0.5), Color::Default);
        assert_ne!(
            Color::Ansi(AnsiColor::Black).lighten(0.5),
            Color::Ansi(AnsiColor::Black)
        );
        assert_ne!(Color::BLACK.lighten(0.5), Color::BLACK);
    }

    #[cfg(feature = "indexed-quant")]
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

    #[cfg(feature = "indexed-quant")]
    #[test]
    fn test_darken_resolves_non_rgb() {
        // `Color::Default` resolves to `(0, 0, 0)`, which darkening leaves at black.
        assert_eq!(Color::Default.darken(0.5), Color::Rgb { r: 0, g: 0, b: 0 });
        // `Color::Ansi(AnsiColor::Black)` (and `Color::BLACK`) resolve to real black too.
        assert_eq!(
            Color::Ansi(AnsiColor::Black).darken(0.5),
            Color::Rgb { r: 0, g: 0, b: 0 }
        );
        assert_eq!(Color::BLACK.darken(0.5), Color::Rgb { r: 0, g: 0, b: 0 });
    }

    #[cfg(feature = "indexed-quant")]
    #[test]
    fn test_complement_red() {
        let red = Color::Rgb { r: 255, g: 0, b: 0 };
        let cyan = red.complement();
        assert!(cyan.to_srgb().is_some_and(|c| c.g > 0.9));
        assert!(cyan.to_srgb().is_some_and(|c| c.b > 0.9));
    }

    #[cfg(feature = "indexed-quant")]
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

    #[cfg(feature = "indexed-quant")]
    #[test]
    fn test_to_srgb_non_rgb_returns_none() {
        assert_eq!(Color::Default.to_srgb(), None);
        assert_eq!(Color::Ansi(AnsiColor::Red).to_srgb(), None);
        assert_eq!(Color::Indexed(42).to_srgb(), None);
    }

    #[cfg(feature = "indexed-quant")]
    #[test]
    fn test_from_srgb_roundtrip() {
        let srgb = Srgb::new(0.8, 0.4, 0.2);
        let c = Color::from_srgb(srgb);
        let back = c.to_srgb().expect("should convert back");
        assert!((back.r - 0.8).abs() < 1.1 / 255.0);
        assert!((back.g - 0.4).abs() < 1.1 / 255.0);
        assert!((back.b - 0.2).abs() < 1.1 / 255.0);
    }

    #[cfg(feature = "indexed-quant")]
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

    #[cfg(feature = "indexed-quant")]
    #[test]
    fn test_saturate_desaturate_resolves_non_rgb() {
        // Gray-ish ANSI colors have a saturation to increase/decrease; black (`Color::Default`'s
        // resolved fallback and `Color::BLACK`) has none, but both must go through the same
        // resolve-then-transform path rather than passing through unchanged.
        assert_eq!(
            Color::Default.saturate(0.5),
            Color::Rgb { r: 0, g: 0, b: 0 }
        );
        assert_eq!(
            Color::Ansi(AnsiColor::Black).desaturate(0.5),
            Color::Rgb { r: 0, g: 0, b: 0 }
        );
        assert_eq!(Color::BLACK.saturate(0.5), Color::Rgb { r: 0, g: 0, b: 0 });

        let red = Color::Ansi(AnsiColor::Red);
        assert_ne!(red.desaturate(0.5), red);
    }

    #[cfg(feature = "indexed-quant")]
    #[test]
    fn test_complement_resolves_non_rgb() {
        // Black's complement (in this HSL model) is still black, but it's computed through a
        // real RGB resolution rather than being returned unchanged.
        assert_eq!(Color::Default.complement(), Color::Rgb { r: 0, g: 0, b: 0 });
        assert_eq!(
            Color::Ansi(AnsiColor::Black).complement(),
            Color::Rgb { r: 0, g: 0, b: 0 }
        );
        assert_eq!(Color::BLACK.complement(), Color::Rgb { r: 0, g: 0, b: 0 });

        let red_via_ansi = Color::Ansi(AnsiColor::Red).complement();
        assert!(red_via_ansi.to_srgb().is_some_and(|c| c.g > 0.5));
        assert!(red_via_ansi.to_srgb().is_some_and(|c| c.b > 0.5));
    }

    #[cfg(feature = "indexed-quant")]
    #[test]
    fn test_lerp_endpoints() {
        let red = Color::Rgb { r: 255, g: 0, b: 0 };
        let blue = Color::Rgb { r: 0, g: 0, b: 255 };
        assert_eq!(Color::lerp(red, blue, 0.0), red);
        assert_eq!(Color::lerp(red, blue, 1.0), blue);
    }

    #[cfg(feature = "indexed-quant")]
    #[test]
    fn test_darken_black_is_black() {
        let black = Color::Rgb { r: 0, g: 0, b: 0 };
        assert_eq!(black.darken(0.5), black);
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
