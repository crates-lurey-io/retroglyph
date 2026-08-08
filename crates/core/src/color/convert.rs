//! `Color`'s inherent methods: named constants, RGB resolution, and the `gem`-backed color-space
//! conversions. See `named` for the string-name/hex constructors (`Color::from_named`,
//! `Color::from_hex`).

use gem::Mix as _;
use gem::rgb::Rgb888;
use gem::space::Srgb;

use super::Color;
use super::ansi::AnsiColor;
use super::ansi::indexed_to_rgb;
use super::ansi::rgb_to_srgb;
use super::ansi::{Quantize, rgb_to_ansi, rgb_to_indexed};

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

    /// Constructs an `Rgb` variant from its three channels.
    #[must_use]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::Rgb { r, g, b }
    }

    /// Constructs an `Rgb` variant from a packed `0xRRGGBB` value. The high byte is ignored.
    ///
    /// Round-trips the `#rrggbb` form [`Display`](core::fmt::Display) writes, so a color logged
    /// out of a running app can be pasted straight back into source. Unlike
    /// [`from_hex`](Self::from_hex), there is no CSS short-form expansion: `Color::hex(0xF80)` is
    /// `#000f80`, not `#ff8800`.
    #[must_use]
    pub const fn hex(rgb: u32) -> Self {
        Self::Rgb {
            r: ((rgb >> 16) & 0xFF) as u8,
            g: ((rgb >> 8) & 0xFF) as u8,
            b: (rgb & 0xFF) as u8,
        }
    }

    /// Resolves this color to a concrete 24-bit `(r, g, b)` triple, substituting `default` for
    /// [`Color::Default`](crate::color::Color::Default).
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
    #[must_use]
    pub fn from_srgb(srgb: Srgb) -> Self {
        let (r, g, b) = Rgb888::from(srgb).to_rgb();
        Self::Rgb { r, g, b }
    }

    /// Linearly interpolates between two colors, always returning a concrete `Rgb` result.
    ///
    /// Both inputs are resolved to `(r, g, b)` via [`Color::resolve_rgb`](crate::color::Color::resolve_rgb) before blending, so
    /// non-`Rgb` variants (`Ansi`, `Indexed`) contribute their real color rather than being
    /// skipped. [`Color::Default`](crate::color::Color::Default) has no intrinsic RGB value, so it falls back to
    /// `(0, 0, 0)` when it appears as `a` and `(255, 255, 255)` when it appears as `b`.
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
    /// Shared by [`Color::lighten`](crate::color::Color::lighten), [`Color::darken`](crate::color::Color::darken), [`Color::saturate`](crate::color::Color::saturate),
    /// [`Color::desaturate`](crate::color::Color::desaturate), and [`Color::complement`](crate::color::Color::complement), which differ only in which
    /// `gem::space::Hsl` method `f` calls. Non-`Rgb` variants are resolved to `(r, g, b)` via
    /// [`Color::resolve_rgb`](crate::color::Color::resolve_rgb) before the transform is applied, rather than being returned
    /// unchanged. [`Color::Default`](crate::color::Color::Default) has no intrinsic RGB value, so it resolves to `(0, 0, 0)`.
    fn map_hsl(self, f: impl FnOnce(gem::space::Hsl) -> gem::space::Hsl) -> Self {
        let (r, g, b) = self.resolve_rgb((0, 0, 0));
        let hsl = gem::space::Hsl::from(rgb_to_srgb(r, g, b));
        Self::from_srgb(Srgb::from(f(hsl)))
    }

    /// Lightens a color by `amount` (0.0 = no change, 1.0 = white).
    ///
    /// Non-`Rgb` variants are resolved to `(r, g, b)` via [`Color::resolve_rgb`](crate::color::Color::resolve_rgb) before the
    /// transform is applied, rather than being returned unchanged. [`Color::Default`](crate::color::Color::Default) has no
    /// intrinsic RGB value, so it resolves to `(0, 0, 0)`.
    #[must_use]
    pub fn lighten(self, amount: f32) -> Self {
        self.map_hsl(|hsl| hsl.lighten(amount))
    }

    /// Darkens a color by `amount` (0.0 = no change, 1.0 = black).
    ///
    /// Non-`Rgb` variants are resolved to `(r, g, b)` via [`Color::resolve_rgb`](crate::color::Color::resolve_rgb) before the
    /// transform is applied, rather than being returned unchanged. [`Color::Default`](crate::color::Color::Default) has no
    /// intrinsic RGB value, so it resolves to `(0, 0, 0)`.
    #[must_use]
    pub fn darken(self, amount: f32) -> Self {
        self.map_hsl(|hsl| hsl.darken(amount))
    }

    /// Increases saturation of a color by `amount` (0.0–1.0).
    ///
    /// Non-`Rgb` variants are resolved to `(r, g, b)` via [`Color::resolve_rgb`](crate::color::Color::resolve_rgb) before the
    /// transform is applied, rather than being returned unchanged. [`Color::Default`](crate::color::Color::Default) has no
    /// intrinsic RGB value, so it resolves to `(0, 0, 0)`.
    #[must_use]
    pub fn saturate(self, amount: f32) -> Self {
        self.map_hsl(|hsl| hsl.saturate(amount))
    }

    /// Decreases saturation of a color by `amount` (0.0–1.0).
    ///
    /// Non-`Rgb` variants are resolved to `(r, g, b)` via [`Color::resolve_rgb`](crate::color::Color::resolve_rgb) before the
    /// transform is applied, rather than being returned unchanged. [`Color::Default`](crate::color::Color::Default) has no
    /// intrinsic RGB value, so it resolves to `(0, 0, 0)`.
    #[must_use]
    pub fn desaturate(self, amount: f32) -> Self {
        self.map_hsl(|hsl| hsl.desaturate(amount))
    }

    /// Returns the complementary color (hue shifted by 180 degrees).
    ///
    /// Non-`Rgb` variants are resolved to `(r, g, b)` via [`Color::resolve_rgb`](crate::color::Color::resolve_rgb) before the
    /// transform is applied, rather than being returned unchanged. [`Color::Default`](crate::color::Color::Default) has no
    /// intrinsic RGB value, so it resolves to `(0, 0, 0)`.
    #[must_use]
    pub fn complement(self) -> Self {
        self.map_hsl(gem::space::Hsl::complement)
    }

    /// Quantizes an RGB color to the nearest entry in the standard 256-color palette, by
    /// perceptual (Oklab) distance.
    ///
    /// Equivalent to [`to_indexed_with(Quantize::Perceptual)`](Self::to_indexed_with); see there
    /// for the full contract and for the euclidean alternative.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::color::Color;
    ///
    /// let black = Color::rgb(0, 0, 0);
    /// assert_eq!(black.to_indexed(), Color::Indexed(0));
    ///
    /// // Non-RGB colors pass through unchanged.
    /// assert_eq!(Color::Default.to_indexed(), Color::Default);
    /// ```
    ///
    /// Backends that render to terminals without full RGB support can call this method to
    /// downgrade colors before emitting them; `retroglyph-core` never downgrades colors on
    /// its own. See [`Color::to_ansi`](crate::color::Color::to_ansi) to quantize to the smaller 16-color ANSI palette.
    #[must_use]
    pub fn to_indexed(self) -> Self {
        self.to_indexed_with(Quantize::Perceptual)
    }

    /// Quantizes an RGB color to the nearest entry in the standard 256-color palette, under
    /// `metric`.
    ///
    /// - `Color::Rgb` inputs are converted to the nearest 256-color palette index (0–255),
    ///   searching the 16 ANSI colors (0–15), the 6×6×6 color cube (16–231), and the grayscale
    ///   ramp (232–255).
    /// - `Color::Default`, `Color::Ansi`, and `Color::Indexed` are returned unchanged: this
    ///   method only downgrades `Rgb` colors.
    /// - Ties (multiple equidistant palette entries) are resolved by preferring the lower
    ///   index.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::color::{Color, Quantize};
    ///
    /// let salmon = Color::rgb(250, 128, 114);
    /// assert_eq!(salmon.to_indexed_with(Quantize::Perceptual), Color::Indexed(210));
    /// assert_eq!(salmon.to_indexed_with(Quantize::Euclidean), Color::Indexed(209));
    /// ```
    #[must_use]
    pub fn to_indexed_with(self, metric: Quantize) -> Self {
        match self {
            Self::Rgb { r, g, b } => Self::Indexed(rgb_to_indexed(r, g, b, metric)),
            other => other,
        }
    }

    /// Quantizes an RGB color to the nearest of the 16 standard ANSI palette colors, by
    /// perceptual (Oklab) distance.
    ///
    /// Equivalent to [`to_ansi_with(Quantize::Perceptual)`](Self::to_ansi_with); see there for the
    /// full contract and for the euclidean alternative.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::color::{AnsiColor, Color};
    ///
    /// let pure_red = Color::rgb(255, 0, 0);
    /// assert_eq!(pure_red.to_ansi(), Color::Ansi(AnsiColor::BrightRed));
    ///
    /// // Non-RGB colors pass through unchanged.
    /// assert_eq!(Color::Default.to_ansi(), Color::Default);
    /// ```
    ///
    /// Use this method when rendering to terminals limited to 16 colors, or when a caller
    /// otherwise needs to reduce color depth. See [`Color::to_indexed`](crate::color::Color::to_indexed) to quantize to the
    /// larger 256-color palette instead.
    #[must_use]
    pub fn to_ansi(self) -> Self {
        self.to_ansi_with(Quantize::Perceptual)
    }

    /// Quantizes an RGB color to the nearest of the 16 standard ANSI palette colors, under
    /// `metric`.
    ///
    /// - `Color::Rgb` inputs are converted to the nearest of the 16 standard ANSI colors.
    /// - `Color::Default`, `Color::Ansi`, and `Color::Indexed` are returned unchanged: this
    ///   method only downgrades `Rgb` colors.
    /// - Ties (multiple equidistant palette entries) are resolved by preferring the lower
    ///   ANSI index.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::color::{AnsiColor, Color, Quantize};
    ///
    /// // Euclidean RGB distance over-weights green, so this reddish brown lands on yellow.
    /// let chocolate = Color::rgb(210, 105, 30);
    /// assert_eq!(chocolate.to_ansi_with(Quantize::Perceptual), Color::Ansi(AnsiColor::BrightRed));
    /// assert_eq!(chocolate.to_ansi_with(Quantize::Euclidean), Color::Ansi(AnsiColor::Yellow));
    /// ```
    #[must_use]
    pub fn to_ansi_with(self, metric: Quantize) -> Self {
        match self {
            Self::Rgb { r, g, b } => Self::Ansi(rgb_to_ansi(r, g, b, metric)),
            other => other,
        }
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
        assert_eq!(Color::rgb(10, 20, 30).resolve_rgb((1, 2, 3)), (10, 20, 30));
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
        let c = Color::rgb(10, 20, 30);
        assert!(matches!(c.to_indexed(), Color::Indexed(_)));
    }

    #[test]
    fn test_to_ansi_returns_ansi_variant() {
        let c = Color::rgb(10, 20, 30);
        assert!(matches!(c.to_ansi(), Color::Ansi(_)));
    }

    #[test]
    fn test_to_indexed_black_and_white() {
        let black = Color::rgb(0, 0, 0);
        assert_eq!(black.to_indexed(), Color::Indexed(0));

        let white = Color::rgb(255, 255, 255);
        assert_eq!(white.to_indexed(), Color::Indexed(15));
    }

    #[test]
    fn test_to_ansi_pure_primaries() {
        let red = Color::rgb(255, 0, 0);
        assert_eq!(red.to_ansi(), Color::Ansi(AnsiColor::BrightRed));

        let green = Color::rgb(0, 255, 0);
        assert_eq!(green.to_ansi(), Color::Ansi(AnsiColor::BrightGreen));

        // Pure (0, 0, 255) is closer to the standard Blue reference (0, 0, 238) than to
        // BrightBlue (92, 92, 255), whose red/green components pull it further away.
        let blue = Color::rgb(0, 0, 255);
        assert_eq!(blue.to_ansi(), Color::Ansi(AnsiColor::Blue));

        let black = Color::rgb(0, 0, 0);
        assert_eq!(black.to_ansi(), Color::Ansi(AnsiColor::Black));
    }

    #[test]
    fn test_to_ansi_all_16_roundtrip() {
        // Each ANSI reference color, when quantized back to ANSI, should resolve to
        // itself (it is by definition its own nearest neighbor in the ANSI palette).
        for ansi in ANSI_COLORS {
            let (r, g, b) = ansi.to_rgb();
            let c = Color::rgb(r, g, b);
            assert_eq!(c.to_ansi(), Color::Ansi(ansi), "ansi color {ansi:?}");
        }
    }

    #[test]
    fn test_to_indexed_mid_gray() {
        let gray = Color::rgb(128, 128, 128);
        // Should land in the grayscale ramp or cube, never panics or overflows.
        assert!(matches!(gray.to_indexed(), Color::Indexed(_)));
    }

    #[test]
    fn test_to_indexed_and_to_ansi_default_to_perceptual() {
        // The no-argument forms are the `Quantize::Perceptual` ones, not merely "whichever
        // metric happens to be compiled in".
        for (r, g, b) in [
            (210, 105, 30),
            (250, 128, 114),
            (135, 206, 235),
            (0, 128, 128),
        ] {
            let c = Color::rgb(r, g, b);
            assert_eq!(c.to_indexed(), c.to_indexed_with(Quantize::Perceptual));
            assert_eq!(c.to_ansi(), c.to_ansi_with(Quantize::Perceptual));
        }
    }

    #[test]
    fn test_quantize_metrics_disagree() {
        // Guards the point of the knob: if these ever agreed everywhere, one metric would be
        // dead weight.
        let chocolate = Color::rgb(210, 105, 30);
        assert_eq!(
            chocolate.to_ansi_with(Quantize::Perceptual),
            Color::Ansi(AnsiColor::BrightRed)
        );
        assert_eq!(
            chocolate.to_ansi_with(Quantize::Euclidean),
            Color::Ansi(AnsiColor::Yellow)
        );

        let salmon = Color::rgb(250, 128, 114);
        assert_eq!(
            salmon.to_indexed_with(Quantize::Perceptual),
            Color::Indexed(210)
        );
        assert_eq!(
            salmon.to_indexed_with(Quantize::Euclidean),
            Color::Indexed(209)
        );
    }

    #[test]
    fn test_quantize_non_rgb_passthrough_under_every_metric() {
        for metric in [Quantize::Perceptual, Quantize::Euclidean] {
            for color in [
                Color::Default,
                Color::Ansi(AnsiColor::Green),
                Color::Indexed(42),
            ] {
                assert_eq!(color.to_indexed_with(metric), color, "{color:?} {metric:?}");
                assert_eq!(color.to_ansi_with(metric), color, "{color:?} {metric:?}");
            }
        }
    }

    #[test]
    fn test_quantize_default_is_perceptual() {
        assert_eq!(Quantize::default(), Quantize::Perceptual);
    }

    #[test]
    fn test_every_metric_maps_each_ansi_reference_color_to_itself() {
        for metric in [Quantize::Perceptual, Quantize::Euclidean] {
            for ansi in ANSI_COLORS {
                let (r, g, b) = ansi.to_rgb();
                let c = Color::rgb(r, g, b);
                assert_eq!(
                    c.to_ansi_with(metric),
                    Color::Ansi(ansi),
                    "ansi color {ansi:?} under {metric:?}"
                );
            }
        }
    }

    #[test]
    fn test_lerp() {
        let red = Color::rgb(255, 0, 0);
        let blue = Color::rgb(0, 0, 255);
        let purple = Color::lerp(red, blue, 0.5);
        // 127.5 rounds to 128 (round-to-nearest, ties away from zero).
        assert_eq!(purple, Color::rgb(128, 0, 128));
    }

    #[test]
    fn test_lerp_resolves_non_rgb() {
        let red = Color::rgb(255, 0, 0);

        // `Color::BLACK` (an `Ansi` variant) resolves to real black and blends normally, rather
        // than short-circuiting to itself.
        assert_eq!(Color::lerp(Color::BLACK, red, 1.0), red);
        assert_eq!(Color::lerp(Color::BLACK, red, 0.0), Color::rgb(0, 0, 0));

        // `Color::Ansi(AnsiColor::Black)` behaves identically to `Color::BLACK` (they're the same
        // variant).
        assert_eq!(Color::lerp(Color::Ansi(AnsiColor::Black), red, 1.0), red);

        // `Color::Default` resolves to `(0, 0, 0)` as `a` and `(255, 255, 255)` as `b`.
        assert_eq!(Color::lerp(Color::Default, red, 0.0), Color::rgb(0, 0, 0));
        assert_eq!(
            Color::lerp(red, Color::Default, 1.0),
            Color::rgb(255, 255, 255)
        );
    }

    #[test]
    fn test_lighten_rgb() {
        let c = Color::rgb(128, 64, 32);
        let lighter = c.lighten(0.2);
        assert_ne!(lighter, c);
    }

    #[test]
    fn test_lighten_resolves_non_rgb() {
        assert_ne!(Color::Default.lighten(0.5), Color::Default);
        assert_ne!(
            Color::Ansi(AnsiColor::Black).lighten(0.5),
            Color::Ansi(AnsiColor::Black)
        );
        assert_ne!(Color::BLACK.lighten(0.5), Color::BLACK);
    }

    #[test]
    fn test_darken_rgb() {
        let c = Color::rgb(128, 64, 32);
        let darker = c.darken(0.2);
        assert_ne!(darker, c);
    }

    #[test]
    fn test_darken_resolves_non_rgb() {
        // `Color::Default` resolves to `(0, 0, 0)`, which darkening leaves at black.
        assert_eq!(Color::Default.darken(0.5), Color::rgb(0, 0, 0));
        // `Color::Ansi(AnsiColor::Black)` (and `Color::BLACK`) resolve to real black too.
        assert_eq!(
            Color::Ansi(AnsiColor::Black).darken(0.5),
            Color::rgb(0, 0, 0)
        );
        assert_eq!(Color::BLACK.darken(0.5), Color::rgb(0, 0, 0));
    }

    #[test]
    fn test_complement_red() {
        let red = Color::rgb(255, 0, 0);
        let cyan = red.complement();
        assert!(cyan.to_srgb().is_some_and(|c| c.g > 0.9));
        assert!(cyan.to_srgb().is_some_and(|c| c.b > 0.9));
    }

    #[test]
    fn test_to_srgb_conversion() {
        let c = Color::rgb(200, 100, 50);
        let srgb = c.to_srgb().expect("Rgb variant should convert");
        assert!((srgb.r - 200.0 / 255.0).abs() < 1e-6);
        assert!((srgb.g - 100.0 / 255.0).abs() < 1e-6);
        assert!((srgb.b - 50.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn test_to_srgb_non_rgb_returns_none() {
        assert_eq!(Color::Default.to_srgb(), None);
        assert_eq!(Color::Ansi(AnsiColor::Red).to_srgb(), None);
        assert_eq!(Color::Indexed(42).to_srgb(), None);
    }

    #[test]
    fn test_from_srgb_roundtrip() {
        let srgb = Srgb::new(0.8, 0.4, 0.2);
        let c = Color::from_srgb(srgb);
        let back = c.to_srgb().expect("should convert back");
        assert!((back.r - 0.8).abs() < 1.1 / 255.0);
        assert!((back.g - 0.4).abs() < 1.1 / 255.0);
        assert!((back.b - 0.2).abs() < 1.1 / 255.0);
    }

    #[test]
    fn test_saturate_desaturate() {
        let c = Color::rgb(128, 128, 128);
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

    #[test]
    fn test_saturate_desaturate_resolves_non_rgb() {
        // Gray-ish ANSI colors have a saturation to increase/decrease; black (`Color::Default`'s
        // resolved fallback and `Color::BLACK`) has none, but both must go through the same
        // resolve-then-transform path rather than passing through unchanged.
        assert_eq!(Color::Default.saturate(0.5), Color::rgb(0, 0, 0));
        assert_eq!(
            Color::Ansi(AnsiColor::Black).desaturate(0.5),
            Color::rgb(0, 0, 0)
        );
        assert_eq!(Color::BLACK.saturate(0.5), Color::rgb(0, 0, 0));

        let red = Color::Ansi(AnsiColor::Red);
        assert_ne!(red.desaturate(0.5), red);
    }

    #[test]
    fn test_complement_resolves_non_rgb() {
        // Black's complement (in this HSL model) is still black, but it's computed through a
        // real RGB resolution rather than being returned unchanged.
        assert_eq!(Color::Default.complement(), Color::rgb(0, 0, 0));
        assert_eq!(
            Color::Ansi(AnsiColor::Black).complement(),
            Color::rgb(0, 0, 0)
        );
        assert_eq!(Color::BLACK.complement(), Color::rgb(0, 0, 0));

        let red_via_ansi = Color::Ansi(AnsiColor::Red).complement();
        assert!(red_via_ansi.to_srgb().is_some_and(|c| c.g > 0.5));
        assert!(red_via_ansi.to_srgb().is_some_and(|c| c.b > 0.5));
    }

    #[test]
    fn test_lerp_endpoints() {
        let red = Color::rgb(255, 0, 0);
        let blue = Color::rgb(0, 0, 255);
        assert_eq!(Color::lerp(red, blue, 0.0), red);
        assert_eq!(Color::lerp(red, blue, 1.0), blue);
    }

    #[test]
    fn test_darken_black_is_black() {
        let black = Color::rgb(0, 0, 0);
        assert_eq!(black.darken(0.5), black);
    }
}
