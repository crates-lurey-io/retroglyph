//! Fundamental unit of the grid.

use crate::style::Style;
#[cfg(feature = "egc")]
use alloc::sync::Arc;

bitflags::bitflags! {
    /// Bit-flags tracking wide-character cell roles.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
    pub struct CellFlags: u8 {
        /// This cell is the left half of a 2-column wide character.
        const WIDE_CHAR        = 0b0000_0001;
        /// This cell is the invisible right-half spacer of a wide character.
        const WIDE_CHAR_SPACER = 0b0000_0010;
    }
}

/// A single cell in the terminal grid.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Cell {
    /// Primary codepoint. For ASCII and most Unicode this is the whole story.
    pub(crate) glyph: char,
    /// Style applied to this cell.
    pub(crate) style: Style,
    /// Wide-character role flags (e.g. [`CellFlags::WIDE_CHAR`]).
    ///
    /// Only present when the `egc` feature is enabled.
    #[cfg(feature = "egc")]
    pub(crate) flags: CellFlags,
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

impl Default for Cell {
    fn default() -> Self {
        Self {
            glyph: ' ',
            style: Style::default(),
            #[cfg(feature = "egc")]
            flags: CellFlags::empty(),
            #[cfg(feature = "egc")]
            extra: None,
        }
    }
}

impl Cell {
    /// Creates a new cell with the given glyph and style.
    #[must_use]
    pub const fn new(glyph: char, style: Style) -> Self {
        Self {
            glyph,
            style,
            #[cfg(feature = "egc")]
            flags: CellFlags::empty(),
            #[cfg(feature = "egc")]
            extra: None,
        }
    }

    /// Returns the cell's glyph (primary codepoint).
    #[must_use]
    pub const fn glyph(&self) -> char {
        self.glyph
    }

    /// Returns the cell's style.
    #[must_use]
    pub const fn style(&self) -> Style {
        self.style
    }

    /// Returns the wide-character flags for this cell.
    ///
    /// Only present when the `egc` feature is enabled.
    #[cfg(feature = "egc")]
    #[must_use]
    pub const fn flags(&self) -> CellFlags {
        self.flags
    }

    /// Returns the extra EGC data for this cell, if any.
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

    /// Returns the full grapheme cluster for this cell.
    ///
    /// When the cell contains a multi-codepoint EGC (combining marks, ZWJ
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

    /// Sets the glyph for this cell (builder style).
    #[must_use]
    pub const fn with_glyph(mut self, glyph: char) -> Self {
        self.glyph = glyph;
        self
    }

    /// Sets the style for this cell (builder style).
    #[must_use]
    pub const fn with_style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Resets this cell to the default (space, default style).
    #[cfg(feature = "egc")]
    pub(crate) fn reset(&mut self) {
        self.glyph = ' ';
        self.style = Style::default();
        self.flags = CellFlags::empty();
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
    fn test_cell_defaults() {
        let cell = Cell::default();
        assert_eq!(cell.glyph(), ' ');
        assert_eq!(cell.style(), Style::default());
        #[cfg(feature = "egc")]
        {
            assert_eq!(cell.flags(), CellFlags::empty());
            assert!(cell.extra().is_none());
        }
    }

    #[test]
    fn test_cell_builder() {
        let style = Style::new().fg(Color::RED);
        let cell = Cell::new('A', style);
        assert_eq!(cell.glyph(), 'A');
        assert_eq!(cell.style(), style);

        let cell = cell.with_glyph('B');
        assert_eq!(cell.glyph(), 'B');
    }

    #[test]
    fn test_cell_reset() {
        let style = Style::new().fg(Color::RED);
        let mut cell = Cell::new('X', style);
        cell.reset();
        assert_eq!(cell.glyph(), ' ');
        assert_eq!(cell.style(), Style::default());
    }

    #[cfg(feature = "egc")]
    #[test]
    fn test_cell_grapheme_single() {
        let cell = Cell::new('A', Style::default());
        assert_eq!(cell.grapheme(), None); // single-char, no extra
    }

    #[cfg(feature = "egc")]
    #[test]
    fn test_cell_grapheme_multi() {
        // Multi-codepoint EGC: e + combining acute
        let extra_str = Arc::new(String::from("e\u{0301}"));
        let cell = Cell {
            glyph: 'e',
            style: Style::default(),
            flags: CellFlags::empty(),
            extra: Some(extra_str),
        };
        assert_eq!(cell.grapheme(), Some("e\u{0301}"));
    }

    #[cfg(feature = "egc")]
    #[test]
    fn test_cell_wide_flag() {
        let mut cell = Cell::new('漢', Style::default());
        cell.flags = CellFlags::WIDE_CHAR;
        assert!(cell.flags().contains(CellFlags::WIDE_CHAR));
        assert!(!cell.flags().contains(CellFlags::WIDE_CHAR_SPACER));
    }
}
