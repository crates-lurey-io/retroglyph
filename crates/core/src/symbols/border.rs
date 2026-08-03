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
