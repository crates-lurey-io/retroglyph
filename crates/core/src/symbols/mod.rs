//! `char` data for drawing borders, gridlines, and partial-block glyphs, plus the pixel-to-glyph
//! matching logic that picks one of those glyphs for a raw pixel block.
//!
//! Every set here is plain `const` data with no rendering logic of its own, so a widget crate, a
//! backend, or a caller drawing its own frame can all reach the same glyphs instead of retyping
//! Unicode box-drawing literals. [`crate::symbols::border`] and [`crate::symbols::line`] cover
//! whole-cell frame/gridline drawing; [`crate::symbols::block`] and [`crate::symbols::bar`] cover
//! the partial-block glyphs used for horizontal and vertical fill ramps (progress bars, gauges,
//! sparklines); [`crate::symbols::braille`] covers the 2x4-dot glyphs used for higher-resolution
//! point/line plotting than a single block cell allows. [`quantize_half_block`],
//! [`quantize_quadrant`], and [`quantize_sextant`] are the one exception: they posterize a block
//! of raw pixels down to the best-matching glyph from [`HALF_BLOCKS`](crate::symbols::HALF_BLOCKS)/
//! [`QUADRANTS`](crate::symbols::QUADRANTS)/[`SEXTANTS`](crate::symbols::SEXTANTS), the actual
//! matching algorithm alongside the data it searches over.

/// Vertical, bottom-anchored eighth-block glyphs (`▁▂▃▄▅▆▇█`), for a bar that fills a cell from
/// the bottom edge: the ramp a sparkline, gauge, or meter widget uses for one column's worth of
/// magnitude.
pub mod bar;
/// Horizontal, left-anchored eighth-block glyphs (`█▉▊▋▌▍▎▏`).
///
/// For filling a cell from the left edge by fractions of a column: a horizontal progress bar that
/// wants sub-cell resolution rather than rounding its fill to whole columns. For the vertical,
/// bottom-anchored equivalent used by [`bar`], see that module instead.
pub mod block;
/// Whole-cell box-border glyph sets: [`PLAIN`](border::PLAIN), [`ROUNDED`](border::ROUNDED),
/// [`DOUBLE`](border::DOUBLE), and [`THICK`](border::THICK).
pub mod border;
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
pub mod braille;
/// Gridline glyph sets, for drawing dividers that cross or tee into each other rather than an
/// outer frame: [`NORMAL`](line::NORMAL), [`DOUBLE`](line::DOUBLE), [`THICK`](line::THICK).
pub mod line;
mod subcell;

pub use subcell::{
    Glyph, HALF_BLOCKS, Pixel, QUADRANTS, SEXTANTS, quantize_half_block, quantize_quadrant,
    quantize_sextant,
};

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
