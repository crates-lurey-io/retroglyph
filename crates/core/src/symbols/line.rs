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
