//! Posterizes small blocks of raw pixels to the best-matching Unicode block-element glyph.
//!
//! This is the "subcell" technique used by `doryen-rs` (`blit_2x`), libtcod, and notcurses'
//! blitter chain to render raster images as text without a tileset: split a source image into
//! one small pixel block per terminal cell, then pick whichever glyph plus foreground/background
//! color pair best reconstructs that block. Three block shapes are supported, in increasing
//! fidelity (and decreasing terminal compatibility):
//!
//! - [`quantize_half_block`]: 1x2 pixels -> `' '`/`▀`/`▄`/`█` (Unicode Block Elements, supported
//!   almost everywhere monospace fonts render at all).
//! - [`quantize_quadrant`]: 2x2 pixels -> the 16 quadrant block characters (`▘▝▀▖▌▞▛...`).
//! - [`quantize_sextant`]: 2x3 pixels -> the 64 "Symbols for Legacy Computing" sextant
//!   characters, doubling vertical resolution again over quadrants. Newest and least
//!   universally supported of the three (a 2022 Unicode addition).
//!
//! Callers own the fallback chain: probe terminal/font support (or just take a caller-supplied
//! capability flag) and call whichever function matches, sampling the source image at that
//! function's pixel geometry. There's no single "auto-detect and degrade" entry point here,
//! matching every other terminal-capability decision in retroglyph (e.g. `egc` support) --
//! detection policy lives with the backend, not with this pure geometry/color utility.
//!
//! # Algorithm
//!
//! For an N-pixel block, every one of the `2^N` ways to split the block into a "foreground set"
//! and "background set" is scored: average the two sets' colors, then sum each pixel's squared
//! distance to whichever average it was assigned to. The split with the lowest total error wins,
//! and its bit pattern selects the glyph directly (the glyph tables below are indexed by that
//! same pattern, foreground bits set, read row-major). This exhaustive search is cheap here (at
//! most 64 candidates, 6 pixels each, for [`quantize_sextant`]) and is the same technique
//! notcurses' blitter chain documents using for its own 3x2 sextant solver.
//!
//! Ties (multiple patterns reconstructing a block with equally minimal error) resolve to the
//! lower-numbered pattern, matching the tie-break convention `retroglyph_core::color`'s own
//! nearest-color search already uses. Two tie shapes come up often enough to call out: a flat,
//! single-color block ties across every pattern (all give zero error) and always resolves to
//! pattern `0`, the cheapest glyph -- a plain space colored by `bg`. And any block with exactly
//! two distinct pixel colors has exactly two zero-error patterns, one the bitwise complement of
//! the other (swap which color is called `fg` and which is `bg` and the reconstruction is
//! identical); the lower pattern number wins there too.
//!
//! # Provenance
//!
//! The glyph tables ([`HALF_BLOCKS`], [`QUADRANTS`], [`SEXTANTS`]) are adapted from
//! [ratatui-core's `symbols::pixel` module](https://github.com/ratatui/ratatui/blob/main/ratatui-core/src/symbols/pixel.rs)
//! (MIT-licensed, like retroglyph), which lists them by bit pattern rather than by Unicode
//! codepoint order -- the sextant block in particular is not contiguous or monotonic in Unicode
//! (four combinations reuse the pre-existing Block Elements `' '`, `█`, `▌`, `▐` instead of
//! having their own Legacy Computing codepoints), so a hand-rolled table is where subtle,
//! hard-to-spot-in-review bugs live. Reusing a table already exercised by a widely-used library
//! is deliberate risk reduction, not just convenience.
//!
//! # Example
//!
//! ```
//! use retroglyph_core::subcell::quantize_quadrant;
//!
//! // A block that's white in the top-left corner, black everywhere else.
//! let black = (0, 0, 0);
//! let white = (255, 255, 255);
//! let glyph = quantize_quadrant([white, black, black, black]);
//! assert_eq!(glyph.ch, '▘'); // top-left quadrant
//! assert_eq!(glyph.fg, retroglyph_core::Color::Rgb { r: 255, g: 255, b: 255 });
//! assert_eq!(glyph.bg, retroglyph_core::Color::Rgb { r: 0, g: 0, b: 0 });
//! ```

use crate::color::Color;

/// A raw 24-bit RGB pixel sample: `(r, g, b)`, one byte per channel.
pub type Rgb = (u8, u8, u8);

/// The Unicode Block Elements glyphs for a 1-wide x 2-tall pixel block, indexed by a 2-bit
/// pattern (bit 0 = top pixel set, bit 1 = bottom pixel set).
pub const HALF_BLOCKS: [char; 4] = [' ', '▀', '▄', '█'];

/// The 16 quadrant block glyphs for a 2x2 pixel block, indexed by a 4-bit pattern in row-major
/// order (bit 0 = top-left, bit 1 = top-right, bit 2 = bottom-left, bit 3 = bottom-right).
///
/// Adapted from [ratatui-core's `symbols::pixel::QUADRANTS`][ratatui].
///
/// [ratatui]: https://github.com/ratatui/ratatui/blob/main/ratatui-core/src/symbols/pixel.rs
pub const QUADRANTS: [char; 16] = [
    ' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█',
];

/// The 64 sextant glyphs for a 2x3 pixel block.
///
/// Indexed by a 6-bit pattern in row-major order (bit 0 = top-left, bit 1 = top-right, bit 2 =
/// mid-left, bit 3 = mid-right, bit 4 = bottom-left, bit 5 = bottom-right). Mostly from Unicode's
/// "Symbols for Legacy Computing" block; adapted from [ratatui-core's
/// `symbols::pixel::SEXTANTS`][ratatui].
///
/// [ratatui]: https://github.com/ratatui/ratatui/blob/main/ratatui-core/src/symbols/pixel.rs
#[rustfmt::skip]
pub const SEXTANTS: [char; 64] = [
    ' ', '🬀', '🬁', '🬂', '🬃', '🬄', '🬅', '🬆', '🬇', '🬈', '🬉', '🬊', '🬋', '🬌', '🬍', '🬎',
    '🬏', '🬐', '🬑', '🬒', '🬓', '▌', '🬔', '🬕', '🬖', '🬗', '🬘', '🬙', '🬚', '🬛', '🬜', '🬝',
    '🬞', '🬟', '🬠', '🬡', '🬢', '🬣', '🬤', '🬥', '🬦', '🬧', '▐', '🬨', '🬩', '🬪', '🬫', '🬬',
    '🬭', '🬮', '🬯', '🬰', '🬱', '🬲', '🬳', '🬴', '🬵', '🬶', '🬷', '🬸', '🬹', '🬺', '🬻', '█',
];

/// A posterized pixel block: the best-matching glyph plus its foreground and background colors.
///
/// The background color is only meaningful for glyphs that don't cover the full cell (anything
/// but `' '` and `'█'`); the foreground color is only meaningful for glyphs other than `' '`.
/// Both are still populated for those edge cases (as the block's overall average color) so a
/// caller never has to special-case `Glyph` before styling a cell with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph {
    /// The selected block-element/quadrant/sextant character.
    pub ch: char,
    /// The color assigned to the "on" pixels (those set in the glyph's bit pattern).
    pub fg: Color,
    /// The color assigned to the "off" pixels (those clear in the glyph's bit pattern).
    pub bg: Color,
}

/// Squared euclidean distance between two RGB colors, as `u32` (no overflow risk for `u8`
/// channel differences).
const fn distance_sq(a: Rgb, b: Rgb) -> u32 {
    let dr = a.0.abs_diff(b.0) as u32;
    let dg = a.1.abs_diff(b.1) as u32;
    let db = a.2.abs_diff(b.2) as u32;
    dr * dr + dg * dg + db * db
}

/// Averages the `pixels` selected by `mask` (bit `i` set means `pixels[i]` is included), rounding
/// each channel to the nearest integer. Returns `None` if `mask` selects no pixels.
fn average(pixels: &[Rgb], mask: usize) -> Option<Rgb> {
    let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
    for (i, &(pr, pg, pb)) in pixels.iter().enumerate() {
        if mask & (1 << i) != 0 {
            r += u32::from(pr);
            g += u32::from(pg);
            b += u32::from(pb);
            n += 1;
        }
    }
    if n == 0 {
        return None;
    }
    let round = |sum: u32| u8::try_from((sum + n / 2) / n).unwrap_or(u8::MAX);
    Some((round(r), round(g), round(b)))
}

/// Posterizes `pixels` to the glyph (from `table`, indexed by row-major bit pattern) and two
/// representative colors that minimize total squared color error, by exhaustive search over
/// every `2^N` way to split the block into a foreground and background set.
///
/// `table.len()` must be `2^pixels.len()`; every caller in this module upholds that by
/// construction, so this stays a plain slice rather than a const-generic-sized array (which
/// would need unstable `generic_const_exprs` to relate `N` to `table`'s length at the type
/// level).
fn posterize(pixels: &[Rgb], table: &[char]) -> Glyph {
    let full_mask = table.len() - 1;
    let overall = average(pixels, full_mask).unwrap_or((0, 0, 0));

    let mut best_pattern = 0usize;
    let mut best_error = u32::MAX;
    for mask in 0..table.len() {
        let fg = average(pixels, mask).unwrap_or(overall);
        let bg = average(pixels, !mask & full_mask).unwrap_or(overall);
        let mut error = 0u32;
        for (i, &pixel) in pixels.iter().enumerate() {
            let assigned = if mask & (1 << i) != 0 { fg } else { bg };
            error += distance_sq(pixel, assigned);
        }
        if error < best_error {
            best_error = error;
            best_pattern = mask;
        }
    }

    let fg = average(pixels, best_pattern).unwrap_or(overall);
    let bg = average(pixels, !best_pattern & full_mask).unwrap_or(overall);
    Glyph {
        ch: table[best_pattern],
        fg: Color::Rgb {
            r: fg.0,
            g: fg.1,
            b: fg.2,
        },
        bg: Color::Rgb {
            r: bg.0,
            g: bg.1,
            b: bg.2,
        },
    }
}

/// Posterizes a 1-wide x 2-tall pixel block (`[top, bottom]`) to `' '`/`▀`/`▄`/`█` plus two
/// representative colors.
///
/// This is the lowest-fidelity, most compatible option: plain Unicode Block Elements, supported
/// by essentially every monospace terminal font.
///
/// # Example
///
/// ```
/// use retroglyph_core::subcell::quantize_half_block;
///
/// let glyph = quantize_half_block([(255, 255, 255), (0, 0, 0)]);
/// assert_eq!(glyph.ch, '▀'); // top half set, bottom clear
/// ```
#[must_use]
pub fn quantize_half_block(pixels: [Rgb; 2]) -> Glyph {
    posterize(&pixels, &HALF_BLOCKS)
}

/// Posterizes a 2x2 pixel block (`[top_left, top_right, bottom_left, bottom_right]`) to one of
/// the 16 quadrant block glyphs plus two representative colors.
///
/// Doubles both horizontal and vertical resolution over [`quantize_half_block`].
///
/// # Example
///
/// ```
/// use retroglyph_core::subcell::quantize_quadrant;
///
/// let black = (0, 0, 0);
/// let white = (255, 255, 255);
/// let glyph = quantize_quadrant([black, white, black, black]);
/// assert_eq!(glyph.ch, '▝'); // top-right quadrant
/// ```
#[must_use]
pub fn quantize_quadrant(pixels: [Rgb; 4]) -> Glyph {
    posterize(&pixels, &QUADRANTS)
}

/// Posterizes a 2-wide x 3-tall pixel block (`[top_left, top_right, mid_left, mid_right,
/// bottom_left, bottom_right]`) to one of the 64 sextant glyphs plus two representative colors.
///
/// The highest-fidelity option (doubles vertical resolution again over [`quantize_quadrant`]),
/// and the newest/least universally supported: sextant glyphs come from a 2022 Unicode addition
/// and need a font with "Symbols for Legacy Computing" coverage to render as blocks rather than
/// tofu/replacement characters.
///
/// # Example
///
/// ```
/// use retroglyph_core::subcell::quantize_sextant;
///
/// let black = (0, 0, 0);
/// let white = (255, 255, 255);
/// let glyph = quantize_sextant([white, black, black, black, black, black]);
/// assert_eq!(glyph.ch, '🬀'); // top-left sextant only
/// ```
#[must_use]
pub fn quantize_sextant(pixels: [Rgb; 6]) -> Glyph {
    posterize(&pixels, &SEXTANTS)
}

#[cfg(test)]
mod tests {
    use super::{
        Color, average, distance_sq, quantize_half_block, quantize_quadrant, quantize_sextant,
    };

    const BLACK: (u8, u8, u8) = (0, 0, 0);
    const WHITE: (u8, u8, u8) = (255, 255, 255);
    const RED: (u8, u8, u8) = (200, 0, 0);

    #[test]
    fn distance_sq_matches_manual_euclidean() {
        assert_eq!(distance_sq(BLACK, WHITE), 255 * 255 * 3);
        assert_eq!(distance_sq(BLACK, BLACK), 0);
    }

    #[test]
    fn average_rounds_to_nearest_and_handles_empty_mask() {
        assert_eq!(average(&[BLACK, WHITE], 0b11), Some((128, 128, 128)));
        assert_eq!(average(&[BLACK, WHITE], 0b01), Some(BLACK));
        assert_eq!(average(&[BLACK, WHITE], 0b00), None);
    }

    #[test]
    fn half_block_uniform_color_picks_space_with_that_color() {
        // A flat block ties across every pattern (all zero error); pattern 0 -- the cheapest
        // glyph, a space -- always wins that tie. `fg` still comes back populated so a caller
        // never has to special-case a uniform block before styling with it.
        let glyph = quantize_half_block([WHITE, WHITE]);
        assert_eq!(glyph.ch, ' ');
        assert_eq!(
            glyph.bg,
            Color::Rgb {
                r: 255,
                g: 255,
                b: 255
            }
        );
        assert_eq!(glyph.fg, glyph.bg);
    }

    #[test]
    fn half_block_top_bottom_split() {
        let glyph = quantize_half_block([WHITE, BLACK]);
        assert_eq!(glyph.ch, '▀');
        assert_eq!(
            glyph.fg,
            Color::Rgb {
                r: 255,
                g: 255,
                b: 255
            }
        );
        assert_eq!(glyph.bg, Color::Rgb { r: 0, g: 0, b: 0 });
    }

    #[test]
    fn quadrant_picks_bottom_left_glyph() {
        // Bit 2 (bottom-left) is the only set pixel.
        let glyph = quantize_quadrant([BLACK, BLACK, WHITE, BLACK]);
        assert_eq!(glyph.ch, '▖');
    }

    #[test]
    fn quadrant_uniform_color_picks_space_with_that_color() {
        let glyph = quantize_quadrant([RED, RED, RED, RED]);
        assert_eq!(glyph.ch, ' ');
        assert_eq!(glyph.bg, Color::Rgb { r: 200, g: 0, b: 0 });
        assert_eq!(glyph.fg, glyph.bg);
    }

    #[test]
    fn sextant_prefers_lower_pattern_on_exact_ties() {
        // A perfect flat block ties every pattern at zero error; pattern 0 (space) always wins.
        let grey = (128, 128, 128);
        let glyph = quantize_sextant([grey, grey, grey, grey, grey, grey]);
        assert_eq!(glyph.ch, ' ');
    }

    #[test]
    fn sextant_single_pixel_set() {
        let glyph = quantize_sextant([BLACK, BLACK, BLACK, BLACK, WHITE, BLACK]);
        assert_eq!(glyph.ch, '🬏'); // bottom-left sextant only (bit 4)
        assert_eq!(
            glyph.fg,
            Color::Rgb {
                r: 255,
                g: 255,
                b: 255
            }
        );
        assert_eq!(glyph.bg, Color::Rgb { r: 0, g: 0, b: 0 });
    }
}
