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
