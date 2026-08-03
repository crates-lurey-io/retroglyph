//! Pure `char` data for drawing borders, gridlines, and partial-block glyphs.
//!
//! Every set here is plain `const` data, no allocation and no rendering logic, so a widget crate,
//! a backend, or a caller drawing its own frame can all reach the same glyphs instead of retyping
//! Unicode box-drawing literals. [`crate::symbols::border`] and [`crate::symbols::line`] cover
//! whole-cell frame/gridline drawing; [`crate::symbols::block`] and [`crate::symbols::bar`] cover
//! the partial-block glyphs used for horizontal and vertical fill ramps (progress bars, gauges,
//! sparklines); [`crate::symbols::braille`] covers the 2x4-dot glyphs used for higher-resolution
//! point/line plotting than a single block cell allows.

/// The six glyphs that make up a single-style box border.
///
/// Field names match position, not weight: [`border::PLAIN`], [`border::ROUNDED`],
/// [`border::DOUBLE`], and [`border::THICK`] are all the same shape, drawn with different line
/// weights and corner styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BorderSet {
    /// Top-left corner.
    pub top_left: char,
    /// Top-right corner.
    pub top_right: char,
    /// Bottom-left corner.
    pub bottom_left: char,
    /// Bottom-right corner.
    pub bottom_right: char,
    /// Horizontal edge (top and bottom).
    pub horizontal: char,
    /// Vertical edge (left and right).
    pub vertical: char,
}

/// The seven glyphs needed to draw a gridline that can cross, tee, or run straight through a
/// cell, as used by table/grid dividers rather than an outer [`BorderSet`] frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LineSet {
    /// A straight horizontal run.
    pub horizontal: char,
    /// A straight vertical run.
    pub vertical: char,
    /// A four-way intersection (`┼`).
    pub cross: char,
    /// A T pointing left, joining a vertical run on its right (`┤`).
    pub vertical_left: char,
    /// A T pointing right, joining a vertical run on its left (`├`).
    pub vertical_right: char,
    /// A T pointing down, joining a horizontal run above (`┬`).
    pub horizontal_down: char,
    /// A T pointing up, joining a horizontal run below (`┴`).
    pub horizontal_up: char,
}

/// Whole-cell box-border glyph sets: [`PLAIN`](border::PLAIN), [`ROUNDED`](border::ROUNDED),
/// [`DOUBLE`](border::DOUBLE), and [`THICK`](border::THICK).
pub mod border {
    use super::BorderSet;

    /// Single-line box-drawing characters (`┌─┐│└┘`).
    pub const PLAIN: BorderSet = BorderSet {
        top_left: '┌',
        top_right: '┐',
        bottom_left: '└',
        bottom_right: '┘',
        horizontal: '─',
        vertical: '│',
    };

    /// [`PLAIN`] with rounded corners (`╭─╮│╰╯`).
    ///
    /// The 4 corners (`╭╮╰╯`) have no glyph in any font this crate bundles; drawing with
    /// `ROUNDED` through `retroglyph_window`'s fullest bundled `FontChain` falls back
    /// to the notdef substitute for those 4 entries (`horizontal`/`vertical` are shared with
    /// [`PLAIN`], which CP437 does cover). Supply a font with real corner glyphs to draw this
    /// set as intended.
    pub const ROUNDED: BorderSet = BorderSet {
        top_left: '╭',
        top_right: '╮',
        bottom_left: '╰',
        bottom_right: '╯',
        horizontal: '─',
        vertical: '│',
    };

    /// Double-line box-drawing characters (`╔═╗║╚╝`).
    pub const DOUBLE: BorderSet = BorderSet {
        top_left: '╔',
        top_right: '╗',
        bottom_left: '╚',
        bottom_right: '╝',
        horizontal: '═',
        vertical: '║',
    };

    /// Heavy (thick) single-line box-drawing characters (`┏━┓┃┗┛`).
    ///
    /// No glyph in any font this crate bundles covers any of these 6 entries; drawing with
    /// `THICK` through `retroglyph_window`'s fullest bundled `FontChain` falls back to
    /// the notdef substitute for the whole set. Supply a font with real heavy-line glyphs to
    /// draw this set as intended.
    pub const THICK: BorderSet = BorderSet {
        top_left: '┏',
        top_right: '┓',
        bottom_left: '┗',
        bottom_right: '┛',
        horizontal: '━',
        vertical: '┃',
    };
}

/// Gridline glyph sets, for drawing dividers that cross or tee into each other rather than an
/// outer frame: [`NORMAL`](line::NORMAL), [`DOUBLE`](line::DOUBLE), [`THICK`](line::THICK).
pub mod line {
    use super::LineSet;

    /// Single-line gridline characters (`─│┼┤├┬┴`).
    pub const NORMAL: LineSet = LineSet {
        horizontal: '─',
        vertical: '│',
        cross: '┼',
        vertical_left: '┤',
        vertical_right: '├',
        horizontal_down: '┬',
        horizontal_up: '┴',
    };

    /// Double-line gridline characters (`═║╬╣╠╦╩`).
    pub const DOUBLE: LineSet = LineSet {
        horizontal: '═',
        vertical: '║',
        cross: '╬',
        vertical_left: '╣',
        vertical_right: '╠',
        horizontal_down: '╦',
        horizontal_up: '╩',
    };

    /// Heavy (thick) gridline characters (`━┃╋┫┣┳┻`).
    ///
    /// `horizontal`/`vertical` are the same glyphs as [`super::border::THICK`]'s and share its notdef
    /// gap. The 4 tees and the cross (`┫┣┳┻╋`) have no glyph in any font this crate bundles
    /// either; drawing with `THICK` through `retroglyph_window`'s fullest bundled
    /// `FontChain` falls back to the notdef substitute for all 7 entries. Supply a font
    /// with real heavy-line glyphs to draw this set as intended.
    pub const THICK: LineSet = LineSet {
        horizontal: '━',
        vertical: '┃',
        cross: '╋',
        vertical_left: '┫',
        vertical_right: '┣',
        horizontal_down: '┳',
        horizontal_up: '┻',
    };
}

/// Horizontal, left-anchored eighth-block glyphs (`█▉▊▋▌▍▎▏`).
///
/// For filling a cell from the left edge by fractions of a column: a horizontal progress bar that
/// wants sub-cell resolution rather than rounding its fill to whole columns. For the vertical,
/// bottom-anchored equivalent used by [`bar`], see that module instead.
pub mod block {
    /// A fully filled cell.
    pub const FULL: char = '█';
    /// Filled 7/8 from the left.
    pub const SEVEN_EIGHTHS: char = '▉';
    /// Filled 3/4 from the left.
    pub const THREE_QUARTERS: char = '▊';
    /// Filled 5/8 from the left.
    pub const FIVE_EIGHTHS: char = '▋';
    /// Filled 1/2 from the left.
    pub const HALF: char = '▌';
    /// Filled 3/8 from the left.
    pub const THREE_EIGHTHS: char = '▍';
    /// Filled 1/4 from the left.
    pub const ONE_QUARTER: char = '▎';
    /// Filled 1/8 from the left.
    pub const ONE_EIGHTH: char = '▏';
}

/// Vertical, bottom-anchored eighth-block glyphs (`▁▂▃▄▅▆▇█`), for a bar that fills a cell from
/// the bottom edge: the ramp a sparkline, gauge, or meter widget uses for one column's worth of
/// magnitude.
pub mod bar {
    /// Filled 1/8 from the bottom.
    pub const ONE_EIGHTH: char = '▁';
    /// Filled 1/4 from the bottom.
    pub const ONE_QUARTER: char = '▂';
    /// Filled 3/8 from the bottom.
    pub const THREE_EIGHTHS: char = '▃';
    /// Filled 1/2 from the bottom.
    pub const HALF: char = '▄';
    /// Filled 5/8 from the bottom.
    pub const FIVE_EIGHTHS: char = '▅';
    /// Filled 3/4 from the bottom.
    pub const THREE_QUARTERS: char = '▆';
    /// Filled 7/8 from the bottom.
    pub const SEVEN_EIGHTHS: char = '▇';
    /// A fully filled cell.
    pub const FULL: char = '█';

    /// The nine bar levels from empty to full, indexed `0..=8`: a blank cell, then
    /// [`ONE_EIGHTH`] through [`FULL`] in order.
    ///
    /// Indexing this directly by `round(fraction * 8.0) as usize` turns a `0.0..=1.0` magnitude
    /// into the right glyph for one bar column.
    pub const NINE_LEVELS: [char; 9] = [
        ' ',
        ONE_EIGHTH,
        ONE_QUARTER,
        THREE_EIGHTHS,
        HALF,
        FIVE_EIGHTHS,
        THREE_QUARTERS,
        SEVEN_EIGHTHS,
        FULL,
    ];
}

/// Unicode Braille Patterns (`U+2800..=U+28FF`), addressed as a 2-column by 4-row dot grid.
///
/// Braille cells pack eight independently-settable dots into one glyph, giving roughly 2x the
/// horizontal and 4x the vertical resolution of a plain block glyph for plotting points or thin
/// lines. Each dot has a fixed bit in the 8-bit pattern index; combine the ones you want set with
/// bitwise OR and pass the result to [`glyph()`](crate::symbols::braille::glyph).
///
/// ```text
/// (0,0) DOT_1  (1,0) DOT_4
/// (0,1) DOT_2  (1,1) DOT_5
/// (0,2) DOT_3  (1,2) DOT_6
/// (0,3) DOT_7  (1,3) DOT_8
/// ```
pub mod braille {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn border_sets_share_field_shape() {
        // Every set is a distinct glyph, not an alias of another weight.
        assert_ne!(border::PLAIN, border::ROUNDED);
        assert_ne!(border::PLAIN.horizontal, border::DOUBLE.horizontal);
        assert_eq!(border::PLAIN.horizontal, border::ROUNDED.horizontal);
    }

    #[test]
    fn line_sets_are_internally_consistent_weights() {
        assert_eq!(line::NORMAL.horizontal, border::PLAIN.horizontal);
        assert_eq!(line::DOUBLE.horizontal, border::DOUBLE.horizontal);
        assert_eq!(line::THICK.horizontal, border::THICK.horizontal);
    }

    #[test]
    fn bar_nine_levels_matches_named_constants() {
        assert_eq!(bar::NINE_LEVELS[0], ' ');
        assert_eq!(bar::NINE_LEVELS[1], bar::ONE_EIGHTH);
        assert_eq!(bar::NINE_LEVELS[8], bar::FULL);
    }
}
