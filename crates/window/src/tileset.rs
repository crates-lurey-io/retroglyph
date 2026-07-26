//! Tileset configuration: codepage mappings, options, builder, and error types.
//!
//! This module defines the public API for configuring PNG sprite sheet tilesets
//! that overlay or replace [`BitmapFont`](crate::font::BitmapFont) glyphs.
//! A tileset is a sprite sheet PNG sliced into equally sized tiles, each
//! mapped to a Unicode codepoint via a [`Codepage`]; [`SpriteCache`](crate::sprite_cache::SpriteCache)
//! decodes and indexes those tiles for lookup by glyph at draw time.

use core::fmt;

/// Where a sprite sits inside the multi-cell box a span reserves for it.
///
/// Only observable when the reserved box is larger than the sprite's own pixels, i.e. when
/// [`Surface::put_span`](retroglyph_core::Surface::put_span) declares more cells than the
/// artwork fills. A sprite drawn into a box its art exactly fills (the common case) renders
/// identically under every variant. Mirrors `BearLibTerminal`'s tileset `align=` option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum SpriteAlign {
    /// Flush with the box's top-left corner.
    #[default]
    TopLeft,
    /// Centred horizontally, flush with the top edge.
    Top,
    /// Flush with the top-right corner.
    TopRight,
    /// Flush with the left edge, centred vertically.
    Left,
    /// Centred on both axes.
    Center,
    /// Flush with the right edge, centred vertically.
    Right,
    /// Flush with the bottom-left corner.
    BottomLeft,
    /// Centred horizontally, flush with the bottom edge.
    Bottom,
    /// Flush with the bottom-right corner.
    BottomRight,
}

impl SpriteAlign {
    /// Returns the offset of a `sprite_w` x `sprite_h` sprite placed inside a `box_w` x `box_h`
    /// box, in unscaled pixels to match [`Tile::dx`](retroglyph_core::Tile::dx).
    ///
    /// Centring uses integer division, so an odd leftover pixel lands on the right/bottom side.
    /// Saturates at `0` on either axis where the sprite is at least as large as the box, so an
    /// oversized sprite is never pulled off its own anchor cell.
    ///
    /// # Examples
    ///
    /// ```
    /// use retroglyph_window::tileset::SpriteAlign;
    ///
    /// // 16x16 art in a 32x32 box leaves 16 pixels of slack on each axis.
    /// assert_eq!(SpriteAlign::Center.offset(16, 16, 32, 32), (8, 8));
    /// assert_eq!(SpriteAlign::BottomRight.offset(16, 16, 32, 32), (16, 16));
    /// // Art that fills its box renders identically under every variant.
    /// assert_eq!(SpriteAlign::Center.offset(16, 16, 16, 16), (0, 0));
    /// ```
    #[must_use]
    pub const fn offset(self, sprite_w: u32, sprite_h: u32, box_w: u32, box_h: u32) -> (i16, i16) {
        let slack_x = box_w.saturating_sub(sprite_w);
        let slack_y = box_h.saturating_sub(sprite_h);
        let (x, y) = match self {
            Self::TopLeft => (0, 0),
            Self::Top => (slack_x / 2, 0),
            Self::TopRight => (slack_x, 0),
            Self::Left => (0, slack_y / 2),
            Self::Center => (slack_x / 2, slack_y / 2),
            Self::Right => (slack_x, slack_y / 2),
            Self::BottomLeft => (0, slack_y),
            Self::Bottom => (slack_x / 2, slack_y),
            Self::BottomRight => (slack_x, slack_y),
        };
        // Slack is bounded by the box, which is a cell count times a `u8` glyph size, so it
        // cannot reach `i16::MAX` for any grid a backend can actually render. `saturating_as`
        // isn't const, hence the explicit clamp.
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        (
            (if x > i16::MAX as u32 {
                i16::MAX as u32
            } else {
                x
            }) as i16,
            (if y > i16::MAX as u32 {
                i16::MAX as u32
            } else {
                y
            }) as i16,
        )
    }
}

/// Errors that can occur during tileset validation or decoding.
#[derive(Debug)]
#[non_exhaustive]
pub enum TilesetError {
    /// PNG decode failed.
    PngDecode(String),
    /// The image dimensions are not evenly divisible by the declared tile size.
    InvalidDimensions(u32, u32, u16, u16),
    /// The codepage mapping table has zero entries.
    EmptyCodepage,
    /// The pixel format is not RGBA8 or RGB8.
    UnsupportedPixelFormat(String),
    /// `tile_width` or `tile_height` is zero.
    ZeroTileSize,
}

impl fmt::Display for TilesetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PngDecode(e) => write!(f, "png decode failed: {e}"),
            Self::InvalidDimensions(iw, ih, tw, th) => {
                write!(f, "image {iw}x{ih} is not divisible by tile size {tw}x{th}")
            }
            Self::EmptyCodepage => write!(f, "codepage mapping has no entries"),
            Self::UnsupportedPixelFormat(fmt_name) => {
                write!(
                    f,
                    "unsupported pixel format: {fmt_name}; expected RGBA8 or RGB8"
                )
            }
            Self::ZeroTileSize => {
                write!(f, "tile_width and tile_height must be non-zero")
            }
        }
    }
}

impl std::error::Error for TilesetError {}

/// Maps row-major tile indices in a sprite sheet to Unicode codepoints.
///
/// `#[non_exhaustive]` allows adding new variants (e.g. `Cp1252`) without a
/// semver break.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Codepage {
    /// Standard CP437 layout: the i-th tile maps to `CP437_TO_UNICODE[i]`.
    ///
    /// Only the first 256 tiles in the sheet are mapped; extras are ignored.
    Cp437,
    /// Starting at `start`, tile index `i` maps to `char::from_u32(start as u32 + i)`.
    ///
    /// Tiles that would map to a surrogate or exceed `char::MAX` are skipped.
    Unicode {
        /// Codepoint of the first tile; subsequent tiles increment by 1.
        start: char,
    },
    /// Positional mapping: tile index `i` maps to `char::from_u32(i)`.
    ///
    /// This is the simplest option when you don't care about Unicode semantics
    /// and just want to reference tiles by a zero-based index. Use
    /// [`Tile::glyph`](retroglyph_core::Tile::glyph) values 0, 1, 2, … to address
    /// individual sprites in sheet order.
    ///
    /// Tiles whose index falls in the surrogate range (0xD800–0xDFFF) are
    /// skipped; all others are valid.
    Identity,
    /// Explicit mapping: tile `i` maps to `table[i]`.
    ///
    /// Tiles beyond `table.len()` are ignored.
    Custom(Vec<char>),
}

impl Codepage {
    /// Returns the codepoint for tile index `i`, or `None` if out of range
    /// or invalid (surrogates, indices past `char::MAX`).
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn codepoint(&self, i: usize) -> Option<char> {
        match self {
            Self::Cp437 => CP437_TO_UNICODE.get(i).copied(),
            Self::Unicode { start } => {
                let scalar = (*start as u32).checked_add(i as u32)?;
                char::from_u32(scalar)
            }
            Self::Identity => char::from_u32(i as u32),
            Self::Custom(table) => table.get(i).copied(),
        }
    }

    /// Number of tiles this codepage defines, or `None` for unbounded variants.
    #[must_use]
    pub const fn len(&self) -> Option<usize> {
        match self {
            Self::Cp437 => Some(256),
            Self::Unicode { .. } | Self::Identity => None,
            Self::Custom(t) => Some(t.len()),
        }
    }

    /// Returns `true` if the codepage defines zero tiles.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == Some(0)
    }
}

/// Standard IBM CP437 to Unicode mapping, 256 entries.
pub const CP437_TO_UNICODE: [char; 256] = [
    '\u{0000}', '\u{263A}', '\u{263B}', '\u{2665}', '\u{2666}', '\u{2663}', '\u{2660}', '\u{2022}',
    '\u{25D8}', '\u{25CB}', '\u{25D9}', '\u{2642}', '\u{2640}', '\u{266A}', '\u{266B}', '\u{263C}',
    '\u{25BA}', '\u{25C4}', '\u{2195}', '\u{203C}', '\u{00B6}', '\u{00A7}', '\u{25AC}', '\u{21A8}',
    '\u{2191}', '\u{2193}', '\u{2192}', '\u{2190}', '\u{221F}', '\u{2194}', '\u{25B2}', '\u{25BC}',
    ' ', '!', '"', '#', '$', '%', '&', '\'', '(', ')', '*', '+', ',', '-', '.', '/', '0', '1', '2',
    '3', '4', '5', '6', '7', '8', '9', ':', ';', '<', '=', '>', '?', '@', 'A', 'B', 'C', 'D', 'E',
    'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X',
    'Y', 'Z', '[', '\\', ']', '^', '_', '`', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k',
    'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '{', '|', '}', '~',
    '\u{2302}', '\u{00C7}', '\u{00FC}', '\u{00E9}', '\u{00E2}', '\u{00E4}', '\u{00E0}', '\u{00E5}',
    '\u{00E7}', '\u{00EA}', '\u{00EB}', '\u{00E8}', '\u{00EF}', '\u{00EE}', '\u{00EC}', '\u{00C4}',
    '\u{00C5}', '\u{00C9}', '\u{00E6}', '\u{00C6}', '\u{00F4}', '\u{00F6}', '\u{00F2}', '\u{00FB}',
    '\u{00F9}', '\u{00FF}', '\u{00D6}', '\u{00DC}', '\u{00A2}', '\u{00A3}', '\u{00A5}', '\u{20A7}',
    '\u{0192}', '\u{00E1}', '\u{00ED}', '\u{00F3}', '\u{00FA}', '\u{00F1}', '\u{00D1}', '\u{00AA}',
    '\u{00BA}', '\u{00BF}', '\u{2310}', '\u{00AC}', '\u{00BD}', '\u{00BC}', '\u{00A1}', '\u{00AB}',
    '\u{00BB}', '\u{2591}', '\u{2592}', '\u{2593}', '\u{2502}', '\u{2524}', '\u{2561}', '\u{2562}',
    '\u{2556}', '\u{2555}', '\u{2563}', '\u{2551}', '\u{2557}', '\u{255D}', '\u{255C}', '\u{255B}',
    '\u{2510}', '\u{2514}', '\u{2534}', '\u{252C}', '\u{251C}', '\u{2500}', '\u{253C}', '\u{255E}',
    '\u{255F}', '\u{255A}', '\u{2554}', '\u{2569}', '\u{2566}', '\u{2560}', '\u{2550}', '\u{256C}',
    '\u{2567}', '\u{2568}', '\u{2564}', '\u{2565}', '\u{2559}', '\u{2558}', '\u{2552}', '\u{2553}',
    '\u{256B}', '\u{256A}', '\u{2518}', '\u{250C}', '\u{2588}', '\u{2584}', '\u{258C}', '\u{2590}',
    '\u{2580}', '\u{03B1}', '\u{00DF}', '\u{0393}', '\u{03C0}', '\u{03A3}', '\u{03C3}', '\u{00B5}',
    '\u{03C4}', '\u{03A6}', '\u{0398}', '\u{03A9}', '\u{03B4}', '\u{221E}', '\u{03C6}', '\u{03B5}',
    '\u{2229}', '\u{2261}', '\u{00B1}', '\u{2265}', '\u{2264}', '\u{2320}', '\u{2321}', '\u{00F7}',
    '\u{2248}', '\u{00B0}', '\u{2219}', '\u{00B7}', '\u{221A}', '\u{207F}', '\u{00B2}', '\u{25A0}',
    '\u{00A0}',
];

/// Options for loading a single tileset (sprite sheet).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TilesetOptions {
    /// Raw bytes of the PNG file.
    pub bytes: Vec<u8>,
    /// Width of a single tile in pixels.
    pub tile_width: u16,
    /// Height of a single tile in pixels.
    pub tile_height: u16,
    /// Number of tiles per row in the sprite sheet.
    ///
    /// If `None`, derived as `image_width / tile_width`.
    pub columns: Option<u16>,
    /// Codepoint mapping from tile index to Unicode character.
    pub codepage: Codepage,
    /// Where each sprite sits inside the multi-cell box a span reserves for it.
    pub align: SpriteAlign,
    /// If set, any pixel matching this RGB colour is made fully transparent
    /// (alpha = 0) when decoding the tileset.
    ///
    /// Useful for spritesheets that use a solid colour background instead
    /// of an alpha channel.  Equivalent to bracket-lib's `with_font_bg()`
    /// or doryen-rs's top-left-pixel key colour auto-detection.
    pub transparent_color: Option<(u8, u8, u8)>,
}

impl TilesetOptions {
    /// Starts building a tileset from raw PNG bytes.
    ///
    /// Pass `include_bytes!("...").to_vec()` to embed the asset at compile
    /// time, or `std::fs::read(path)?` to load it at runtime.
    #[must_use]
    pub const fn from_bytes(bytes: Vec<u8>) -> TilesetBuilder {
        TilesetBuilder {
            bytes,
            tile_width: 0,
            tile_height: 0,
            columns: None,
            codepage: Codepage::Cp437,
            align: SpriteAlign::TopLeft,
            transparent_color: None,
        }
    }
}

/// Builder for [`TilesetOptions`].
///
/// Construct via [`TilesetOptions::from_bytes`].
///
/// [`columns`](TilesetBuilder::columns) defaults to `image_width / tile_width`,
/// so you usually don't need to set it explicitly. [`codepage`](TilesetBuilder::codepage)
/// defaults to [`Codepage::Cp437`].
///
/// # Examples
///
/// Standard CP437 tileset:
///
/// ```ignore
/// use retroglyph_window::tileset::TilesetOptions;
///
/// let png: Vec<u8> = std::fs::read("assets/cp437_16x16.png").unwrap();
/// let opts = TilesetOptions::from_bytes(png)
///     .tile_size(16, 16) // codepage defaults to Cp437
///     .build()
///     .unwrap();
/// ```
///
/// Private-use sprite sheet addressed by index, centred in whatever box a span reserves:
///
/// ```ignore
/// use retroglyph_window::tileset::{Codepage, SpriteAlign, TilesetOptions};
///
/// let png: Vec<u8> = std::fs::read("assets/sprites.png").unwrap();
/// let opts = TilesetOptions::from_bytes(png)
///     .tile_size(32, 32)
///     .codepage(Codepage::Identity)  // tile 0 = '\0', tile 1 = '\x01', …
///     .align(SpriteAlign::Center)
///     .build()
///     .unwrap();
/// ```
///
/// How many cells a sprite occupies is a per-write decision, not a tileset-wide one: declare it
/// with [`Surface::put_span`](retroglyph_core::Surface::put_span) at the draw call.
///
/// Unicode private-use area sprite sheet:
///
/// ```ignore
/// use retroglyph_window::tileset::TilesetOptions;
///
/// let png: Vec<u8> = std::fs::read("assets/monsters.png").unwrap();
/// let opts = TilesetOptions::from_bytes(png)
///     .tile_size(16, 16)
///     .start_codepoint('\u{E000}') // maps to Unicode PUA starting at U+E000
///     .build()
///     .unwrap();
/// ```
pub struct TilesetBuilder {
    bytes: Vec<u8>,
    tile_width: u16,
    tile_height: u16,
    columns: Option<u16>,
    codepage: Codepage,
    align: SpriteAlign,
    transparent_color: Option<(u8, u8, u8)>,
}

impl TilesetBuilder {
    /// Sets the pixel dimensions of each tile.
    #[must_use]
    pub const fn tile_size(mut self, width: u16, height: u16) -> Self {
        self.tile_width = width;
        self.tile_height = height;
        self
    }

    /// Sets the number of tiles per row in the sprite sheet.
    ///
    /// Useful for sheets with padding. If not set, derived from image width.
    #[must_use]
    pub const fn columns(mut self, cols: u16) -> Self {
        self.columns = Some(cols);
        self
    }

    /// Sets the codepoint mapping.
    #[must_use]
    pub fn codepage(mut self, codepage: Codepage) -> Self {
        self.codepage = codepage;
        self
    }

    /// Sets the codepoint of the first tile; subsequent tiles increment by 1.
    ///
    /// Shorthand for `codepage(Codepage::Unicode { start })`.
    #[must_use]
    pub fn start_codepoint(mut self, start: char) -> Self {
        self.codepage = Codepage::Unicode { start };
        self
    }

    /// Sets where each sprite sits inside the multi-cell box a span reserves for it.
    ///
    /// Defaults to [`SpriteAlign::TopLeft`]. Has no visible effect on a sprite whose art fills
    /// its box exactly; see [`SpriteAlign`].
    #[must_use]
    pub const fn align(mut self, align: SpriteAlign) -> Self {
        self.align = align;
        self
    }

    /// Pixels matching `(r, g, b)` are made fully transparent (alpha = 0).
    ///
    /// Use this for spritesheets that use a solid colour background instead
    /// of an alpha channel.
    #[must_use]
    pub const fn transparent_color(mut self, r: u8, g: u8, b: u8) -> Self {
        self.transparent_color = Some((r, g, b));
        self
    }

    /// Validates and builds [`TilesetOptions`].
    ///
    /// # Errors
    ///
    /// Returns [`TilesetError::ZeroTileSize`] if tile dimensions are 0, or
    /// [`TilesetError::EmptyCodepage`] if `Custom` codepage is empty.
    pub fn build(self) -> Result<TilesetOptions, TilesetError> {
        if self.tile_width == 0 || self.tile_height == 0 {
            return Err(TilesetError::ZeroTileSize);
        }
        if let Codepage::Custom(ref t) = self.codepage
            && t.is_empty()
        {
            return Err(TilesetError::EmptyCodepage);
        }
        Ok(TilesetOptions {
            bytes: self.bytes,
            tile_width: self.tile_width,
            tile_height: self.tile_height,
            columns: self.columns,
            codepage: self.codepage,
            align: self.align,
            transparent_color: self.transparent_color,
        })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tileset_builder_rejects_zero_tile_size() {
        let opts = TilesetOptions::from_bytes(vec![]).tile_size(0, 16).build();
        assert!(matches!(opts, Err(TilesetError::ZeroTileSize)));
    }

    #[test]
    fn tileset_builder_rejects_empty_custom_codepage() {
        let opts = TilesetOptions::from_bytes(vec![])
            .tile_size(16, 16)
            .codepage(Codepage::Custom(vec![]))
            .build();
        assert!(matches!(opts, Err(TilesetError::EmptyCodepage)));
    }

    #[test]
    fn tileset_builder_valid() {
        let opts = TilesetOptions::from_bytes(vec![0u8; 64])
            .tile_size(16, 16)
            .start_codepoint('\u{E000}')
            .align(SpriteAlign::Center)
            .build()
            .unwrap();
        assert_eq!(opts.tile_width, 16);
        assert_eq!(opts.align, SpriteAlign::Center);
        assert!(matches!(
            opts.codepage,
            Codepage::Unicode { start: '\u{E000}' }
        ));
    }

    #[test]
    fn tileset_builder_defaults_to_top_left_alignment() {
        let opts = TilesetOptions::from_bytes(vec![0u8; 64])
            .tile_size(16, 16)
            .build()
            .unwrap();
        assert_eq!(opts.align, SpriteAlign::TopLeft);
    }

    #[test]
    fn sprite_align_positions_art_within_a_larger_box() {
        // 16x16 art in a 32x32 box: 16 pixels of slack on each axis.
        let at = |align: SpriteAlign| align.offset(16, 16, 32, 32);
        assert_eq!(at(SpriteAlign::TopLeft), (0, 0));
        assert_eq!(at(SpriteAlign::Top), (8, 0));
        assert_eq!(at(SpriteAlign::TopRight), (16, 0));
        assert_eq!(at(SpriteAlign::Left), (0, 8));
        assert_eq!(at(SpriteAlign::Center), (8, 8));
        assert_eq!(at(SpriteAlign::Right), (16, 8));
        assert_eq!(at(SpriteAlign::BottomLeft), (0, 16));
        assert_eq!(at(SpriteAlign::Bottom), (8, 16));
        assert_eq!(at(SpriteAlign::BottomRight), (16, 16));
    }

    #[test]
    fn sprite_align_centring_rounds_down() {
        // 9 pixels of slack: the odd leftover pixel goes to the right/bottom.
        assert_eq!(SpriteAlign::Center.offset(7, 7, 16, 16), (4, 4));
        assert_eq!(SpriteAlign::Center.offset(8, 8, 15, 15), (3, 3));
    }

    #[test]
    fn sprite_align_is_a_no_op_when_the_art_fills_the_box() {
        for align in [
            SpriteAlign::TopLeft,
            SpriteAlign::Top,
            SpriteAlign::TopRight,
            SpriteAlign::Left,
            SpriteAlign::Center,
            SpriteAlign::Right,
            SpriteAlign::BottomLeft,
            SpriteAlign::Bottom,
            SpriteAlign::BottomRight,
        ] {
            assert_eq!(align.offset(16, 16, 16, 16), (0, 0), "{align:?}");
        }
    }

    #[test]
    fn sprite_align_saturates_when_the_art_exceeds_the_box() {
        // Never pull an oversized sprite off its own anchor cell.
        assert_eq!(SpriteAlign::Center.offset(32, 32, 16, 16), (0, 0));
        assert_eq!(SpriteAlign::BottomRight.offset(32, 32, 16, 16), (0, 0));
    }

    #[test]
    fn cp437_codepage_spot_checks() {
        assert_eq!(Codepage::Cp437.codepoint(32), Some(' '));
        assert_eq!(Codepage::Cp437.codepoint(64), Some('@'));
        assert_eq!(Codepage::Cp437.codepoint(176), Some('\u{2591}'));
        assert_eq!(Codepage::Cp437.codepoint(256), None);
    }

    #[test]
    fn identity_codepage_positional() {
        assert_eq!(Codepage::Identity.codepoint(0), Some('\0'));
        assert_eq!(Codepage::Identity.codepoint(65), Some('A'));
        // Surrogate range must be skipped.
        assert_eq!(Codepage::Identity.codepoint(0xD800), None);
        assert_eq!(Codepage::Identity.codepoint(0xDFFF), None);
        // Above surrogates is fine.
        assert_eq!(Codepage::Identity.codepoint(0xE000), Some('\u{E000}'));
    }

    #[test]
    fn unicode_codepage_offset() {
        let cp = Codepage::Unicode { start: '\u{E000}' };
        assert_eq!(cp.codepoint(0), Some('\u{E000}'));
        assert_eq!(cp.codepoint(5), Some('\u{E005}'));
    }

    #[test]
    fn custom_codepage_bounds() {
        let cp = Codepage::Custom(vec!['A', 'B', 'C']);
        assert_eq!(cp.codepoint(0), Some('A'));
        assert_eq!(cp.codepoint(2), Some('C'));
        assert_eq!(cp.codepoint(3), None);
    }
}
