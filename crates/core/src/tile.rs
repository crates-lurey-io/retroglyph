//! Fundamental unit of the grid: a single drawable tile.

// TODO: `size_of::<Tile>()` is 32 bytes with `egc` (align 8, forced by the inline
// `Option<Arc<String>>`), 20 bytes without -- `flags`/`extra`/`dx`/`dy` are kept
// inline and unconditional (rather than moved to a side-table) so `Tile`'s public
// shape stays feature-stable, which is what makes the core/backend crate split
// clean. A side-table (sparse per-layer map, keeping only a `flags` bit on `Tile`)
// was considered and rejected: the draw path hands backends a `&Tile` via
// `Backend::draw_layers`, and crossterm/software both read the full grapheme
// (`cell.extra`) at draw time, so a side-table would force widening the draw item
// to `(layer, Pos, &Tile, Option<&str>)` across the `Backend` trait and every call
// site and test. That's a bigger trait change than a size optimization justifies on
// its own; revisit only alongside a `Backend`-trait change that's already on the
// table for another reason, not as a standalone shrink.

use crate::style::Style;
use alloc::string::String;
use alloc::sync::Arc;

bitflags::bitflags! {
    /// Bit-flags tracking wide-character tile roles.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
    pub struct TileFlags: u8 {
        /// This tile is the left half of a 2-column wide character.
        const WIDE_CHAR        = 0b0000_0001;
        /// This tile is the invisible right-half spacer of a wide character.
        const WIDE_CHAR_SPACER = 0b0000_0010;
        /// No content has been written to this tile: it is fully transparent.
        ///
        /// Set on [`Tile::default`] and cleared by every write. Compositing
        /// ([`Grid::blit`](crate::grid::Grid::blit), layer flattening) skips
        /// empty tiles, so an *explicit* space (which is not empty) is opaque
        /// and overwrites lower layers, while an untouched cell is not.
        const EMPTY            = 0b0000_0100;
    }
}

/// A single drawable tile in the terminal grid.
///
/// Each tile occupies one cell on a single layer; a [`Grid`](crate::grid::Grid)
/// holds up to 256 independent layers of tiles per cell, composited
/// bottom-to-top. Sub-cell pixel offsets (`dx`, `dy`) are visual only, they do
/// not affect grid logic or hit-testing. Backends that cannot represent pixel
/// offsets (e.g. `CrosstermBackend`) ignore them.
// The manual `PartialEq` below only adds an `Arc::ptr_eq` fast path in front of
// the same field-by-field comparison the derive would generate, so it stays
// consistent with the derived `Hash` (equal tiles have equal grapheme content).
#[allow(clippy::derived_hash_with_manual_eq)]
#[derive(Clone, Debug, Eq, Hash)]
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
    /// Always present so `Tile`'s layout is stable whether or not the `egc`
    /// feature is enabled. Without `egc` it is never set to anything but empty.
    pub(crate) flags: TileFlags,
    /// Allocated only when the grapheme cluster has more than one codepoint
    /// (combining marks, ZWJ emoji sequences, etc.).
    ///
    /// When `Some`, the full EGC string is stored here. The `glyph` field
    /// still holds the first codepoint for fast single-char paths.
    ///
    /// Always present for a stable layout across `egc`; without `egc` it is
    /// always `None` (nothing writes it, so it never allocates).
    pub(crate) extra: Option<Arc<String>>,
}

impl PartialEq for Tile {
    fn eq(&self, other: &Self) -> bool {
        self.glyph == other.glyph
            && self.style == other.style
            && self.dx == other.dx
            && self.dy == other.dy
            && self.flags == other.flags
            && match (&self.extra, &other.extra) {
                // Fast path: the common single-codepoint case, and shared Arcs
                // (e.g. cloned tiles) settle without touching the heap string.
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b) || a == b,
                _ => false,
            }
    }
}

impl Default for Tile {
    fn default() -> Self {
        Self {
            glyph: ' ',
            style: Style::default(),
            dx: 0,
            dy: 0,
            flags: TileFlags::EMPTY,
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
            flags: TileFlags::empty(),
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
    #[must_use]
    pub const fn flags(&self) -> TileFlags {
        self.flags
    }

    /// Returns `true` if nothing has been written to this tile.
    ///
    /// Empty tiles are transparent when compositing layers. An explicit
    /// space (e.g. `Tile::new(' ', style)`) is **not** empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.flags.contains(TileFlags::EMPTY)
    }

    /// Returns the extra EGC data for this tile, if any.
    ///
    /// `Some` only for multi-codepoint grapheme clusters (combining marks,
    /// ZWJ sequences, etc.). `None` for the common single-codepoint case, and
    /// always `None` without the `egc` feature.
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
    /// Always `None` without the `egc` feature.
    #[must_use]
    pub fn grapheme(&self) -> Option<&str> {
        self.extra.as_deref().map(String::as_str)
    }

    /// Sets the glyph for this tile (builder style).
    ///
    /// Writing content marks the tile non-empty (see [`is_empty`](Self::is_empty)).
    #[must_use]
    pub const fn with_glyph(mut self, glyph: char) -> Self {
        self.glyph = glyph;
        self.flags = self.flags.difference(TileFlags::EMPTY);
        self
    }

    /// Sets the style for this tile (builder style).
    ///
    /// Writing content marks the tile non-empty (see [`is_empty`](Self::is_empty)).
    #[must_use]
    pub const fn with_style(mut self, style: Style) -> Self {
        self.style = style;
        self.flags = self.flags.difference(TileFlags::EMPTY);
        self
    }

    /// Sets the sub-cell pixel offset for this tile (builder style).
    ///
    /// Writing content marks the tile non-empty (see [`is_empty`](Self::is_empty)).
    #[must_use]
    pub const fn with_offset(mut self, dx: i16, dy: i16) -> Self {
        self.dx = dx;
        self.dy = dy;
        self.flags = self.flags.difference(TileFlags::EMPTY);
        self
    }

    /// Resets this tile to the default (empty, space, default style, no offset).
    #[cfg(feature = "egc")]
    pub(crate) fn reset(&mut self) {
        self.glyph = ' ';
        self.style = Style::default();
        self.dx = 0;
        self.dy = 0;
        self.flags = TileFlags::EMPTY;
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
        // The default tile is empty (transparent when composited).
        assert!(tile.is_empty());
        assert_eq!(tile.flags(), TileFlags::EMPTY);
        #[cfg(feature = "egc")]
        assert!(tile.extra().is_none());
    }

    #[test]
    fn test_tile_empty_semantics() {
        // An explicit space is not empty; a default tile is.
        assert!(Tile::default().is_empty());
        assert!(!Tile::new(' ', Style::default()).is_empty());
        assert!(!Tile::default().with_glyph(' ').is_empty());
        assert!(!Tile::default().with_style(Style::default()).is_empty());
        assert!(!Tile::default().with_offset(1, 1).is_empty());
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
        assert!(!tile.is_empty());
        tile.reset();
        assert_eq!(tile.glyph(), ' ');
        assert_eq!(tile.style(), Style::default());
        assert_eq!(tile.dx, 0);
        assert_eq!(tile.dy, 0);
        assert!(tile.is_empty());
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
    fn test_tile_eq_arc_fast_path_and_value_fallback() {
        let tile_with = |s: &str| Tile {
            glyph: 'e',
            style: Style::default(),
            dx: 0,
            dy: 0,
            flags: TileFlags::empty(),
            extra: Some(Arc::new(String::from(s))),
        };

        // Cloned tiles share the same Arc: equal via the pointer fast path.
        let a = tile_with("e\u{0301}");
        let cloned = a.clone();
        assert!(Arc::ptr_eq(
            a.extra.as_ref().unwrap(),
            cloned.extra.as_ref().unwrap()
        ));
        assert_eq!(a, cloned);

        // Distinct Arcs with equal contents fall back to a value compare.
        let b = tile_with("e\u{0301}");
        assert!(!Arc::ptr_eq(
            a.extra.as_ref().unwrap(),
            b.extra.as_ref().unwrap()
        ));
        assert_eq!(a, b);

        // Different contents are unequal.
        assert_ne!(a, tile_with("a\u{0301}"));

        // None vs Some are unequal.
        assert_ne!(a, Tile::new('e', Style::default()));
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
