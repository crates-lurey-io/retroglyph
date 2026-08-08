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
/// into the right glyph for one bar column. Six of these eight are outside CP437 (only ' ' and
/// [`FULL`] are covered); a pixel backend resolving glyphs through a CP437 table draws the other
/// six as its fallback solid block, which erases the ramp's granularity. For a bar that has to
/// stay legible on a CP437-only tileset/sprite path, use [`SHADE_LEVELS`] instead.
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

/// A light shade: 25% coverage, CP437 code point 176 (`U+2591`).
pub const LIGHT_SHADE: char = '░';
/// A medium shade: 50% coverage, CP437 code point 177 (`U+2592`).
pub const MEDIUM_SHADE: char = '▒';
/// A dark shade: 75% coverage, CP437 code point 178 (`U+2593`).
pub const DARK_SHADE: char = '▓';

/// The five bar levels from empty to full, indexed `0..=4`: a blank cell, then [`LIGHT_SHADE`],
/// [`MEDIUM_SHADE`], [`DARK_SHADE`], and [`FULL`] in order.
///
/// Every glyph here is in CP437 (verified against `CP437_TO_UNICODE` in `retroglyph-window`),
/// unlike [`NINE_LEVELS`]: a caller drawing through a pixel backend that resolves glyphs via a
/// CP437 table, or a tileset/sprite backend that can't tint an arbitrary glyph by `Style::fg`,
/// can use this ramp and still get five distinguishable fill levels instead of collapsing to a
/// single fallback block. Index it by `round(fraction * 4.0) as usize` for one bar column.
pub const SHADE_LEVELS: [char; 5] = [' ', LIGHT_SHADE, MEDIUM_SHADE, DARK_SHADE, FULL];
