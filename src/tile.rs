//! Fundamental unit of the grid: a single drawable tile.

// TODO: measure `sizeof::<Tile>()` and consider struct-of-arrays or
// splitting `dx`/`dy` out of layer 0 if > 32 bytes.

use crate::style::Style;
#[cfg(feature = "egc")]
use alloc::sync::Arc;

bitflags::bitflags! {
    /// Bit-flags tracking wide-character tile roles.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
    pub struct TileFlags: u8 {
        /// This tile is the left half of a 2-column wide character.
        const WIDE_CHAR        = 0b0000_0001;
        /// This tile is the invisible right-half spacer of a wide character.
        const WIDE_CHAR_SPACER = 0b0000_0010;
    }
}

/// A single drawable tile in the terminal grid.
///
/// Each tile occupies one cell on a single layer (see ADR 008 for the layer
/// model). Sub-cell pixel offsets (`dx`, `dy`) are visual only — they do not
/// affect grid logic or hit-testing. Backends that cannot represent pixel
/// offsets (e.g. `CrosstermBackend`) ignore them.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Tile {
    /// Primary codepoint. For ASCII and most Unicode this is the whole story.
    pub(crate) glyph: char,
    /// Style applied to this tile.
    pub(crate) style: Style,
    /// Pixel offset from the cell's left edge. Negative shifts left.
    ///
    /// Only meaningful for graphical backends (e.g. `SoftwareBackend`).
    pub(crate) dx: i16,
    /// Pixel offset from the cell's top edge. Negative shifts up.
    ///
    /// Only meaningful for graphical backends (e.g. `SoftwareBackend`).
    pub(crate) dy: i16,
    /// Wide-character role flags (e.g. [`TileFlags::WIDE_CHAR`]).
    ///
    /// Only present when the `egc` feature is enabled.
    #[cfg(feature = "egc")]
    pub(crate) flags: TileFlags,
    /// Allocated only when the grapheme cluster has more than one codepoint
    /// (combining marks, ZWJ emoji sequences, etc.).
    ///
    /// When `Some`, the full EGC string is stored here. The `glyph` field
    /// still holds the first codepoint for fast single-char paths.
    ///
    /// Only present when the `egc` feature is enabled.
    #[cfg(feature = "egc")]
    pub(crate) extra: Option<Arc<String>>,
}

impl Default for Tile {
    fn default() -> Self {
        Self {
            glyph: ' ',
            style: Style::default(),
            dx: 0,
            dy: 0,
            #[cfg(feature = "egc")]
            flags: TileFlags::empty(),
            #[cfg(feature = "egc")]
            extra: None,
        }
    }
}

impl Tile {
    /// Creates a new tile with the given glyph and style.
    ///
    /// `dx` and `dy` default to 0 (no sub-cell offset).
    #[must_use]
    pub const fn new(glyph: char, style: Style) -> Self {
        Self {
            glyph,
            style,
            dx: 0,
            dy: 0,
            #[cfg(feature = "egc")]
            flags: TileFlags::empty(),
            #[cfg(feature = "egc")]
            extra: None,
        }
    }

    /// Returns the tile's glyph (primary codepoint).
    #[must_use]
    pub const fn glyph(&self) -> char {
        self.glyph
    }

    /// Returns the tile's style.
    #[must_use]
    pub const fn style(&self) -> Style {
        self.style
    }

    /// Returns the sub-cell pixel X offset.
    #[must_use]
    pub const fn dx(&self) -> i16 {
        self.dx
    }

    /// Returns the sub-cell pixel Y offset.
    #[must_use]
    pub const fn dy(&self) -> i16 {
        self.dy
    }

    /// Returns the wide-character flags for this tile.
    ///
    /// Only present when the `egc` feature is enabled.
    #[cfg(feature = "egc")]
    #[must_use]
    pub const fn flags(&self) -> TileFlags {
        self.flags
    }

    /// Returns the extra EGC data for this tile, if any.
    ///
    /// `Some` only for multi-codepoint grapheme clusters (combining marks,
    /// ZWJ sequences, etc.). `None` for the common single-codepoint case.
    ///
    /// Only present when the `egc` feature is enabled.
    #[cfg(feature = "egc")]
    #[must_use]
    pub fn extra(&self) -> Option<&str> {
        self.extra.as_deref().map(String::as_str)
    }

    /// Returns the full grapheme cluster for this tile.
    ///
    /// When the tile contains a multi-codepoint EGC (combining marks, ZWJ
    /// sequences, etc.) this returns the stored string. For the common
    /// single-codepoint case it returns `None`; use [`glyph`](Self::glyph)
    /// and [`encode_utf8`](char::encode_utf8) to reconstruct the string.
    ///
    /// Only present when the `egc` feature is enabled.
    #[cfg(feature = "egc")]
    #[must_use]
    pub fn grapheme(&self) -> Option<&str> {
        self.extra.as_deref().map(String::as_str)
    }

    /// Sets the glyph for this tile (builder style).
    #[must_use]
    pub const fn with_glyph(mut self, glyph: char) -> Self {
        self.glyph = glyph;
        self
    }

    /// Sets the style for this tile (builder style).
    #[must_use]
    pub const fn with_style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the sub-cell pixel offset for this tile (builder style).
    #[must_use]
    pub const fn with_offset(mut self, dx: i16, dy: i16) -> Self {
        self.dx = dx;
        self.dy = dy;
        self
    }

    /// Resets this tile to the default (space, default style, no offset).
    #[cfg(feature = "egc")]
    pub(crate) fn reset(&mut self) {
        self.glyph = ' ';
        self.style = Style::default();
        self.dx = 0;
        self.dy = 0;
        self.flags = TileFlags::empty();
        self.extra = None;
    }
}

/// Returns `grapheme` truncated to at most 8 codepoints (combining-mark bomb
/// defence). If the input is already within the limit it is returned as-is.
///
/// Only present when the `egc` feature is enabled.
#[cfg(feature = "egc")]
pub(crate) fn cap_grapheme(grapheme: &str) -> String {
    const MAX_CODEPOINTS: usize = 8;
    // Most graphemes are already within the cap; avoid allocation when possible.
    if grapheme.chars().count() <= MAX_CODEPOINTS {
        return String::from(grapheme);
    }
    grapheme.chars().take(MAX_CODEPOINTS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;

    #[test]
    fn test_tile_defaults() {
        let tile = Tile::default();
        assert_eq!(tile.glyph(), ' ');
        assert_eq!(tile.style(), Style::default());
        assert_eq!(tile.dx, 0);
        assert_eq!(tile.dy, 0);
        #[cfg(feature = "egc")]
        {
            assert_eq!(tile.flags(), TileFlags::empty());
            assert!(tile.extra().is_none());
        }
    }

    #[test]
    fn test_tile_builder() {
        let style = Style::new().fg(Color::RED);
        let tile = Tile::new('A', style);
        assert_eq!(tile.glyph(), 'A');
        assert_eq!(tile.style(), style);

        let tile = tile.with_glyph('B');
        assert_eq!(tile.glyph(), 'B');
    }

    #[test]
    fn test_tile_with_offset() {
        let tile = Tile::new('X', Style::default()).with_offset(-3, 5);
        assert_eq!(tile.dx, -3);
        assert_eq!(tile.dy, 5);
    }

    #[test]
    fn test_tile_reset() {
        let style = Style::new().fg(Color::RED);
        let mut tile = Tile::new('X', style);
        tile.reset();
        assert_eq!(tile.glyph(), ' ');
        assert_eq!(tile.style(), Style::default());
        assert_eq!(tile.dx, 0);
        assert_eq!(tile.dy, 0);
    }

    #[cfg(feature = "egc")]
    #[test]
    fn test_tile_grapheme_single() {
        let tile = Tile::new('A', Style::default());
        assert_eq!(tile.grapheme(), None); // single-char, no extra
    }

    #[cfg(feature = "egc")]
    #[test]
    fn test_tile_grapheme_multi() {
        // Multi-codepoint EGC: e + combining acute
        let extra_str = Arc::new(String::from("e\u{0301}"));
        let tile = Tile {
            glyph: 'e',
            style: Style::default(),
            dx: 0,
            dy: 0,
            flags: TileFlags::empty(),
            extra: Some(extra_str),
        };
        assert_eq!(tile.grapheme(), Some("e\u{0301}"));
    }

    #[cfg(feature = "egc")]
    #[test]
    fn test_tile_wide_flag() {
        let mut tile = Tile::new('漢', Style::default());
        tile.flags = TileFlags::WIDE_CHAR;
        assert!(tile.flags().contains(TileFlags::WIDE_CHAR));
        assert!(!tile.flags().contains(TileFlags::WIDE_CHAR_SPACER));
    }
}
