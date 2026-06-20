//! Tileset configuration: codepage mappings, options, builder, and error types.
//!
//! This module defines the public API for configuring PNG sprite sheet tilesets
//! that overlay or replace [`BitmapFont`](super::bitmap_font::BitmapFont) glyphs.
//!
//! See ADR 009 for the full design.

use core::fmt;

/// Errors that can occur during tileset validation or decoding.
#[derive(Debug)]
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
    /// `spacing_cells_x` or `spacing_cells_y` is zero.
    ZeroSpacing,
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
            Self::ZeroSpacing => {
                write!(f, "spacing_cells_x and spacing_cells_y must be non-zero")
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
            Self::Custom(table) => table.get(i).copied(),
        }
    }

    /// Number of tiles this codepage defines, or `None` for `Unicode` (unbounded).
    #[must_use]
    pub fn len(&self) -> Option<usize> {
        match self {
            Self::Cp437 => Some(256),
            Self::Unicode { .. } => None,
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
    /// Number of grid cells this sprite spans horizontally. Must be >= 1.
    pub spacing_cells_x: u16,
    /// Number of grid cells this sprite spans vertically. Must be >= 1.
    pub spacing_cells_y: u16,
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
    #[must_use]
    pub const fn from_bytes(bytes: Vec<u8>) -> TilesetBuilder {
        TilesetBuilder {
            bytes,
            tile_width: 0,
            tile_height: 0,
            columns: None,
            codepage: Codepage::Cp437,
            spacing_cells_x: 1,
            spacing_cells_y: 1,
            transparent_color: None,
        }
    }
}

/// Builder for [`TilesetOptions`].
///
/// Construct via [`TilesetOptions::from_bytes`].
///
/// # Example
///
/// ```ignore
/// use rg::backend::software::tileset::{
///     Codepage, TilesetOptions,
/// };
///
/// let png_data: Vec<u8> = vec![]; // real PNG data
/// let opts = TilesetOptions::from_bytes(png_data)
///     .tile_size(16, 16)
///     .start_codepoint('\u{E000}')
///     .spacing(2, 2)
///     .build()
///     .unwrap();
/// ```
pub struct TilesetBuilder {
    bytes: Vec<u8>,
    tile_width: u16,
    tile_height: u16,
    columns: Option<u16>,
    codepage: Codepage,
    spacing_cells_x: u16,
    spacing_cells_y: u16,
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

    /// Number of grid cells each sprite occupies (width x height).
    ///
    /// Defaults to (1, 1). A value of (2, 2) means the sprite spans 2x2 cells.
    #[must_use]
    pub const fn spacing(mut self, x: u16, y: u16) -> Self {
        self.spacing_cells_x = x;
        self.spacing_cells_y = y;
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
    /// Returns [`TilesetError::ZeroTileSize`] if tile dimensions are 0,
    /// [`TilesetError::ZeroSpacing`] if spacing is 0, or
    /// [`TilesetError::EmptyCodepage`] if `Custom` codepage is empty.
    pub fn build(self) -> Result<TilesetOptions, TilesetError> {
        if self.tile_width == 0 || self.tile_height == 0 {
            return Err(TilesetError::ZeroTileSize);
        }
        if self.spacing_cells_x == 0 || self.spacing_cells_y == 0 {
            return Err(TilesetError::ZeroSpacing);
        }
        if let Codepage::Custom(ref t) = self.codepage {
            if t.is_empty() {
                return Err(TilesetError::EmptyCodepage);
            }
        }
        Ok(TilesetOptions {
            bytes: self.bytes,
            tile_width: self.tile_width,
            tile_height: self.tile_height,
            columns: self.columns,
            codepage: self.codepage,
            spacing_cells_x: self.spacing_cells_x,
            spacing_cells_y: self.spacing_cells_y,
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
    fn tileset_builder_rejects_zero_spacing() {
        let opts = TilesetOptions::from_bytes(vec![])
            .tile_size(16, 16)
            .spacing(0, 1)
            .build();
        assert!(matches!(opts, Err(TilesetError::ZeroSpacing)));
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
            .spacing(2, 2)
            .build()
            .unwrap();
        assert_eq!(opts.tile_width, 16);
        assert_eq!(opts.spacing_cells_x, 2);
        assert!(matches!(
            opts.codepage,
            Codepage::Unicode { start: '\u{E000}' }
        ));
    }

    #[test]
    fn cp437_codepage_spot_checks() {
        assert_eq!(Codepage::Cp437.codepoint(32), Some(' '));
        assert_eq!(Codepage::Cp437.codepoint(64), Some('@'));
        assert_eq!(Codepage::Cp437.codepoint(176), Some('\u{2591}'));
        assert_eq!(Codepage::Cp437.codepoint(256), None);
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
