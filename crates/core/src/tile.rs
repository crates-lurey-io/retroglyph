//! Fundamental unit of the grid: a single drawable tile.

use crate::color::Style;
use crate::text::char_width;
#[cfg(feature = "egc")]
use alloc::string::String;

/// Computes the display (column) width of a single glyph, capped to what fits in a `u8`.
///
/// Delegates to [`char_width`], so a control character occupies the one column
/// [`Surface`](crate::surface::Surface) actually draws it in, and `Tile::width`'s value can never drift
/// from what that function documents and tests.
fn glyph_width(glyph: char) -> u8 {
    u8::try_from(char_width(glyph)).unwrap_or(1)
}

bitflags::bitflags! {
    /// Bit-flags tracking a tile's emptiness and its role in any multi-cell structure it is part
    /// of: a wide character, or a [span](crate::grid::Grid::write_span).
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
    pub struct TileFlags: u8 {
        /// This tile is the left half of a 2-column wide character.
        const WIDE_CHAR        = 0b0000_0001;
        /// This tile is the invisible right-half spacer of a wide character.
        const WIDE_CHAR_SPACER = 0b0000_0010;
        /// No content has been written to this tile: it is fully transparent.
        ///
        /// Set on [`Tile::default`](crate::tile::Tile::default) and cleared by every write. Compositing
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
        /// around. `Tile` cannot carry the string itself and stay small; the split is
        /// what keeps the common single-codepoint tile compact.
        const HAS_EXTRA         = 0b0000_1000;
        /// This tile is the top-left anchor of a multi-cell span: it occupies
        /// [`Tile::span`](crate::tile::Tile::span) cells, not one.
        ///
        /// Written only by [`Grid::write_span`](crate::grid::Grid::write_span), which also writes
        /// the matching [`SPAN_COVERED`](Self::SPAN_COVERED) tiles. An anchor without its covered
        /// cells is a broken invariant, which is why there is no `Tile` builder for this flag.
        const SPAN_ANCHOR       = 0b0001_0000;
        /// This tile is covered by a multi-cell span anchored above and/or to its left; see
        /// [`Tile::span_offset`](crate::tile::Tile::span_offset).
        ///
        /// Unlike [`WIDE_CHAR_SPACER`](Self::WIDE_CHAR_SPACER), a covered tile keeps a real glyph
        /// and **is** rendered by cell backends: that glyph is the span artwork's text fallback.
        /// Only a backend that actually draws the span's artwork (a pixel backend blitting one
        /// sprite across the whole footprint) skips it. See the [`grid`](crate::grid) module
        /// docs for the full contract.
        const SPAN_COVERED      = 0b0010_0000;
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
/// [`DrawCell::grapheme`](crate::backend::DrawCell::grapheme), streamed off
/// [`Grid::layers`](crate::grid::Grid::layers).
///
/// # Examples
///
/// ```
/// use retroglyph_core::color::{Color, Style};
/// use retroglyph_core::tile::Tile;
///
/// let tile = Tile::new('@', Style::new().fg(Color::GREEN));
/// assert_eq!(tile.glyph(), '@');
/// assert_eq!(tile.style().foreground(), Color::GREEN);
/// ```
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
    /// and just read back afterward. Almost always 0, 1, or 2 (combining marks are 0; control
    /// characters are 1, matching [`char_width`]; a handful of grapheme
    /// clusters can report other values via `unicode_width`, but `u8` comfortably covers every
    /// value that crate returns).
    pub(crate) width: u8,
    /// Pixel offset from the cell's left edge. Negative shifts left.
    ///
    /// Only meaningful for graphical backends (e.g. `SoftwareBackend`).
    pub(crate) dx: i16,
    /// Pixel offset from the cell's top edge. Negative shifts up.
    ///
    /// Only meaningful for graphical backends (e.g. `SoftwareBackend`).
    pub(crate) dy: i16,
    /// Role and occupancy flags: emptiness, wide-character halves, EGC side-table presence, and
    /// multi-cell span roles (see [`TileFlags`]).
    ///
    /// Always present so `Tile`'s layout is stable whether or not the `egc`
    /// feature is enabled. `WIDE_CHAR`/`WIDE_CHAR_SPACER` are set on every feature combination
    /// (both [`Grid::put_tile`](crate::grid::Grid::put_tile) and
    /// [`Grid::write_grapheme`](crate::grid::Grid::write_grapheme) set them); only the side-table
    /// presence bit ([`TileFlags::HAS_EXTRA`]) is `egc`-only, since it depends on grapheme
    /// clustering (`unicode-segmentation`) that this crate only pulls in under `egc`.
    pub(crate) flags: TileFlags,
    /// Multi-cell span bookkeeping, **overloaded by role** (see `flags`):
    ///
    /// | Flag | `span_w` | `span_h` |
    /// | --- | --- | --- |
    /// | [`TileFlags::SPAN_ANCHOR`] | footprint width in cells (>= 1) | footprint height (>= 1) |
    /// | [`TileFlags::SPAN_COVERED`] | `x - anchor.x` | `y - anchor.y` |
    /// | neither | 1 | 1 |
    ///
    /// The overload is what makes [`Grid::span_owner`](crate::grid::Grid::span_owner) O(1): a
    /// covered cell names its anchor directly instead of being found by scanning. Both bytes sit
    /// in `Tile`'s tail padding, so spans cost nothing (see `test_tile_size_is_stable_and_small`).
    /// Read them through [`span`](Self::span) and [`span_offset`](Self::span_offset), which
    /// enforce the roles, rather than touching the fields directly.
    pub(crate) span_w: u8,
    /// See [`span_w`](Self::span_w): the vertical half of the same overloaded pair.
    pub(crate) span_h: u8,
}

impl Default for Tile {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl Tile {
    /// The tile every layer cell starts as: a blank, unstyled, unwritten cell.
    ///
    /// Equivalent to [`Tile::default`], expressed as an associated `const` so callers that need
    /// a `'static` reference to a default tile (e.g. [`Grid::diff`](crate::grid::Grid::diff)
    /// reporting a layer that stopped being written) don't need an owned value to borrow from.
    pub(crate) const EMPTY: Self = Self {
        glyph: ' ',
        style: Style {
            fg: crate::color::Color::Default,
            bg: crate::color::Color::Default,
        },
        width: 1,
        dx: 0,
        dy: 0,
        flags: TileFlags::EMPTY,
        span_w: 1,
        span_h: 1,
    };

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
            span_w: 1,
            span_h: 1,
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

    /// Returns the role and occupancy flags for this tile: emptiness, wide-character halves,
    /// EGC side-table presence, and multi-cell span roles (see [`TileFlags`]).
    #[must_use]
    pub const fn flags(&self) -> TileFlags {
        self.flags
    }

    /// Returns how many cells this tile occupies, `(width, height)`.
    ///
    /// `(1, 1)` for every tile except a [`TileFlags::SPAN_ANCHOR`], which reports the footprint
    /// declared by [`Grid::write_span`](crate::grid::Grid::write_span). A covered cell reports
    /// `(1, 1)`: it does not own a footprint, it is inside one (see
    /// [`span_offset`](Self::span_offset)).
    #[must_use]
    pub const fn span(&self) -> (u16, u16) {
        if self.flags.contains(TileFlags::SPAN_ANCHOR) {
            (self.span_w as u16, self.span_h as u16)
        } else {
            (1, 1)
        }
    }

    /// Returns this tile's `(dx, dy)` offset back to its span anchor, or `None` when it is not
    /// covered by one.
    ///
    /// A covered cell at `(x, y)` has its anchor at `(x - dx, y - dy)`, so a backend holding a
    /// whole layer reaches it with one subtraction. A caller holding a
    /// [`Grid`](crate::grid::Grid) should use
    /// [`Grid::span_owner`](crate::grid::Grid::span_owner) instead, which handles the bounds and
    /// the anchor-cell case too.
    #[must_use]
    pub const fn span_offset(&self) -> Option<(u16, u16)> {
        if self.flags.contains(TileFlags::SPAN_COVERED) {
            Some((self.span_w as u16, self.span_h as u16))
        } else {
            None
        }
    }

    /// Returns the flat index of this tile's span anchor in a row-major buffer, given this
    /// tile's own flat `idx` and the buffer's row stride `cols`.
    ///
    /// `None` when this tile is not [`TileFlags::SPAN_COVERED`] (see [`span_offset`]), or when
    /// the offset would land before the start of the buffer. This does not check `idx` against
    /// the buffer's length or that the anchor is in the same row-block as `idx`; a caller holding
    /// a whole layer already knows both hold.
    ///
    /// [`span_offset`]: Self::span_offset
    #[must_use]
    pub const fn span_anchor_index(&self, idx: usize, cols: usize) -> Option<usize> {
        let Some((dx, dy)) = self.span_offset() else {
            return None;
        };
        idx.checked_sub(dy as usize * cols + dx as usize)
    }

    /// Returns `true` if nothing has been written to this tile.
    ///
    /// Empty tiles are transparent when compositing layers. An explicit
    /// space (e.g. `Tile::new(' ', style)`) is **not** empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.flags.contains(TileFlags::EMPTY)
    }

    /// Returns `true` if this tile is the left half of a 2-column wide character.
    #[must_use]
    pub const fn is_wide(&self) -> bool {
        self.flags.contains(TileFlags::WIDE_CHAR)
    }

    /// Returns `true` if this tile is the invisible right-half spacer of a wide character.
    #[must_use]
    pub const fn is_wide_spacer(&self) -> bool {
        self.flags.contains(TileFlags::WIDE_CHAR_SPACER)
    }

    /// Returns `true` if this tile is the top-left anchor of a multi-cell span (see
    /// [`span`](Self::span)).
    ///
    /// Unlike `span() != (1, 1)`, this is accurate for a 1x1 span: a span anchor whose declared
    /// footprint happens to be one cell still reports `true` here, whereas its `span()` is
    /// indistinguishable from a plain tile's.
    #[must_use]
    pub const fn is_span_anchor(&self) -> bool {
        self.flags.contains(TileFlags::SPAN_ANCHOR)
    }

    /// Sets the glyph for this tile (builder style).
    ///
    /// Writing content marks the tile non-empty (see [`is_empty`](Self::is_empty)). Recomputes
    /// the cached display width (see [`width`](Self::width)) for the new glyph, and clears
    /// [`TileFlags::WIDE_CHAR`]/[`TileFlags::WIDE_CHAR_SPACER`], which describe the old glyph's
    /// role and would otherwise disagree with the recomputed width.
    #[must_use]
    pub fn with_glyph(mut self, glyph: char) -> Self {
        self.glyph = glyph;
        self.width = glyph_width(glyph);
        self.flags = self
            .flags
            .difference(TileFlags::EMPTY | TileFlags::WIDE_CHAR | TileFlags::WIDE_CHAR_SPACER);
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
    /// Does not touch the owning [`Grid`](crate::grid::Grid)'s EGC side-table; callers that
    /// reset a tile which may have carried [`TileFlags::HAS_EXTRA`] are
    /// responsible for also clearing that entry (see `Grid::clear_overlap`).
    pub(crate) fn reset(&mut self) {
        self.glyph = ' ';
        self.style = Style::default();
        self.width = 1;
        self.dx = 0;
        self.dy = 0;
        self.flags = TileFlags::EMPTY;
        self.span_w = 1;
        self.span_h = 1;
    }

    /// Strips this tile's multi-cell span role, leaving its glyph and style alone.
    ///
    /// Used by copy paths that cannot preserve a span's cross-cell invariant
    /// ([`Grid::blit`](crate::grid::Grid::blit) can clip a footprint in half), so the copy
    /// degrades to exactly the span's text fallback instead of to a dangling anchor.
    pub(crate) fn clear_span(&mut self) {
        self.flags
            .remove(TileFlags::SPAN_ANCHOR | TileFlags::SPAN_COVERED);
        self.span_w = 1;
        self.span_h = 1;
    }

    /// Strips this tile's wide-character-pair role, leaving its glyph and style alone.
    ///
    /// The wide-character counterpart to [`clear_span`](Self::clear_span): used by copy paths
    /// that cannot preserve a wide pair's cross-cell invariant ([`Grid::blit`](crate::grid::Grid::blit)
    /// can clip a pair in half via `src_rect`, or land on only one half of a destination pair), so
    /// the copy degrades to a plain, unpaired cell instead of a dangling lead or spacer.
    pub(crate) fn clear_wide(&mut self) {
        self.flags
            .remove(TileFlags::WIDE_CHAR | TileFlags::WIDE_CHAR_SPACER);
    }
}

/// Returns `grapheme` truncated to at most 8 codepoints (combining-mark bomb defence). If the
/// input is already within the limit it is returned as-is.
///
/// The cap bounds how much text one cell can pull into its layer's EGC side-table, so a string
/// of thousands of combining marks on a single base character can't blow up per-cell storage.
/// 8 is chosen to clear the longest clusters a caller can reasonably intend (a base plus a couple
/// of combining marks, or an emoji ZWJ sequence of a few joined code points) while still cutting
/// off an adversarial run early. A cluster longer than 8 is truncated on a code-point boundary,
/// so the stored text stays valid UTF-8 but may render differently than the untruncated input.
///
/// The exact value was picked by headroom, not measured against a corpus of real clusters; raise
/// it if a legitimate sequence turns out to exceed it.
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

    /// `with_style` only touches `style` and the `EMPTY` flag; the glyph and its precomputed
    /// width must survive untouched.
    #[test]
    fn test_tile_with_style_preserves_glyph_and_width() {
        let tile = Tile::new('漢', Style::default()).with_style(Style::new().fg(Color::RED));
        assert_eq!(tile.glyph(), '漢');
        assert_eq!(tile.width(), 2);
        assert_eq!(tile.style(), Style::new().fg(Color::RED));
    }

    /// `with_offset` only touches `dx`/`dy` and the `EMPTY` flag; the glyph and its precomputed
    /// width must survive untouched.
    #[test]
    fn test_tile_with_offset_preserves_glyph_and_width() {
        let tile = Tile::new('漢', Style::default()).with_offset(1, 1);
        assert_eq!(tile.glyph(), '漢');
        assert_eq!(tile.width(), 2);
    }

    /// Each `with_offset` call sets `dx`/`dy` outright rather than accumulating; a second call
    /// in a chain must not leave anything from the first behind.
    #[test]
    fn test_tile_with_offset_chain_does_not_leak_prior_values() {
        let tile = Tile::new('X', Style::default())
            .with_offset(-3, 5)
            .with_offset(2, -1);
        assert_eq!(tile.dx, 2);
        assert_eq!(tile.dy, -1);
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

    /// Control characters report `None` from `unicode_width`; `glyph_width`'s `unwrap_or(1)`
    /// fallback treats them as single-column, matching this crate's prior per-cell behavior.
    #[test]
    fn test_tile_width_falls_back_to_one_for_control_characters() {
        assert_eq!(Tile::new('\t', Style::default()).width(), 1);
        assert_eq!(Tile::new('\u{7}', Style::default()).width(), 1);
        assert_eq!(Tile::new('\u{1b}', Style::default()).width(), 1);
    }

    /// Combining marks and zero-width joiners are genuinely zero-column: unlike control
    /// characters, `unicode_width` reports `Some(0)` for these rather than `None`, so they skip
    /// the `unwrap_or` fallback entirely.
    #[test]
    fn test_tile_width_is_zero_for_zero_width_glyphs() {
        assert_eq!(Tile::new('\u{0301}', Style::default()).width(), 0);
        assert_eq!(Tile::new('\u{200d}', Style::default()).width(), 0);
    }

    #[test]
    fn test_tile_with_glyph_recomputes_width() {
        let tile = Tile::new('A', Style::default()).with_glyph('漢');
        assert_eq!(tile.glyph(), '漢');
        assert_eq!(tile.width(), 2);
    }

    /// A tile carrying a stale `WIDE_CHAR_SPACER` (e.g. read back out of a grid via
    /// `*grid.tile(..)`) must not keep that flag once `with_glyph` gives it a real glyph: the
    /// flag tells `Grid::put_tile` to treat the tile as an already-resolved replay and store it
    /// verbatim, which means every backend skips drawing it (see issue #986).
    #[test]
    fn test_tile_with_glyph_clears_stale_wide_char_spacer_flag() {
        let mut spacer = Tile::new(' ', Style::default());
        spacer.flags = TileFlags::WIDE_CHAR_SPACER;

        let rebuilt = spacer.with_glyph('!');

        assert_eq!(rebuilt.glyph(), '!');
        assert_eq!(rebuilt.width(), 1);
        assert!(!rebuilt.flags().contains(TileFlags::WIDE_CHAR_SPACER));
        assert!(!rebuilt.is_empty());
    }

    /// A tile carrying a stale `WIDE_CHAR` must not keep that flag once `with_glyph` narrows it:
    /// the flag tells `Grid::clear_overlap` that the cell to the right is this tile's spacer, so
    /// a stale flag makes an overlapping write reset an unrelated neighbour (see issue #986).
    #[test]
    fn test_tile_with_glyph_clears_stale_wide_char_flag() {
        let mut wide = Tile::new('漢', Style::default());
        wide.flags = TileFlags::WIDE_CHAR;

        let rebuilt = wide.with_glyph('A');

        assert_eq!(rebuilt.glyph(), 'A');
        assert_eq!(rebuilt.width(), 1);
        assert!(!rebuilt.flags().contains(TileFlags::WIDE_CHAR));
    }

    #[test]
    fn test_tile_flag_predicates() {
        let mut tile = Tile::new('A', Style::default());
        assert!(!tile.is_wide());
        assert!(!tile.is_wide_spacer());
        assert!(!tile.is_span_anchor());

        tile.flags = TileFlags::WIDE_CHAR;
        assert!(tile.is_wide());
        assert!(!tile.is_wide_spacer());
        assert!(!tile.is_span_anchor());

        tile.flags = TileFlags::WIDE_CHAR_SPACER;
        assert!(!tile.is_wide());
        assert!(tile.is_wide_spacer());
        assert!(!tile.is_span_anchor());

        // A 1x1 span anchor is still an anchor even though its `span()` matches a plain tile's.
        tile.flags = TileFlags::SPAN_ANCHOR;
        tile.span_w = 1;
        tile.span_h = 1;
        assert!(tile.is_span_anchor());
        assert_eq!(tile.span(), (1, 1));
    }

    #[test]
    fn test_tile_span_defaults_to_one_by_one() {
        assert_eq!(Tile::default().span(), (1, 1));
        assert_eq!(Tile::new('A', Style::default()).span(), (1, 1));
        assert_eq!(Tile::default().span_offset(), None);
        assert_eq!(Tile::new('A', Style::default()).span_offset(), None);
    }

    /// `span_w`/`span_h` are overloaded by role, so reading them through the wrong accessor must
    /// report the neutral answer rather than the other role's number.
    #[test]
    fn test_tile_span_accessors_are_keyed_by_role() {
        let mut anchor = Tile::new('C', Style::default());
        anchor.flags = TileFlags::SPAN_ANCHOR;
        anchor.span_w = 2;
        anchor.span_h = 3;
        assert_eq!(anchor.span(), (2, 3));
        assert_eq!(anchor.span_offset(), None);

        let mut covered = Tile::new(']', Style::default());
        covered.flags = TileFlags::SPAN_COVERED;
        covered.span_w = 1;
        covered.span_h = 2;
        assert_eq!(covered.span_offset(), Some((1, 2)));
        assert_eq!(covered.span(), (1, 1));
    }

    #[test]
    fn test_tile_span_anchor_index_resolves_a_covered_cell_to_its_anchor() {
        let mut covered = Tile::new(']', Style::default());
        covered.flags = TileFlags::SPAN_COVERED;
        covered.span_w = 1;
        covered.span_h = 2;
        // idx 23 is (3, 2) in a 10-wide buffer; the anchor is (dx, dy) = (1, 2) back, at (2, 0).
        assert_eq!(covered.span_anchor_index(23, 10), Some(2));
    }

    #[test]
    fn test_tile_span_anchor_index_is_none_when_not_covered() {
        assert_eq!(Tile::default().span_anchor_index(5, 10), None);

        let mut anchor = Tile::new('C', Style::default());
        anchor.flags = TileFlags::SPAN_ANCHOR;
        anchor.span_w = 2;
        anchor.span_h = 3;
        assert_eq!(anchor.span_anchor_index(5, 10), None);
    }

    #[test]
    fn test_tile_span_anchor_index_is_none_past_the_buffer_start() {
        let mut covered = Tile::new(']', Style::default());
        covered.flags = TileFlags::SPAN_COVERED;
        covered.span_w = 1;
        covered.span_h = 2;
        assert_eq!(covered.span_anchor_index(1, 10), None);
    }

    /// `cols == 0` is a caller error (there is no valid row stride), but the method has no way to
    /// detect it: `checked_sub` only guards against the anchor landing before the buffer start,
    /// not against a degenerate stride. Documented here rather than in the method's doc, which
    /// lists exactly the two `None` conditions this is not one of.
    #[test]
    fn test_tile_span_anchor_index_does_not_detect_a_zero_stride() {
        let mut covered = Tile::new(']', Style::default());
        covered.flags = TileFlags::SPAN_COVERED;
        covered.span_w = 1;
        covered.span_h = 0;
        assert_eq!(covered.span_anchor_index(1, 0), Some(0));
    }

    /// The method does not check that the resolved anchor is in the same row-block as `idx`; a
    /// covered cell whose `dx` exceeds its own column lands on the last cell of the *previous*
    /// row instead of returning `None`. `Grid::write_span` can never produce this (a span's
    /// footprint always fits, so `x >= dx` holds for every covered cell it writes), so this pins
    /// the doc's "caller already knows this holds" precondition rather than guarding a real bug.
    #[test]
    fn test_tile_span_anchor_index_does_not_detect_crossing_a_row_block() {
        let mut covered = Tile::new(']', Style::default());
        covered.flags = TileFlags::SPAN_COVERED;
        covered.span_w = 1;
        covered.span_h = 0;
        // idx 4 is (0, 1) in a 4-wide buffer; dx = 1 walks back past column 0 into row 0's tail.
        assert_eq!(covered.span_anchor_index(4, 4), Some(3));
    }

    #[test]
    fn test_tile_clear_span_keeps_the_glyph() {
        let mut tile = Tile::new('C', Style::default());
        tile.flags = TileFlags::SPAN_ANCHOR;
        tile.span_w = 2;
        tile.span_h = 2;
        tile.clear_span();
        assert_eq!(tile.glyph(), 'C');
        assert_eq!(tile.span(), (1, 1));
        assert!(!tile.flags().contains(TileFlags::SPAN_ANCHOR));
    }

    #[test]
    fn test_tile_reset_clears_span() {
        let mut tile = Tile::new('C', Style::default());
        tile.flags = TileFlags::SPAN_ANCHOR;
        tile.span_w = 4;
        tile.span_h = 4;
        tile.reset();
        assert_eq!(tile.span(), (1, 1));
        assert_eq!(tile.span_offset(), None);
        assert!(tile.is_empty());
    }

    /// The derived `Default` on `TileFlags` is `empty()`, not `EMPTY`, which disagrees with the
    /// flags a default `Tile` actually carries. Nothing in the workspace calls
    /// `TileFlags::default()`; this pins the divergence rather than silently relying on it, given
    /// how easy it would be to reach for `TileFlags::default()` expecting `EMPTY` back.
    #[test]
    fn test_tile_flags_default_is_not_empty_flag() {
        assert_eq!(TileFlags::default(), TileFlags::empty());
        assert_ne!(TileFlags::default(), TileFlags::EMPTY);
        assert_eq!(Tile::default().flags(), TileFlags::EMPTY);
    }

    #[cfg(feature = "egc")]
    #[test]
    fn test_cap_grapheme_leaves_short_input_unchanged() {
        assert_eq!(cap_grapheme(""), "");
        assert_eq!(cap_grapheme("a"), "a");
        assert_eq!(cap_grapheme("e\u{0301}"), "e\u{0301}");
    }

    #[cfg(feature = "egc")]
    #[test]
    fn test_cap_grapheme_leaves_exactly_the_cap_unchanged() {
        // 8 codepoints: the boundary itself must not be truncated.
        let input: String = core::iter::repeat_n('\u{0301}', 8).collect();
        assert_eq!(cap_grapheme(&input), input);
    }

    #[cfg(feature = "egc")]
    #[test]
    fn test_cap_grapheme_truncates_past_the_cap_on_a_codepoint_boundary() {
        // 9 codepoints, each multi-byte (U+0301 is 2 bytes in UTF-8), so a byte-oriented
        // truncation would split a codepoint; `cap_grapheme` must not.
        let input: String = core::iter::repeat_n('\u{0301}', 9).collect();
        let capped = cap_grapheme(&input);
        assert_eq!(capped.chars().count(), 8);
        assert!(capped.is_char_boundary(capped.len()));
        let expected: String = core::iter::repeat_n('\u{0301}', 8).collect();
        assert_eq!(capped, expected);
    }
}
