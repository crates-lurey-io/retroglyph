//! The 16-color ANSI palette, and the shared indexed/ANSI quantization machinery `Color`'s
//! `to_indexed`/`to_ansi`/`resolve_rgb` build on.

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
/// use retroglyph_core::color::{AnsiColor, Color};
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
    /// These are the same 16 reference colors used by [`Color::to_indexed`](super::Color::to_indexed) and
    /// [`Color::to_ansi`](super::Color::to_ansi) when quantizing RGB input; a terminal's actual theme may
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

/// All 16 [`AnsiColor`](crate::color::AnsiColor) variants in index order (0–15), for iterating the palette.
pub(super) const ANSI_COLORS: [AnsiColor; 16] = [
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
/// Not feature-gated: [`Color::resolve_rgb`](super::Color::resolve_rgb) calls it on every build, so it must always compile
/// regardless of the `indexed-quant` feature (the `to_indexed`-family callers below are `gem`-gated, but
/// this table lookup itself has no `gem` dependency).
pub(super) const fn indexed_to_rgb(index: u8) -> (u8, u8, u8) {
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
/// Ties (exactly halfway between two steps) round to the lower step: steps are
/// scanned in ascending order and only a strictly closer step replaces the
/// current best, so an equal-distance higher step never wins.
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
/// This is the fallback used by [`Color::to_indexed`](super::Color::to_indexed) without the `indexed-quant` feature, and
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
/// This is the fallback used by [`Color::to_ansi`](super::Color::to_ansi) without the `indexed-quant` feature, and is
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

/// Converts an 8-bit RGB channel triplet to `gem::space::Srgb`, the shared conversion behind
/// every `Srgb::new(f32::from(r) / 255.0, ...)` call site in this module.
#[cfg(feature = "indexed-quant")]
pub(super) fn rgb_to_srgb(r: u8, g: u8, b: u8) -> Srgb {
    Srgb::new(
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
    )
}

/// Converts an 8-bit RGB channel triplet to Oklab.
#[cfg(feature = "indexed-quant")]
fn rgb_to_oklab(r: u8, g: u8, b: u8) -> gem::space::Oklab {
    gem::space::Oklab::from(rgb_to_srgb(r, g, b))
}

/// Builds the 256-entry table of [`gem::space::Oklab`] values for the 256-color palette
/// (`indexed_to_rgb(0..256)` converted to Oklab), in index order.
#[cfg(feature = "indexed-quant")]
fn build_indexed_oklab_table() -> [gem::space::Oklab; 256] {
    core::array::from_fn(|i| {
        let (r, g, b) = indexed_to_rgb(u8::try_from(i).unwrap_or(u8::MAX));
        rgb_to_oklab(r, g, b)
    })
}

/// Builds the 16-entry table of [`gem::space::Oklab`] values for the 16 standard ANSI
/// colors ([`ANSI_COLORS`] converted to Oklab), in index order.
#[cfg(feature = "indexed-quant")]
fn build_ansi_oklab_table() -> [gem::space::Oklab; 16] {
    core::array::from_fn(|i| {
        let (r, g, b) = ANSI_COLORS[i].to_rgb();
        rgb_to_oklab(r, g, b)
    })
}

/// The 256-color palette's Oklab table, as returned by [`indexed_oklab_table`]: a cached
/// `'static` reference when `std` is enabled, or an owned array rebuilt per call otherwise.
#[cfg(all(feature = "indexed-quant", feature = "std"))]
type IndexedOklabTable = &'static [gem::space::Oklab; 256];
#[cfg(all(feature = "indexed-quant", not(feature = "std")))]
type IndexedOklabTable = [gem::space::Oklab; 256];

/// The 16-color ANSI palette's Oklab table, as returned by [`ansi_oklab_table`]: a cached
/// `'static` reference when `std` is enabled, or an owned array rebuilt per call otherwise.
#[cfg(all(feature = "indexed-quant", feature = "std"))]
type AnsiOklabTable = &'static [gem::space::Oklab; 16];
#[cfg(all(feature = "indexed-quant", not(feature = "std")))]
type AnsiOklabTable = [gem::space::Oklab; 16];

/// Returns the 256-color palette's Oklab table.
///
/// `indexed_to_rgb` is a pure function of a compile-time-constant palette, but its Oklab
/// conversion (`powf`/`cbrt`) isn't `const`-evaluable, so the table can't be a plain `const`.
/// When the `std` feature is enabled, this caches the table behind a `OnceLock` so the
/// conversion only ever runs once for all 256 entries, no matter how many times
/// [`Color::to_indexed`](super::Color::to_indexed) is called; the hot path this feeds ([`ColorSupport::Indexed256`] in
/// `retroglyph-terminal`'s draw loop) calls it once per cell, per frame.
///
/// `no_std` builds (this crate's `std` feature off) have no safe way to back a lazily
/// initialized `static` without `unsafe` (forbidden workspace-wide), so they rebuild the table
/// on every call instead; that path is expected to be cold.
#[cfg(feature = "indexed-quant")]
fn indexed_oklab_table() -> IndexedOklabTable {
    #[cfg(feature = "std")]
    {
        static TABLE: std::sync::OnceLock<[gem::space::Oklab; 256]> = std::sync::OnceLock::new();
        TABLE.get_or_init(build_indexed_oklab_table)
    }
    #[cfg(not(feature = "std"))]
    {
        build_indexed_oklab_table()
    }
}

/// Returns the 16-color ANSI palette's Oklab table. See [`indexed_oklab_table`] for the
/// caching rationale and the `no_std` fallback.
#[cfg(feature = "indexed-quant")]
fn ansi_oklab_table() -> AnsiOklabTable {
    #[cfg(feature = "std")]
    {
        static TABLE: std::sync::OnceLock<[gem::space::Oklab; 16]> = std::sync::OnceLock::new();
        TABLE.get_or_init(build_ansi_oklab_table)
    }
    #[cfg(not(feature = "std"))]
    {
        build_ansi_oklab_table()
    }
}

/// Quantizes `(r, g, b)` to the nearest 256-color palette index using perceptual
/// (Oklab) distance, breaking ties by preferring the lower index.
#[cfg(feature = "indexed-quant")]
fn perceptual_to_indexed(r: u8, g: u8, b: u8) -> u8 {
    let target = rgb_to_oklab(r, g, b);
    let table = indexed_oklab_table();
    let mut best_index = 0u8;
    let mut best_distance = f32::MAX;
    for index in 0u16..256 {
        let index = u8::try_from(index).unwrap_or(u8::MAX);
        let distance = target.distance_sq(table[index as usize]);
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
    let table = ansi_oklab_table();
    let mut best = AnsiColor::Black;
    let mut best_distance = f32::MAX;
    for (i, ansi) in ANSI_COLORS.iter().enumerate() {
        let distance = target.distance_sq(table[i]);
        if distance < best_distance {
            best_distance = distance;
            best = *ansi;
        }
    }
    best
}

/// Quantizes `(r, g, b)` to the nearest 256-color palette index, using perceptual
/// (Oklab) distance when the `indexed-quant` feature is enabled, or euclidean RGB
/// cube-mapping otherwise.
pub(super) fn rgb_to_indexed(r: u8, g: u8, b: u8) -> u8 {
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
pub(super) fn rgb_to_ansi(r: u8, g: u8, b: u8) -> AnsiColor {
    #[cfg(feature = "indexed-quant")]
    {
        perceptual_to_ansi(r, g, b)
    }
    #[cfg(not(feature = "indexed-quant"))]
    {
        cube_map_to_ansi(r, g, b)
    }
}

/// Error returned when a `u8` value has no corresponding [`AnsiColor`](crate::color::AnsiColor).
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
