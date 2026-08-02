//! Styling types for character cells.

use alloc::string::String;
#[cfg(feature = "indexed-quant")]
use gem::Mix as _;
#[cfg(feature = "indexed-quant")]
use gem::space::Srgb;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
/// Standard 16-color ANSI palette.
///
/// Prefer `Ansi` colors when you want your game to respect the user's
/// terminal color theme (e.g., Solarized, Nord, or custom themes).
/// Use `Rgb` for fixed colors that must appear identical regardless of
/// the user's terminal configuration.
///
/// # Examples
///
/// ```
/// use retroglyph_core::{AnsiColor, Color};
///
/// let color = Color::Ansi(AnsiColor::Green);
/// assert_eq!(AnsiColor::Green.to_index(), 2);
/// assert_eq!(color, Color::GREEN);
/// ```
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

    /// Returns the standard xterm RGB values for this ANSI color.
    ///
    /// These are the same 16 reference colors used by [`Color::to_indexed`] and
    /// [`Color::to_ansi`] when quantizing RGB input; a terminal's actual theme may
    /// render these colors differently.
    #[must_use]
    pub const fn to_rgb(self) -> (u8, u8, u8) {
        match self {
            Self::Black => (0, 0, 0),
            Self::Red => (205, 0, 0),
            Self::Green => (0, 205, 0),
            Self::Yellow => (205, 205, 0),
            Self::Blue => (0, 0, 238),
            Self::Magenta => (205, 0, 205),
            Self::Cyan => (0, 205, 205),
            Self::White => (229, 229, 229),
            Self::BrightBlack => (127, 127, 127),
            Self::BrightRed => (255, 0, 0),
            Self::BrightGreen => (0, 255, 0),
            Self::BrightYellow => (255, 255, 0),
            Self::BrightBlue => (92, 92, 255),
            Self::BrightMagenta => (255, 0, 255),
            Self::BrightCyan => (0, 255, 255),
            Self::BrightWhite => (255, 255, 255),
        }
    }
}

/// All 16 [`AnsiColor`] variants in index order (0–15), for iterating the palette.
const ANSI_COLORS: [AnsiColor; 16] = [
    AnsiColor::Black,
    AnsiColor::Red,
    AnsiColor::Green,
    AnsiColor::Yellow,
    AnsiColor::Blue,
    AnsiColor::Magenta,
    AnsiColor::Cyan,
    AnsiColor::White,
    AnsiColor::BrightBlack,
    AnsiColor::BrightRed,
    AnsiColor::BrightGreen,
    AnsiColor::BrightYellow,
    AnsiColor::BrightBlue,
    AnsiColor::BrightMagenta,
    AnsiColor::BrightCyan,
    AnsiColor::BrightWhite,
];

/// The 6 steps used for each channel of the 256-color palette's 6×6×6 RGB cube
/// (indices 16–231).
const CUBE_STEPS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// The 24 grayscale ramp values used by the 256-color palette (indices 232–255).
const GRAYSCALE_RAMP: [u8; 24] = [
    8, 18, 28, 38, 48, 58, 68, 78, 88, 98, 108, 118, 128, 138, 148, 158, 168, 178, 188, 198, 208,
    218, 228, 238,
];

/// Returns the RGB value for a 256-color palette index (0–255).
///
/// Indices 0–15 are the 16 standard ANSI colors, 16–231 are the 6×6×6 RGB cube, and
/// 232–255 are the grayscale ramp.
///
/// Not feature-gated: [`Color::resolve_rgb`] calls it on every build, so it must always compile
/// regardless of the `indexed-quant` feature (the `to_indexed`-family callers below are `gem`-gated, but
/// this table lookup itself has no `gem` dependency).
const fn indexed_to_rgb(index: u8) -> (u8, u8, u8) {
    if index < 16 {
        ANSI_COLORS[index as usize].to_rgb()
    } else if index < 232 {
        let cube_index = index - 16;
        let r = CUBE_STEPS[(cube_index / 36) as usize];
        let g = CUBE_STEPS[((cube_index / 6) % 6) as usize];
        let b = CUBE_STEPS[(cube_index % 6) as usize];
        (r, g, b)
    } else {
        let gray = GRAYSCALE_RAMP[(index - 232) as usize];
        (gray, gray, gray)
    }
}

/// Rounds `value` to the nearest of the 6 [`CUBE_STEPS`], returning the step's index
/// (0–5).
///
/// Ties (exactly halfway between two steps) round to the higher step, matching
/// standard "round half up" arithmetic rounding on the midpoint distance.
#[cfg(any(not(feature = "indexed-quant"), test))]
fn nearest_cube_step(value: u8) -> u8 {
    let value = i32::from(value);
    let mut best_index = 0u8;
    let mut best_distance = i32::MAX;
    for (i, &step) in CUBE_STEPS.iter().enumerate() {
        let distance = (value - i32::from(step)).abs();
        if distance < best_distance {
            best_distance = distance;
            best_index = u8::try_from(i).unwrap_or(0);
        }
    }
    best_index
}

/// Quantizes `(r, g, b)` to the nearest 256-color palette index using the 6×6×6 RGB
/// cube, grayscale ramp, and the 16 ANSI colors, breaking ties by preferring the
/// lower index.
///
/// This is the fallback used by [`Color::to_indexed`] without the `indexed-quant` feature, and
/// is always available regardless of feature flags.
///
/// Checks the 16 ANSI colors, the cube's single nearest point (found by rounding each
/// channel independently), and the grayscale ramp's single nearest point, rather than
/// scanning all 256 entries individually: rounding each channel independently already
/// finds the cube's closest point, and likewise for the single-channel grayscale ramp.
/// Candidates are checked in ascending index order and only replace the current best
/// on strictly smaller distance, so ties naturally resolve to the lower index.
#[cfg(any(not(feature = "indexed-quant"), test))]
fn cube_map_to_indexed(r: u8, g: u8, b: u8) -> u8 {
    let mut best_index = 0u8;
    let mut best_distance = u32::MAX;

    // Candidate group 1: the 16 ANSI colors (indices 0-15), lowest indices first.
    for (i, ansi) in ANSI_COLORS.iter().enumerate() {
        let distance = gem::rgb::distance_sq((r, g, b), ansi.to_rgb());
        if distance < best_distance {
            best_distance = distance;
            best_index = u8::try_from(i).unwrap_or(0);
        }
    }

    // Candidate group 2: nearest point in the 6x6x6 cube (indices 16-231).
    let cube_r = nearest_cube_step(r);
    let cube_g = nearest_cube_step(g);
    let cube_b = nearest_cube_step(b);
    let cube_index = 16 + 36 * cube_r + 6 * cube_g + cube_b;
    let cube_rgb = (
        CUBE_STEPS[cube_r as usize],
        CUBE_STEPS[cube_g as usize],
        CUBE_STEPS[cube_b as usize],
    );
    let cube_distance = gem::rgb::distance_sq((r, g, b), cube_rgb);
    if cube_distance < best_distance {
        best_distance = cube_distance;
        best_index = cube_index;
    }

    // Candidate group 3: nearest grayscale ramp entry (indices 232-255).
    for (i, &gray) in GRAYSCALE_RAMP.iter().enumerate() {
        let distance = gem::rgb::distance_sq((r, g, b), (gray, gray, gray));
        if distance < best_distance {
            best_distance = distance;
            best_index = 232 + u8::try_from(i).unwrap_or(0);
        }
    }

    best_index
}

/// Quantizes `(r, g, b)` to the nearest of the 16 standard ANSI colors, using
/// euclidean RGB distance and breaking ties by preferring the lower index.
///
/// This is the fallback used by [`Color::to_ansi`] without the `indexed-quant` feature, and is
/// always available regardless of feature flags.
#[cfg(any(not(feature = "indexed-quant"), test))]
fn cube_map_to_ansi(r: u8, g: u8, b: u8) -> AnsiColor {
    let mut best = AnsiColor::Black;
    let mut best_distance = u32::MAX;
    for ansi in ANSI_COLORS {
        let distance = gem::rgb::distance_sq((r, g, b), ansi.to_rgb());
        if distance < best_distance {
            best_distance = distance;
            best = ansi;
        }
    }
    best
}

/// Squared euclidean distance between two colors in the Oklab perceptually-uniform
/// color space.
///
/// Used instead of raw RGB distance because Oklab distance correlates far better with
/// human perception of color difference (the same premise as CIEDE2000, without its
/// extra complexity).
#[cfg(feature = "indexed-quant")]
fn oklab_distance_sq(a: gem::space::Oklab, b: gem::space::Oklab) -> f32 {
    let dl = a.l - b.l;
    let da = a.a - b.a;
    let db = a.b - b.b;
    db.mul_add(db, da.mul_add(da, dl * dl))
}

/// Converts an 8-bit RGB channel triplet to Oklab.
#[cfg(feature = "indexed-quant")]
fn rgb_to_oklab(r: u8, g: u8, b: u8) -> gem::space::Oklab {
    gem::space::Oklab::from(Srgb::new(
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
    ))
}

/// Quantizes `(r, g, b)` to the nearest 256-color palette index using perceptual
/// (Oklab) distance, breaking ties by preferring the lower index.
#[cfg(feature = "indexed-quant")]
fn perceptual_to_indexed(r: u8, g: u8, b: u8) -> u8 {
    let target = rgb_to_oklab(r, g, b);
    let mut best_index = 0u8;
    let mut best_distance = f32::MAX;
    for index in 0u16..256 {
        let index = u8::try_from(index).unwrap_or(u8::MAX);
        let (pr, pg, pb) = indexed_to_rgb(index);
        let distance = oklab_distance_sq(target, rgb_to_oklab(pr, pg, pb));
        if distance < best_distance {
            best_distance = distance;
            best_index = index;
        }
    }
    best_index
}

/// Quantizes `(r, g, b)` to the nearest of the 16 standard ANSI colors using
/// perceptual (Oklab) distance, breaking ties by preferring the lower index.
#[cfg(feature = "indexed-quant")]
fn perceptual_to_ansi(r: u8, g: u8, b: u8) -> AnsiColor {
    let target = rgb_to_oklab(r, g, b);
    let mut best = AnsiColor::Black;
    let mut best_distance = f32::MAX;
    for ansi in ANSI_COLORS {
        let (pr, pg, pb) = ansi.to_rgb();
        let distance = oklab_distance_sq(target, rgb_to_oklab(pr, pg, pb));
        if distance < best_distance {
            best_distance = distance;
            best = ansi;
        }
    }
    best
}

/// Quantizes `(r, g, b)` to the nearest 256-color palette index, using perceptual
/// (Oklab) distance when the `indexed-quant` feature is enabled, or euclidean RGB
/// cube-mapping otherwise.
fn rgb_to_indexed(r: u8, g: u8, b: u8) -> u8 {
    #[cfg(feature = "indexed-quant")]
    {
        perceptual_to_indexed(r, g, b)
    }
    #[cfg(not(feature = "indexed-quant"))]
    {
        cube_map_to_indexed(r, g, b)
    }
}

/// Quantizes `(r, g, b)` to the nearest of the 16 standard ANSI colors, using
/// perceptual (Oklab) distance when the `indexed-quant` feature is enabled, or euclidean RGB
/// distance otherwise.
fn rgb_to_ansi(r: u8, g: u8, b: u8) -> AnsiColor {
    #[cfg(feature = "indexed-quant")]
    {
        perceptual_to_ansi(r, g, b)
    }
    #[cfg(not(feature = "indexed-quant"))]
    {
        cube_map_to_ansi(r, g, b)
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

impl core::error::Error for InvalidAnsiIndex {}

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
#[non_exhaustive]
/// Represents a color in the terminal grid.
///
/// # Examples
///
/// ```
/// use retroglyph_core::Color;
///
/// let named = Color::GREEN;
/// let rgb = Color::Rgb { r: 255, g: 0, b: 0 };
/// let indexed = Color::Indexed(42);
/// assert_ne!(named, rgb);
/// assert_ne!(rgb, indexed);
/// ```
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

    /// Resolves this color to a concrete 24-bit `(r, g, b)` triple, substituting `default` for
    /// [`Color::Default`].
    ///
    /// This is the canonical color-to-RGB resolution every graphical backend shares, so that a
    /// glyph drawn through the CPU rasterizer (`retroglyph-software`) and the GPU atlas
    /// (`retroglyph-gl`) comes out the same pixel color:
    ///
    /// - [`Rgb`](Self::Rgb) passes through unchanged.
    /// - [`Ansi`](Self::Ansi) resolves through [`AnsiColor::to_rgb`] (the one canonical ANSI
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
    #[cfg(feature = "indexed-quant")]
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
        Self::from_srgb(a_srgb.mix(b_srgb, t))
    }

    /// Lightens a color by `amount` (0.0 = no change, 1.0 = white).
    ///
    /// Non-`Rgb` variants are resolved to `(r, g, b)` via [`Color::resolve_rgb`] before the
    /// transform is applied, rather than being returned unchanged. [`Color::Default`] has no
    /// intrinsic RGB value, so it resolves to `(0, 0, 0)`.
    #[cfg(feature = "indexed-quant")]
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
        let (r, g, b) = self.resolve_rgb((0, 0, 0));
        inner(r, g, b, amount)
    }

    /// Darkens a color by `amount` (0.0 = no change, 1.0 = black).
    ///
    /// See [`Color::lighten`] for how non-`Rgb` variants are resolved before the transform.
    #[cfg(feature = "indexed-quant")]
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
        let (r, g, b) = self.resolve_rgb((0, 0, 0));
        inner(r, g, b, amount)
    }

    /// Increases saturation of a color by `amount` (0.0–1.0).
    ///
    /// See [`Color::lighten`] for how non-`Rgb` variants are resolved before the transform.
    #[cfg(feature = "indexed-quant")]
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
        let (r, g, b) = self.resolve_rgb((0, 0, 0));
        inner(r, g, b, amount)
    }

    /// Decreases saturation of a color by `amount` (0.0–1.0).
    ///
    /// See [`Color::lighten`] for how non-`Rgb` variants are resolved before the transform.
    #[cfg(feature = "indexed-quant")]
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
        let (r, g, b) = self.resolve_rgb((0, 0, 0));
        inner(r, g, b, amount)
    }

    /// Returns the complementary color (hue shifted by 180 degrees).
    ///
    /// See [`Color::lighten`] for how non-`Rgb` variants are resolved before the transform.
    #[cfg(feature = "indexed-quant")]
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
        let (r, g, b) = self.resolve_rgb((0, 0, 0));
        inner(r, g, b)
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

/// The name [`Color::fmt`] writes for an ANSI color, e.g. `"bright-red"`.
const fn ansi_name(color: AnsiColor) -> &'static str {
    match color {
        AnsiColor::Black => "black",
        AnsiColor::Red => "red",
        AnsiColor::Green => "green",
        AnsiColor::Yellow => "yellow",
        AnsiColor::Blue => "blue",
        AnsiColor::Magenta => "magenta",
        AnsiColor::Cyan => "cyan",
        AnsiColor::White => "white",
        AnsiColor::BrightBlack => "bright-black",
        AnsiColor::BrightRed => "bright-red",
        AnsiColor::BrightGreen => "bright-green",
        AnsiColor::BrightYellow => "bright-yellow",
        AnsiColor::BrightBlue => "bright-blue",
        AnsiColor::BrightMagenta => "bright-magenta",
        AnsiColor::BrightCyan => "bright-cyan",
        AnsiColor::BrightWhite => "bright-white",
    }
}

/// The inverse of [`ansi_name`]: matches a name with separators already stripped and lowercased
/// (e.g. `"brightred"`), as produced by [`Color::from_str`]'s normalization.
fn parse_ansi_name(name: &str) -> Option<AnsiColor> {
    Some(match name {
        "black" => AnsiColor::Black,
        "red" => AnsiColor::Red,
        "green" => AnsiColor::Green,
        "yellow" => AnsiColor::Yellow,
        "blue" => AnsiColor::Blue,
        "magenta" => AnsiColor::Magenta,
        "cyan" => AnsiColor::Cyan,
        "white" => AnsiColor::White,
        "brightblack" => AnsiColor::BrightBlack,
        "brightred" => AnsiColor::BrightRed,
        "brightgreen" => AnsiColor::BrightGreen,
        "brightyellow" => AnsiColor::BrightYellow,
        "brightblue" => AnsiColor::BrightBlue,
        "brightmagenta" => AnsiColor::BrightMagenta,
        "brightcyan" => AnsiColor::BrightCyan,
        "brightwhite" => AnsiColor::BrightWhite,
        _ => return None,
    })
}

/// Parses `#rgb` or `#rrggbb` (case-insensitive) into an `(r, g, b)` triple.
///
/// Self-contained rather than routed through [`Color::from_hex`], so [`Color`]'s
/// [`FromStr`](core::str::FromStr) impl -- and the `serde` feature built on it -- works
/// regardless of whether the `indexed-quant` feature (which `from_hex` needs for its
/// [`gem::space::Srgb`] parsing) is enabled.
fn parse_hex(s: &str) -> Option<(u8, u8, u8)> {
    let hex = s.strip_prefix('#')?;
    match hex.len() {
        3 => {
            let mut chars = hex.chars();
            let r = u8::try_from(chars.next()?.to_digit(16)?).ok()?;
            let g = u8::try_from(chars.next()?.to_digit(16)?).ok()?;
            let b = u8::try_from(chars.next()?.to_digit(16)?).ok()?;
            Some((r * 17, g * 17, b * 17))
        }
        6 => {
            let r = u8::from_str_radix(hex.get(0..2)?, 16).ok()?;
            let g = u8::from_str_radix(hex.get(2..4)?, 16).ok()?;
            let b = u8::from_str_radix(hex.get(4..6)?, 16).ok()?;
            Some((r, g, b))
        }
        _ => None,
    }
}

impl core::fmt::Display for Color {
    /// Writes the string form [`FromStr`](core::str::FromStr) parses back and, with the `serde`
    /// feature, [`Serialize`](serde::Serialize) writes: `"default"`, an ANSI name like
    /// `"bright-red"`, a palette index like `"42"`, or `#rrggbb` hex.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{AnsiColor, Color};
    ///
    /// assert_eq!(Color::Default.to_string(), "default");
    /// assert_eq!(Color::Ansi(AnsiColor::BrightRed).to_string(), "bright-red");
    /// assert_eq!(Color::Indexed(42).to_string(), "42");
    /// assert_eq!(Color::Rgb { r: 255, g: 128, b: 0 }.to_string(), "#ff8000");
    /// ```
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::Default => write!(f, "default"),
            Self::Ansi(color) => write!(f, "{}", ansi_name(color)),
            Self::Indexed(i) => write!(f, "{i}"),
            Self::Rgb { r, g, b } => write!(f, "#{r:02x}{g:02x}{b:02x}"),
        }
    }
}

/// Error returned when parsing a [`Color`] from a string fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParseColorError;

impl core::fmt::Display for ParseColorError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "invalid color string")
    }
}

impl core::error::Error for ParseColorError {}

impl core::str::FromStr for Color {
    type Err = ParseColorError;

    /// Parses the string form written by [`Display`](core::fmt::Display).
    ///
    /// Hyphens, underscores, and spaces are ignored and matching is case-insensitive, so
    /// `"BrightRed"`, `"bright red"`, and `"bright-red"` all parse the same as the canonical
    /// `"bright-red"` that [`Display`](core::fmt::Display) writes. `"reset"` is accepted as a
    /// synonym for `"default"`. Accepts `#rgb` in addition to the `#rrggbb` [`Display`](core::fmt::Display) writes.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_core::{AnsiColor, Color};
    ///
    /// assert_eq!("default".parse(), Ok(Color::Default));
    /// assert_eq!("reset".parse(), Ok(Color::Default));
    /// assert_eq!("bright-red".parse(), Ok(Color::Ansi(AnsiColor::BrightRed)));
    /// assert_eq!("Bright Red".parse(), Ok(Color::Ansi(AnsiColor::BrightRed)));
    /// assert_eq!("42".parse(), Ok(Color::Indexed(42)));
    /// assert_eq!("#ff8000".parse(), Ok(Color::Rgb { r: 255, g: 128, b: 0 }));
    /// assert_eq!("#f80".parse(), Ok(Color::Rgb { r: 255, g: 136, b: 0 }));
    /// assert!("not-a-color".parse::<Color>().is_err());
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        let normalized: String = trimmed
            .chars()
            .filter(|c| !matches!(c, '-' | '_' | ' '))
            .map(|c| c.to_ascii_lowercase())
            .collect();

        if normalized == "default" || normalized == "reset" {
            return Ok(Self::Default);
        }
        if let Some(color) = parse_ansi_name(&normalized) {
            return Ok(Self::Ansi(color));
        }
        if let Ok(index) = normalized.parse::<u8>() {
            return Ok(Self::Indexed(index));
        }
        if let Some((r, g, b)) = parse_hex(trimmed) {
            return Ok(Self::Rgb { r, g, b });
        }
        Err(ParseColorError)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Color {
    /// Serializes through the [`Display`](core::fmt::Display) round trip, e.g. `"bright-red"` or
    /// `"#ff8000"`, rather than deriving a structural form -- so a hand-edited TOML/JSON theme
    /// file stays legible and isn't coupled to this (`#[non_exhaustive]`) enum's variant shape.
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use alloc::string::ToString as _;

        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Color {
    /// Deserializes through [`FromStr`](core::str::FromStr), so every string
    /// [`Display`](core::fmt::Display) can produce, plus every alias [`FromStr`](core::str::FromStr)
    /// additionally accepts (e.g. `"BrightRed"`, `"#f80"`), round-trips.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ColorVisitor;

        impl serde::de::Visitor<'_> for ColorVisitor {
            type Value = Color;

            fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str(
                    "a color string: \"default\", an ANSI name, a palette index, or #rrggbb hex",
                )
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                v.parse().map_err(E::custom)
            }
        }

        deserializer.deserialize_str(ColorVisitor)
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

    // ── cube-mapping fallback (always tested, regardless of `indexed-quant` feature) ─

    #[test]
    fn test_nearest_cube_step_boundaries() {
        assert_eq!(nearest_cube_step(0), 0);
        assert_eq!(nearest_cube_step(255), 5);
        assert_eq!(nearest_cube_step(95), 1);
        assert_eq!(nearest_cube_step(135), 2);
    }

    #[test]
    fn test_indexed_to_rgb_ansi_range() {
        assert_eq!(indexed_to_rgb(0), (0, 0, 0));
        assert_eq!(indexed_to_rgb(15), (255, 255, 255));
    }

    #[test]
    fn test_indexed_to_rgb_cube_range() {
        // Index 16 is the cube origin (0, 0, 0).
        assert_eq!(indexed_to_rgb(16), (0, 0, 0));
        // Index 231 is the cube's opposite corner (255, 255, 255).
        assert_eq!(indexed_to_rgb(231), (255, 255, 255));
    }

    #[test]
    fn test_indexed_to_rgb_grayscale_range() {
        assert_eq!(indexed_to_rgb(232), (8, 8, 8));
        assert_eq!(indexed_to_rgb(255), (238, 238, 238));
    }

    #[test]
    fn test_cube_map_to_indexed_pure_black() {
        assert_eq!(cube_map_to_indexed(0, 0, 0), 0);
    }

    #[test]
    fn test_cube_map_to_indexed_pure_white() {
        assert_eq!(cube_map_to_indexed(255, 255, 255), 15);
    }

    #[test]
    fn test_cube_map_to_indexed_cube_interior() {
        // A color exactly on a cube step should map to that exact cube index.
        // (95, 135, 175) -> cube coords (1, 2, 3) -> 16 + 36*1 + 6*2 + 3 = 67.
        assert_eq!(cube_map_to_indexed(95, 135, 175), 67);
    }

    #[test]
    fn test_cube_map_to_ansi_matches_reference() {
        for ansi in ANSI_COLORS {
            let (r, g, b) = ansi.to_rgb();
            assert_eq!(cube_map_to_ansi(r, g, b), ansi, "ansi color {ansi:?}");
        }
    }

    #[test]
    fn test_rgb_distance_sq_symmetry() {
        let a = (10, 20, 30);
        let b = (200, 100, 50);
        assert_eq!(gem::rgb::distance_sq(a, b), gem::rgb::distance_sq(b, a));
    }

    #[test]
    fn test_rgb_distance_sq_zero_for_identical() {
        assert_eq!(gem::rgb::distance_sq((1, 2, 3), (1, 2, 3)), 0);
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

    #[test]
    fn test_display() {
        assert_eq!(Color::Default.to_string(), "default");
        assert_eq!(Color::Ansi(AnsiColor::Black).to_string(), "black");
        assert_eq!(
            Color::Ansi(AnsiColor::BrightYellow).to_string(),
            "bright-yellow"
        );
        assert_eq!(Color::Indexed(7).to_string(), "7");
        assert_eq!(
            Color::Rgb {
                r: 255,
                g: 128,
                b: 0
            }
            .to_string(),
            "#ff8000"
        );
    }

    #[test]
    fn test_from_str_round_trips_display() {
        let colors = [
            Color::Default,
            Color::Ansi(AnsiColor::Black),
            Color::Ansi(AnsiColor::BrightMagenta),
            Color::Indexed(200),
            Color::Rgb {
                r: 18,
                g: 52,
                b: 86,
            },
        ];
        for color in colors {
            let s = color.to_string();
            assert_eq!(s.parse(), Ok(color), "round trip of {s:?}");
        }
    }

    #[test]
    fn test_from_str_aliases_and_separators() {
        assert_eq!("reset".parse(), Ok(Color::Default));
        assert_eq!("Default".parse(), Ok(Color::Default));
        assert_eq!("BrightRed".parse(), Ok(Color::Ansi(AnsiColor::BrightRed)));
        assert_eq!("bright red".parse(), Ok(Color::Ansi(AnsiColor::BrightRed)));
        assert_eq!("bright_red".parse(), Ok(Color::Ansi(AnsiColor::BrightRed)));
        assert_eq!(
            "#F80".parse(),
            Ok(Color::Rgb {
                r: 255,
                g: 136,
                b: 0
            })
        );
        assert_eq!(
            " #ff8000 ".parse(),
            Ok(Color::Rgb {
                r: 255,
                g: 128,
                b: 0
            })
        );
    }

    #[test]
    fn test_from_str_invalid() {
        assert!("not-a-color".parse::<Color>().is_err());
        assert!("#gg0000".parse::<Color>().is_err());
        assert!("#12345".parse::<Color>().is_err());
        assert!("256".parse::<Color>().is_err());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_serialize_then_deserialize() -> Result<(), serde_json::Error> {
        let colors = [
            Color::Default,
            Color::Ansi(AnsiColor::BrightGreen),
            Color::Indexed(42),
            Color::Rgb {
                r: 255,
                g: 0,
                b: 255,
            },
        ];
        for color in colors {
            let json = serde_json::to_string(&color)?;
            assert_eq!(serde_json::from_str::<Color>(&json)?, color);
        }

        assert_eq!(
            serde_json::to_string(&Color::Rgb {
                r: 255,
                g: 0,
                b: 255
            })?,
            r##""#ff00ff""##
        );
        assert_eq!(
            serde_json::from_str::<Color>(r#""bright-white""#)?,
            Color::Ansi(AnsiColor::BrightWhite)
        );
        assert!(serde_json::from_str::<Color>(r#""not-a-color""#).is_err());

        Ok(())
    }
}
