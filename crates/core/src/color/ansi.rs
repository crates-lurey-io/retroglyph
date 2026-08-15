//! The 16-color ANSI palette, and the shared indexed/ANSI quantization machinery `Color`'s
//! `to_indexed`/`to_ansi`/`resolve_rgb` build on, including the [`Quantize`] metric that picks
//! between them.
//!
//! The palette values here are the de-facto-standard xterm 256-color palette (the 16 ANSI
//! defaults plus the 6x6x6 cube and 24-step gray ramp), the same numbers every other terminal
//! matches. See the 8-bit color table at
//! <https://en.wikipedia.org/wiki/ANSI_escape_code#8-bit>. They are a fixed external palette, not
//! values this crate is free to retune: changing one changes what every RGB input quantizes to and
//! desyncs retroglyph's output from every other terminal's rendering of the same index.

use gem::space::Srgb;

use super::palette_oklab::PALETTE_OKLAB;

/// The distance metric [`Color::to_indexed_with`](super::Color::to_indexed_with) and
/// [`Color::to_ansi_with`](super::Color::to_ansi_with) use to find a palette entry's nearest
/// neighbour.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Quantize {
    /// Euclidean distance in the Oklab perceptually-uniform color space.
    ///
    /// Matches human color perception far better than raw RGB distance, at the cost of
    /// converting the input color to Oklab (three `powf` and three `cbrt`) on every call. The
    /// palette side of the comparison is precomputed, so that conversion is the whole cost.
    ///
    /// The default, and what [`Color::to_indexed`](super::Color::to_indexed) and
    /// [`Color::to_ansi`](super::Color::to_ansi) use.
    #[default]
    Perceptual,

    /// Euclidean distance over the raw 8-bit RGB channels.
    ///
    /// Integer-only and allocation-free, and for [`Color::to_indexed_with`](super::Color::to_indexed_with)
    /// it finds the 6x6x6 cube's nearest point by rounding each channel independently rather than
    /// scanning the cube. Perceptually worse than [`Perceptual`](Self::Perceptual) (it
    /// over-weights green and under-weights blue), but it's the rule most terminal tooling
    /// applies, so it's the one to pick when the output has to agree with another tool's
    /// downgrade.
    Euclidean,
}

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
    /// These are the classic xterm defaults, the same 16 reference colors used by
    /// [`Color::to_indexed`](super::Color::to_indexed) and [`Color::to_ansi`](super::Color::to_ansi)
    /// when quantizing RGB input. xterm's own defaults have shifted across versions, and a
    /// terminal's actual theme may render these colors differently still.
    #[must_use]
    pub const fn to_rgb(self) -> (u8, u8, u8) {
        ANSI_TABLE[self as usize].2
    }
}

/// Every [`AnsiColor`] variant's canonical [`Display`](core::fmt::Display) name and xterm
/// reference RGB, in discriminant order (0-15).
///
/// The single source of truth [`AnsiColor::to_rgb`], [`TryFrom<u8>`](AnsiColor), and the ANSI
/// name/parse pair in `parse.rs` all derive from, so a name or RGB value only ever needs
/// transcribing once.
pub(super) const ANSI_TABLE: [(AnsiColor, &str, (u8, u8, u8)); 16] = [
    (AnsiColor::Black, "black", (0, 0, 0)),
    (AnsiColor::Red, "red", (205, 0, 0)),
    (AnsiColor::Green, "green", (0, 205, 0)),
    (AnsiColor::Yellow, "yellow", (205, 205, 0)),
    (AnsiColor::Blue, "blue", (0, 0, 238)),
    (AnsiColor::Magenta, "magenta", (205, 0, 205)),
    (AnsiColor::Cyan, "cyan", (0, 205, 205)),
    (AnsiColor::White, "white", (229, 229, 229)),
    (AnsiColor::BrightBlack, "bright-black", (127, 127, 127)),
    (AnsiColor::BrightRed, "bright-red", (255, 0, 0)),
    (AnsiColor::BrightGreen, "bright-green", (0, 255, 0)),
    (AnsiColor::BrightYellow, "bright-yellow", (255, 255, 0)),
    (AnsiColor::BrightBlue, "bright-blue", (92, 92, 255)),
    (AnsiColor::BrightMagenta, "bright-magenta", (255, 0, 255)),
    (AnsiColor::BrightCyan, "bright-cyan", (0, 255, 255)),
    (AnsiColor::BrightWhite, "bright-white", (255, 255, 255)),
];

/// All 16 [`AnsiColor`](crate::color::AnsiColor) variants in index order (0–15), for iterating the palette.
pub(super) const ANSI_COLORS: [AnsiColor; 16] = {
    let mut colors = [AnsiColor::Black; 16];
    let mut i = 0;
    while i < ANSI_TABLE.len() {
        colors[i] = ANSI_TABLE[i].0;
        i += 1;
    }
    colors
};

/// The 6 steps used for each channel of the 256-color palette's 6x6x6 RGB cube
/// (indices 16-231).
///
/// The five non-zero steps follow xterm's `55 + 40 * n` for `n` in `1..=5`; step 0 is a true 0,
/// not `55 - 40`. Do not "regularize" these to evenly spaced values: they must match the xterm
/// palette other terminals use.
const CUBE_STEPS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// The 24 grayscale ramp values used by the 256-color palette (indices 232-255).
///
/// xterm's ramp: `8 + 10 * n` for `n` in `0..=23`, so it runs 8..=238 and never reaches pure
/// black or pure white (those live in the cube and the ANSI set).
const GRAYSCALE_RAMP: [u8; 24] = [
    8, 18, 28, 38, 48, 58, 68, 78, 88, 98, 108, 118, 128, 138, 148, 158, 168, 178, 188, 198, 208,
    218, 228, 238,
];

/// Returns the RGB value for a 256-color palette index (0–255).
///
/// Indices 0–15 are the 16 standard ANSI colors, 16–231 are the 6×6×6 RGB cube, and
/// 232–255 are the grayscale ramp.
///
/// `const` and integer-only: [`Color::resolve_rgb`](super::Color::resolve_rgb) calls it for every
/// [`Indexed`](super::Color::Indexed) tile a pixel backend draws, and it's also what generates
/// [`PALETTE_OKLAB`].
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

/// Picks the candidate with the smallest `D`, preferring the lower index on ties (strict `<`
/// only). Index candidates with an inclusive range (`data.zip(start..=u8::MAX)`), not
/// `start..`: an open `RangeFrom<u8>` computes one-past-the-end on the call that yields 255
/// itself and overflows, since `u8` has no representable "256" to stop at.
fn nearest<D: PartialOrd>(candidates: impl IntoIterator<Item = (u8, D)>) -> u8 {
    candidates
        .into_iter()
        .reduce(|a, b| if b.1 < a.1 { b } else { a })
        .map_or(0, |(i, _)| i)
}

/// Rounds `value` to the nearest of the 6 [`CUBE_STEPS`], returning the step's index
/// (0–5).
///
/// Ties (exactly halfway between two steps) round to the lower step: see [`nearest`].
fn nearest_cube_step(value: u8) -> u8 {
    let value = i32::from(value);
    nearest(
        CUBE_STEPS
            .iter()
            .zip(0u8..)
            .map(|(&step, i)| (i, (value - i32::from(step)).abs())),
    )
}

/// Quantizes `(r, g, b)` to the nearest 256-color palette index using the 6×6×6 RGB
/// cube, grayscale ramp, and the 16 ANSI colors, breaking ties by preferring the
/// lower index.
///
/// Backs [`Quantize::Euclidean`] for [`Color::to_indexed_with`](super::Color::to_indexed_with).
///
/// Checks the 16 ANSI colors, the cube's single nearest point (found by rounding each
/// channel independently), and the grayscale ramp's single nearest point, rather than
/// scanning all 256 entries individually: rounding each channel independently already
/// finds the cube's closest point, and likewise for the single-channel grayscale ramp.
/// Candidates are passed to [`nearest`] in ascending index order (ANSI, cube, ramp), so
/// ties naturally resolve to the lower index.
fn cube_map_to_indexed(r: u8, g: u8, b: u8) -> u8 {
    // Nearest point in the 6x6x6 cube (candidate group 2, indices 16-231).
    let cube_r = nearest_cube_step(r);
    let cube_g = nearest_cube_step(g);
    let cube_b = nearest_cube_step(b);
    let cube_index = 16 + 36 * cube_r + 6 * cube_g + cube_b;
    let cube_rgb = (
        CUBE_STEPS[cube_r as usize],
        CUBE_STEPS[cube_g as usize],
        CUBE_STEPS[cube_b as usize],
    );

    nearest(
        // Candidate group 1: the 16 ANSI colors (indices 0-15), lowest indices first.
        ANSI_COLORS
            .iter()
            .zip(0u8..)
            .map(|(ansi, i)| (i, gem::rgb::distance_sq((r, g, b), ansi.to_rgb())))
            .chain(core::iter::once((
                cube_index,
                gem::rgb::distance_sq((r, g, b), cube_rgb),
            )))
            // Candidate group 3: nearest grayscale ramp entry (indices 232-255).
            .chain(
                GRAYSCALE_RAMP
                    .iter()
                    .zip(232u8..=u8::MAX)
                    .map(|(&gray, i)| (i, gem::rgb::distance_sq((r, g, b), (gray, gray, gray)))),
            ),
    )
}

/// Quantizes `(r, g, b)` to the nearest of the 16 standard ANSI colors, using
/// euclidean RGB distance and breaking ties by preferring the lower index.
///
/// Backs [`Quantize::Euclidean`] for [`Color::to_ansi_with`](super::Color::to_ansi_with).
fn cube_map_to_ansi(r: u8, g: u8, b: u8) -> AnsiColor {
    let i = nearest(
        ANSI_COLORS
            .iter()
            .zip(0u8..)
            .map(|(ansi, i)| (i, gem::rgb::distance_sq((r, g, b), ansi.to_rgb()))),
    );
    ANSI_COLORS[i as usize]
}

/// Converts an 8-bit RGB channel triplet to `gem::space::Srgb`, the shared conversion behind
/// every `Srgb::new(f32::from(r) / 255.0, ...)` call site in this module.
pub(super) fn rgb_to_srgb(r: u8, g: u8, b: u8) -> Srgb {
    Srgb::new(
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
    )
}

/// Converts an 8-bit RGB channel triplet to Oklab.
fn rgb_to_oklab(r: u8, g: u8, b: u8) -> gem::space::Oklab {
    gem::space::Oklab::from(rgb_to_srgb(r, g, b))
}

/// Quantizes `(r, g, b)` to the nearest 256-color palette index using perceptual
/// (Oklab) distance, breaking ties by preferring the lower index.
fn perceptual_to_indexed(r: u8, g: u8, b: u8) -> u8 {
    let target = rgb_to_oklab(r, g, b);
    nearest(
        PALETTE_OKLAB
            .iter()
            .zip(0u8..=u8::MAX)
            .map(|(&entry, i)| (i, target.distance_sq(entry))),
    )
}

/// Quantizes `(r, g, b)` to the nearest of the 16 standard ANSI colors using
/// perceptual (Oklab) distance, breaking ties by preferring the lower index.
///
/// Searches [`PALETTE_OKLAB`]'s first 16 entries rather than a table of its own: the 256-color
/// palette opens with the 16 ANSI colors in [`ANSI_COLORS`] order, so those entries already are
/// the ANSI palette's Oklab values.
fn perceptual_to_ansi(r: u8, g: u8, b: u8) -> AnsiColor {
    let target = rgb_to_oklab(r, g, b);
    let i = nearest(
        PALETTE_OKLAB
            .iter()
            .zip(0u8..)
            .take(ANSI_COLORS.len())
            .map(|(&entry, i)| (i, target.distance_sq(entry))),
    );
    ANSI_COLORS[i as usize]
}

/// Quantizes `(r, g, b)` to the nearest 256-color palette index under `metric`, breaking ties by
/// preferring the lower index.
pub(super) fn rgb_to_indexed(r: u8, g: u8, b: u8, metric: Quantize) -> u8 {
    match metric {
        Quantize::Euclidean => cube_map_to_indexed(r, g, b),
        // `Quantize` is `#[non_exhaustive]`: an unrecognized future metric falls back to the
        // default rather than failing to compile.
        _ => perceptual_to_indexed(r, g, b),
    }
}

/// Quantizes `(r, g, b)` to the nearest of the 16 standard ANSI colors under `metric`, breaking
/// ties by preferring the lower index.
pub(super) fn rgb_to_ansi(r: u8, g: u8, b: u8, metric: Quantize) -> AnsiColor {
    match metric {
        Quantize::Euclidean => cube_map_to_ansi(r, g, b),
        // See `rgb_to_indexed` above for why this isn't an exhaustive match.
        _ => perceptual_to_ansi(r, g, b),
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
        ANSI_TABLE
            .get(v as usize)
            .map(|entry| entry.0)
            .ok_or(InvalidAnsiIndex(v))
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

    // ── the generated Oklab palette table ────────────────────────────────────────────

    /// The largest per-channel deviation tolerated between a [`PALETTE_OKLAB`] entry and the same
    /// color converted at runtime.
    ///
    /// Not zero, because the table's literals were generated under one float backend and the
    /// comparison runs under whichever of `std`/`libm` the build selected; those disagree by a few
    /// ULP in `powf`/`cbrt`. Far tighter than the gap between any two palette entries, so a
    /// genuinely wrong or stale entry still fails.
    const PALETTE_EPSILON: f32 = 1e-5;

    #[test]
    fn test_palette_oklab_matches_computed_table() {
        for (index, &entry) in PALETTE_OKLAB.iter().enumerate() {
            let (r, g, b) = indexed_to_rgb(u8::try_from(index).expect("index is 0..256"));
            let computed = rgb_to_oklab(r, g, b);
            for (label, generated, computed) in [
                ("l", entry.l, computed.l),
                ("a", entry.a, computed.a),
                ("b", entry.b, computed.b),
            ] {
                assert!(
                    (generated - computed).abs() <= PALETTE_EPSILON,
                    "palette entry {index} channel {label}: {generated} != {computed}"
                );
            }
        }
    }

    /// Quantization itself, not just the table, must be unchanged by using generated literals:
    /// a nearest-neighbour search only cares about the *ordering* of distances, so an entry could
    /// drift within [`PALETTE_EPSILON`] and still flip a near-tie.
    #[test]
    fn test_palette_oklab_quantizes_identically_to_computed_table() {
        let computed: [gem::space::Oklab; 256] = core::array::from_fn(|i| {
            let (r, g, b) = indexed_to_rgb(u8::try_from(i).expect("index is 0..256"));
            rgb_to_oklab(r, g, b)
        });

        // Every 17th value per channel: the 16^3 grid hits both palette entries and the midpoints
        // between them, where a tie is most likely to flip.
        for r in (0..=255u8).step_by(17) {
            for g in (0..=255u8).step_by(17) {
                for b in (0..=255u8).step_by(17) {
                    let target = rgb_to_oklab(r, g, b);
                    let nearest = |table: &[gem::space::Oklab; 256]| {
                        let mut best = (0usize, f32::MAX);
                        for (i, &entry) in table.iter().enumerate() {
                            let distance = target.distance_sq(entry);
                            if distance < best.1 {
                                best = (i, distance);
                            }
                        }
                        best.0
                    };
                    assert_eq!(
                        nearest(&PALETTE_OKLAB),
                        nearest(&computed),
                        "rgb({r}, {g}, {b})"
                    );
                }
            }
        }
    }

    // ── `Quantize::Euclidean`'s cube-mapping ────────────────────────────────────────────

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
