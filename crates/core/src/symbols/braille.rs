/// The empty braille cell (no dots set), `⠀` (U+2800, distinct from a plain space).
pub const BLANK: char = '\u{2800}';

/// Dot at column 0, row 0.
pub const DOT_1: u8 = 0x01;
/// Dot at column 0, row 1.
pub const DOT_2: u8 = 0x02;
/// Dot at column 0, row 2.
pub const DOT_3: u8 = 0x04;
/// Dot at column 1, row 0.
pub const DOT_4: u8 = 0x08;
/// Dot at column 1, row 1.
pub const DOT_5: u8 = 0x10;
/// Dot at column 1, row 2.
pub const DOT_6: u8 = 0x20;
/// Dot at column 0, row 3.
pub const DOT_7: u8 = 0x40;
/// Dot at column 1, row 3.
pub const DOT_8: u8 = 0x80;

/// The 2x4 dot-position table, indexed `[row][col]`, giving the bit for each cell in the
/// braille dot grid.
pub const DOTS: [[u8; 2]; 4] = [
    [DOT_1, DOT_4],
    [DOT_2, DOT_5],
    [DOT_3, DOT_6],
    [DOT_7, DOT_8],
];

/// The glyph for `pattern`, a bitmask of the eight `DOT_*` constants (or values from
/// [`DOTS`]) OR'd together.
///
/// Every value of `pattern` maps to a valid glyph: `U+2800..=U+28FF` contains no surrogate
/// code points, so this never falls back to a placeholder.
#[must_use]
pub const fn glyph(pattern: u8) -> char {
    // `0x2800..=0x28FF` is entirely outside the surrogate range (`0xD800..=0xDFFF`), so this
    // is always a valid `char`; `unwrap_or` sidesteps `Option::expect` not being `const fn`
    // yet on our MSRV without claiming a real fallback exists.
    // `u32::from` isn't a stable `const fn` at this MSRV, so `pattern` (already `u8`) is
    // widened with `as` instead; this is a lossless, non-truncating widening, not the lossy
    // narrowing cast clippy's `as_conversions`/`cast_possible_truncation` warn about.
    #[allow(clippy::as_conversions)]
    match char::from_u32(0x2800 + pattern as u32) {
        Some(c) => c,
        None => BLANK,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_zero_is_blank() {
        assert_eq!(glyph(0), BLANK);
    }

    #[test]
    fn glyph_covers_the_full_byte_range() {
        for pattern in 0..=u8::MAX {
            let c = glyph(pattern);
            assert_eq!(u32::from(c), 0x2800 + u32::from(pattern));
        }
    }

    #[test]
    fn dots_table_has_eight_distinct_bits() {
        let mut seen = 0u8;
        for row in DOTS {
            for bit in row {
                assert_eq!(seen & bit, 0, "bit {bit:#04x} reused");
                seen |= bit;
            }
        }
        assert_eq!(seen, 0xFF);
    }
}
