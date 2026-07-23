//! Fundamental unit of the grid: a single drawable tile.

use crate::style::Style;
#[cfg(feature = "egc")]
use alloc::string::String;
use unicode_width::UnicodeWidthChar;

/// Computes the display (column) width of a single glyph, capped to what fits in a `u8`
/// (`unicode_width` only ever returns 0, 1, or 2 for a single `char`, well within range).
/// Unassigned/control-character widths (`None`) are treated as 1, matching this crate's prior
/// per-cell fallback behavior.
fn glyph_width(glyph: char) -> u8 {
    #[allow(clippy::cast_possible_truncation)]
    let width = glyph.width().unwrap_or(1) as u8;
    width
}

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
        /// This tile has an entry in its layer's sparse EGC side-table
        /// (see `Grid`'s internal `LayerBuf::extras`), because it holds a
        /// multi-codepoint grapheme cluster (combining marks, ZWJ sequences).
        ///
        /// This flag is authoritative for whether extra text exists: code
        /// that reads a tile's grapheme must check this bit first and treat
        /// the side-table as backing storage only, never the other way
        /// around. `Tile` cannot carry the string itself and stay small (see
        /// [`Grid::grapheme`](crate::grid::Grid::grapheme)); the split is
        /// what keeps the common single-codepoint tile compact.
        const HAS_EXTRA         = 0b0000_1000;
    }
}

/// A single drawable tile in the terminal grid.
///
/// Each tile occupies one cell on a single layer; a [`Grid`](crate::grid::Grid)
/// holds up to 256 independent layers of tiles per cell, composited
/// bottom-to-top. Sub-cell pixel offsets (`dx`, `dy`) are visual only, they do
/// not affect grid logic or hit-testing. Backends that cannot represent pixel
/// offsets (e.g. `CrosstermBackend`) ignore them.
///
/// A tile does *not* carry its own multi-codepoint grapheme text (see
/// [`TileFlags::HAS_EXTRA`]): that lives in a sparse side-table on the owning
/// [`Grid`](crate::grid::Grid), keeping every `Tile` a small, fully `Copy`
/// value regardless of whether the `egc` feature is enabled. Read it back via
/// [`Grid::grapheme`](crate::grid::Grid::grapheme).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Tile {
    /// Primary codepoint. For ASCII and most Unicode this is the whole story.
    pub(crate) glyph: char,
    /// Style applied to this tile.
    pub(crate) style: Style,
    /// Display (column) width of `glyph`, precomputed at write time.
    ///
    /// Terminal-family renderers need this on every [`draw`](crate::backend::Output::draw) call
    /// to know how far the cursor advances after printing a cell; recomputing it with
    /// `unicode_width` on every cell of every frame is pure waste since a glyph's width never
    /// changes between frames. It is computed once, here, whenever the glyph is written (see
    /// [`with_glyph`](Self::with_glyph) and [`Grid::write_grapheme`](crate::grid::Grid::write_grapheme)),
    /// and just read back afterward. Almost always 0, 1, or 2 (control characters/combining
    /// marks are 0; a handful of grapheme clusters can report other values via
    /// `unicode_width`, but `u8` comfortably covers every value that crate returns).
    pub(crate) width: u8,
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
}

impl Default for Tile {
    fn default() -> Self {
        Self {
            glyph: ' ',
            style: Style::default(),
            width: 1,
            dx: 0,
            dy: 0,
            flags: TileFlags::EMPTY,
        }
    }
}

impl Tile {
    /// Creates a new tile with the given glyph and style.
    ///
    /// `dx` and `dy` default to 0 (no sub-cell offset). `glyph`'s display width is computed
    /// once here (see [`width`](Self::width)) rather than on every render.
    #[must_use]
    pub fn new(glyph: char, style: Style) -> Self {
        Self {
            glyph,
            style,
            width: glyph_width(glyph),
            dx: 0,
            dy: 0,
            flags: TileFlags::empty(),
        }
    }

    /// Returns the tile's glyph (primary codepoint).
    #[must_use]
    pub const fn glyph(&self) -> char {
        self.glyph
    }

    /// Returns the precomputed display (column) width of [`glyph`](Self::glyph).
    ///
    /// Computed once when the glyph is written (see [`with_glyph`](Self::with_glyph) and
    /// [`Grid::write_grapheme`](crate::grid::Grid::write_grapheme)), not recomputed on every
    /// render. For tiles written via `write_grapheme`, this reflects the full grapheme cluster's
    /// width, not just the primary codepoint's.
    #[must_use]
    pub const fn width(&self) -> u16 {
        self.width as u16
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

    /// Sets the glyph for this tile (builder style).
    ///
    /// Writing content marks the tile non-empty (see [`is_empty`](Self::is_empty)). Recomputes
    /// the cached display width (see [`width`](Self::width)) for the new glyph.
    #[must_use]
    pub fn with_glyph(mut self, glyph: char) -> Self {
        self.glyph = glyph;
        self.width = glyph_width(glyph);
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
    ///
    /// Does not touch the owning [`Grid`]'s EGC side-table; callers that
    /// reset a tile which may have carried [`TileFlags::HAS_EXTRA`] are
    /// responsible for also clearing that entry (see `Grid::clear_overlap`).
    #[cfg(feature = "egc")]
    pub(crate) fn reset(&mut self) {
        self.glyph = ' ';
        self.style = Style::default();
        self.width = 1;
        self.dx = 0;
        self.dy = 0;
        self.flags = TileFlags::EMPTY;
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

    /// Regression guard for the size win the EGC side-table exists for: a
    /// `Tile` must stay small and feature-stable (same layout with or
    /// without `egc`) now that it no longer inlines grapheme text.
    #[test]
    fn test_tile_size_is_stable_and_small() {
        assert_eq!(size_of::<Tile>(), 20);
    }

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
    fn test_tile_wide_flag() {
        let mut tile = Tile::new('漢', Style::default());
        tile.flags = TileFlags::WIDE_CHAR;
        assert!(tile.flags().contains(TileFlags::WIDE_CHAR));
        assert!(!tile.flags().contains(TileFlags::WIDE_CHAR_SPACER));
    }

    #[test]
    fn test_tile_width_is_precomputed_from_glyph() {
        // ASCII is single-column; a CJK ideograph is double-column. Both are computed once at
        // write time (`new`/`with_glyph`), not left for callers to recompute per render.
        assert_eq!(Tile::new('A', Style::default()).width(), 1);
        assert_eq!(Tile::new('漢', Style::default()).width(), 2);
        assert_eq!(Tile::default().width(), 1);
    }

    #[test]
    fn test_tile_with_glyph_recomputes_width() {
        let tile = Tile::new('A', Style::default()).with_glyph('漢');
        assert_eq!(tile.glyph(), '漢');
        assert_eq!(tile.width(), 2);
    }
}
