# ADR 009: Tilesets, Sprite Sheets, and Asset Loading

**Status:** Accepted **Date:** 2026-06-19 **Parent:** [ADR 001: Architecture](001-architecture.md)

## Context

`SoftwareBackend` (ADR 007) renders glyphs using 1-bit bitmap fonts (`BitmapFont`). ADR 008 adds
layers and sub-cell offsets. This ADR adds retro-style `.png` sprite sheets (CP437 or custom
Unicode-mapped) that overlay or replace `BitmapFont` glyphs, including multi-cell sprites.

The existing `BitmapFont` covers 256-glyph embedded bitmaps. Tilesets extend this to arbitrary
`u32::MAX` codepoints backed by PNG pixel data.

**This ADR is `SoftwareBackend`-only.** `CrosstermBackend` and `HeadlessBackend` have no visual
representation for tilesets; they fall back to the glyph's Unicode codepoint as a character.

---

## `alpha-blend` dependency

All sprite compositing uses the `alpha-blend` crate. The integer `U8x4Rgba::source_over` is the
primary blend path; `U8x4Rgba::from_rgb_u32` / `to_rgb_u32` bridge between the `0x00RRGGBB` pixel
buffer and `U8x4Rgba`. See `.matan/alpha-blend.md` for the full audit.

```toml
alpha-blend = { version = "0.2", default-features = false, features = ["std"] }
```

## Decisions & Rust API Guidelines

1. **Typed configuration (C-BUILDER):** Unlike BearLibTerminal's stringly-typed

   `terminal_set("0xE000: tileset.png, size=16x16")`, tileset configuration uses a typed builder. No
   string parsing, no runtime format errors.

1. **PNG decoding via the `image` crate:** The `image` crate (with `png` feature only, to minimize

   compile-time cost) decodes PNG files into RGBA8 pixel data. Raw bytes (already decoded, e.g. from
   `include_bytes!`) are also accepted.

1. **Sprite cache alongside glyph cache:** Decoded sprites are stored in a `SpriteCache` keyed by

   `char`. When `draw_layers` (ADR 008) encounters a `Tile`, the backend checks `SpriteCache` first,
   falling back to `BitmapFont` if no sprite is registered for that codepoint. Each layer buffer
   holds one `Tile` per cell (no stacking), so the dispatch is one `sprite_cache.get(tile.glyph)`
   check per cell.

1. **CP437 and custom codepage mapping:** A `Codepage` enum maps sprite sheet row/column indices

   to Unicode codepoints. `Codepage::Cp437` follows the standard IBM CP437 → Unicode table.
   `Codepage::Unicode` treats row-major tile index as a direct Unicode scalar value starting at
   `start_codepoint`. `Codepage::Custom(&'static [char])` provides a caller-supplied mapping table.

1. **Multi-cell spacing:** A tileset can declare `spacing_cells_x` and `spacing_cells_y > 1` to

   indicate that each sprite occupies multiple grid cells. The anchor cell holds the tile; during
   blit, the sprite's pixels are drawn extending into adjacent cells. Adjacent cell entries in the
   sprite sheet define separate sprites, not the right half of the anchor. The anchor relationship
   is a rendering concept only — the grid still stores one `Tile` per cell.

1. **Codepoint collision handling:** If two tilesets map the same codepoint, the last-registered

   tileset wins. A `warn` log (via `log` crate, which has no runtime cost when the logger is a
   no-op) is emitted on collision.

1. **Error handling (C-GOOD-ERR):** A `TilesetError` type covers all failure modes: PNG decode

   failure, sprite-sheet dimensions that don't evenly divide by tile size, empty codepoint tables,
   unsupported pixel formats (only RGBA8 and RGB8 are accepted).

1. **Feature flag:** Everything in this ADR lives behind a `software-tilesets` feature flag that

   implies `software`. The `image` crate dependency is gated on this flag.

---

## Detailed Implementation Milestones

### M1: Tileset Configuration API

**Goal:**Define `TilesetOptions`, `Codepage`, and `TilesetError`. No PNG loading yet.**File:**
`src/backend/software/tileset.rs` (new)

```rust
use std::fmt;

/// Error type for tileset loading and validation.
#[derive(Debug)]
pub enum TilesetError {
    /// PNG decode failed. Contains the `image` crate error string.
    PngDecode(String),
    /// The image dimensions are not evenly divisible by the declared tile size.
    ///
    /// Contains `(image_width, image_height, tile_width, tile_height)`.
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
            Self::InvalidDimensions(iw, ih, tw, th) => write!(
                f,
                "image {iw}×{ih} is not divisible by tile size {tw}×{th}"
            ),
            Self::EmptyCodepage => write!(f, "codepage mapping has no entries"),
            Self::UnsupportedPixelFormat(fmt_name) => {
                write!(f, "unsupported pixel format: {fmt_name}; expected RGBA8 or RGB8")
            }
            Self::ZeroTileSize => write!(f, "tile_width and tile_height must be non-zero"),
            Self::ZeroSpacing => {
                write!(f, "spacing_cells_x and spacing_cells_y must be non-zero")
            }
        }
    }
}

impl std::error::Error for TilesetError {}

/// Maps row-major tile indices in a sprite sheet to Unicode codepoints.
#[derive(Debug, Clone)]
pub enum Codepage {
    /// Standard CP437 layout: the i-th tile maps to `CP437_TO_UNICODE[i]`.
    ///
    /// Only the first 256 tiles in the sheet are mapped; extras are ignored.
    Cp437,
    /// Starting at `start`, tile index i maps to `char::from_u32(start as u32 + i)`.
    ///
    /// Tiles that would map to a surrogate or exceed `char::MAX` are skipped.
    Unicode { start: char },
    /// Explicit mapping: tile i maps to `table[i]`.
    ///
    /// Tiles beyond `table.len()` are ignored.
    Custom(alloc::vec::Vec<char>),
}

impl Codepage {
    /// Returns the codepoint for tile index `i`, or `None` if out of range or
    /// invalid (surrogates, indices past `char::MAX`).
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

    /// Number of tiles this codepage defines, or `None` for Unicode (unbounded).
    pub fn len(&self) -> Option<usize> {
        match self {
            Self::Cp437 => Some(256),
            Self::Unicode { .. } => None,
            Self::Custom(t) => Some(t.len()),
        }
    }
}

/// Standard IBM CP437 to Unicode mapping, 256 entries.
pub const CP437_TO_UNICODE: [char; 256] = [
    '\u{0000}', '\u{263A}', '\u{263B}', '\u{2665}', '\u{2666}', '\u{2663}', '\u{2660}', '\u{2022}',
    '\u{25D8}', '\u{25CB}', '\u{25D9}', '\u{2642}', '\u{2640}', '\u{266A}', '\u{266B}', '\u{263C}',
    '\u{25BA}', '\u{25C4}', '\u{2195}', '\u{203C}', '\u{00B6}', '\u{00A7}', '\u{25AC}', '\u{21A8}',
    '\u{2191}', '\u{2193}', '\u{2192}', '\u{2190}', '\u{221F}', '\u{2194}', '\u{25B2}', '\u{25BC}',
    // ... (32 through 127: printable ASCII, identical to Unicode)
    ' ', '!', '"', '#', '$', '%', '&', '\'', '(', ')', '*', '+', ',', '-', '.', '/',
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', ':', ';', '<', '=', '>', '?',
    '@', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O',
    'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', '[', '\\', ']', '^', '_',
    '`', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o',
    'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '{', '|', '}', '~', '\u{2302}',
    // 128–255: high CP437 box-drawing, block elements, etc.
    '\u{00C7}', '\u{00FC}', '\u{00E9}', '\u{00E2}', '\u{00E4}', '\u{00E0}', '\u{00E5}', '\u{00E7}',
    '\u{00EA}', '\u{00EB}', '\u{00E8}', '\u{00EF}', '\u{00EE}', '\u{00EC}', '\u{00C4}', '\u{00C5}',
    '\u{00C9}', '\u{00E6}', '\u{00C6}', '\u{00F4}', '\u{00F6}', '\u{00F2}', '\u{00FB}', '\u{00F9}',
    '\u{00FF}', '\u{00D6}', '\u{00DC}', '\u{00A2}', '\u{00A3}', '\u{00A5}', '\u{20A7}', '\u{0192}',
    '\u{00E1}', '\u{00ED}', '\u{00F3}', '\u{00FA}', '\u{00F1}', '\u{00D1}', '\u{00AA}', '\u{00BA}',
    '\u{00BF}', '\u{2310}', '\u{00AC}', '\u{00BD}', '\u{00BC}', '\u{00A1}', '\u{00AB}', '\u{00BB}',
    '\u{2591}', '\u{2592}', '\u{2593}', '\u{2502}', '\u{2524}', '\u{2561}', '\u{2562}', '\u{2556}',
    '\u{2555}', '\u{2563}', '\u{2551}', '\u{2557}', '\u{255D}', '\u{255C}', '\u{255B}', '\u{2510}',
    '\u{2514}', '\u{2534}', '\u{252C}', '\u{251C}', '\u{2500}', '\u{253C}', '\u{255E}', '\u{255F}',
    '\u{255A}', '\u{2554}', '\u{2569}', '\u{2566}', '\u{2560}', '\u{2550}', '\u{256C}', '\u{2567}',
    '\u{2568}', '\u{2564}', '\u{2565}', '\u{2559}', '\u{2558}', '\u{2552}', '\u{2553}', '\u{256B}',
    '\u{256A}', '\u{2518}', '\u{250C}', '\u{2588}', '\u{2584}', '\u{258C}', '\u{2590}', '\u{2580}',
    '\u{03B1}', '\u{00DF}', '\u{0393}', '\u{03C0}', '\u{03A3}', '\u{03C3}', '\u{00B5}', '\u{03C4}',
    '\u{03A6}', '\u{0398}', '\u{03A9}', '\u{03B4}', '\u{221E}', '\u{03C6}', '\u{03B5}', '\u{2229}',
    '\u{2261}', '\u{00B1}', '\u{2265}', '\u{2264}', '\u{2320}', '\u{2321}', '\u{00F7}', '\u{2248}',
    '\u{00B0}', '\u{2219}', '\u{00B7}', '\u{221A}', '\u{207F}', '\u{00B2}', '\u{25A0}', '\u{00A0}',
];

/// Options for loading a single tileset (sprite sheet).
#[derive(Debug, Clone)]
pub struct TilesetOptions {
    /// Raw bytes of the PNG file. Use `include_bytes!` for embedded assets,
    /// or read from disk at runtime.
    pub bytes: alloc::vec::Vec<u8>,
    /// Width of a single tile in pixels.
    pub tile_width: u16,
    /// Height of a single tile in pixels.
    pub tile_height: u16,
    /// Number of tiles per row in the sprite sheet.
    ///
    /// If `None`, it is derived as `image_width / tile_width`. Providing an
    /// explicit value is useful for sheets with padding or non-square layouts.
    pub columns: Option<u16>,
    /// Codepoint mapping from tile index to Unicode character.
    pub codepage: Codepage,
    /// Number of grid cells this sprite spans horizontally.
    ///
    /// A value of 2 means a 32px-wide sprite on a 16px-wide cell grid occupies
    /// two adjacent cells. Must be ≥ 1.
    pub spacing_cells_x: u16,
    /// Number of grid cells this sprite spans vertically. Must be ≥ 1.
    pub spacing_cells_y: u16,
}

impl TilesetOptions {
    /// Starts building a tileset from raw PNG bytes.
    pub fn from_bytes(bytes: alloc::vec::Vec<u8>) -> TilesetBuilder {
        TilesetBuilder {
            bytes,
            tile_width: 0,
            tile_height: 0,
            columns: None,
            codepage: Codepage::Cp437,
            spacing_cells_x: 1,
            spacing_cells_y: 1,
        }
    }
}

/// Builder for [`TilesetOptions`].
pub struct TilesetBuilder {
    bytes: alloc::vec::Vec<u8>,
    tile_width: u16,
    tile_height: u16,
    columns: Option<u16>,
    codepage: Codepage,
    spacing_cells_x: u16,
    spacing_cells_y: u16,
}

impl TilesetBuilder {
    #[must_use]
    pub const fn tile_size(mut self, width: u16, height: u16) -> Self {
        self.tile_width = width;
        self.tile_height = height;
        self
    }

    #[must_use]
    pub const fn columns(mut self, cols: u16) -> Self {
        self.columns = Some(cols);
        self
    }

    #[must_use]
    pub fn codepage(mut self, codepage: Codepage) -> Self {
        self.codepage = codepage;
        self
    }

    /// Sets the codepoint of the first tile; subsequent tiles increment by 1.
    /// Shorthand for `codepage(Codepage::Unicode { start })`.
    #[must_use]
    pub fn start_codepoint(mut self, start: char) -> Self {
        self.codepage = Codepage::Unicode { start };
        self
    }

    /// Number of grid cells each sprite occupies (width × height).
    #[must_use]
    pub const fn spacing(mut self, x: u16, y: u16) -> Self {
        self.spacing_cells_x = x;
        self.spacing_cells_y = y;
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
        })
    }
}
```

### Acceptance criteria

- `TilesetBuilder::build()` with `tile_width = 0` returns `Err(TilesetError::ZeroTileSize)`.
- `TilesetBuilder::build()` with `spacing_cells_x = 0` returns `Err(TilesetError::ZeroSpacing)`.
- `TilesetBuilder::build()` with `Codepage::Custom(vec![])` returns
  `Err(TilesetError::EmptyCodepage)`.
- A valid builder produces `Ok(TilesetOptions)` with all fields set correctly.
- `Codepage::Cp437.codepoint(32)` returns `Some(' ')`.
- `Codepage::Cp437.codepoint(64)` returns `Some('@')`.
- `Codepage::Cp437.codepoint(176)` returns `Some('\u{2591}')` (light shade).
- `Codepage::Unicode { start: '\u{E000}' }.codepoint(5)` returns `Some('\u{E005}')`.
- `Codepage::Custom(vec!['A', 'B']).codepoint(1)` returns `Some('B')`.
- `Codepage::Custom(vec!['A']).codepoint(1)` returns `None`.

**Tests:**

```rust
#[test]
fn tileset_builder_rejects_zero_tile_size() {
    let opts = TilesetOptions::from_bytes(vec![])
        .tile_size(0, 16)
        .build();
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
    assert!(matches!(opts.codepage, Codepage::Unicode { start: '\u{E000}' }));
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
```

---

### M2: PNG Decoding and `SpriteCache`

**Goal:** Decode a `TilesetOptions` into a `SpriteCache`: a `HashMap<char, Sprite>` mapping
codepoints to decoded RGBA8 pixel slices.

**File:** `src/backend/software/sprite_cache.rs` (new)

```rust
use super::tileset::{Codepage, TilesetError, TilesetOptions};
use std::collections::HashMap;

/// A decoded, ready-to-blit sprite.
#[derive(Debug, Clone)]
pub struct Sprite {
    /// RGBA8 pixel data, row-major, `width * height * 4` bytes.
    pub pixels: alloc::vec::Vec<u8>,
    /// Pixel width of the sprite. May span `spacing_cells_x` grid cells.
    pub pixel_width: u32,
    /// Pixel height of the sprite. May span `spacing_cells_y` grid cells.
    pub pixel_height: u32,
    /// How many grid cells wide this sprite is.
    pub spacing_cells_x: u16,
    /// How many grid cells tall this sprite is.
    pub spacing_cells_y: u16,
}

/// Cache of decoded sprites, keyed by Unicode codepoint.
pub struct SpriteCache {
    sprites: HashMap<char, Sprite>,
}

impl SpriteCache {
    pub fn new() -> Self {
        Self { sprites: HashMap::new() }
    }

    /// Returns the sprite for `ch`, if registered.
    pub fn get(&self, ch: char) -> Option<&Sprite> {
        self.sprites.get(&ch)
    }

    /// Loads a tileset, decoding the PNG and inserting all sprites.
    ///
    /// On codepoint collision, the new sprite replaces the old one and a
    /// message is logged via `log::warn`.
    ///
    /// # Errors
    ///
    /// Returns `TilesetError` on PNG decode failure, unsupported pixel format,
    /// or dimension mismatch.
    pub fn load(&mut self, opts: &TilesetOptions) -> Result<(), TilesetError> {
        let img = image::load_from_memory(&opts.bytes)
            .map_err(|e| TilesetError::PngDecode(e.to_string()))?
            .into_rgba8();

        let img_w = img.width();
        let img_h = img.height();
        let tile_w = u32::from(opts.tile_width);
        let tile_h = u32::from(opts.tile_height);

        if tile_w == 0 || tile_h == 0 {
            return Err(TilesetError::ZeroTileSize);
        }
        if img_w % tile_w != 0 || img_h % tile_h != 0 {
            return Err(TilesetError::InvalidDimensions(
                img_w, img_h,
                opts.tile_width, opts.tile_height,
            ));
        }

        let columns = opts.columns
            .map(u32::from)
            .unwrap_or(img_w / tile_w);
        let rows = img_h / tile_h;
        let total_tiles = (columns * rows) as usize;

        let raw = img.as_raw();

        for tile_idx in 0..total_tiles {
            let codepoint = match opts.codepage.codepoint(tile_idx) {
                Some(ch) => ch,
                None => break, // Codepage exhausted.
            };

            let tile_col = (tile_idx as u32) % columns;
            let tile_row = (tile_idx as u32) / columns;

            // Extract pixel data for this tile.
            let px_x = tile_col * tile_w;
            let px_y = tile_row * tile_h;
            let mut pixels = alloc::vec![0u8; (tile_w * tile_h * 4) as usize];

            for row in 0..tile_h {
                let src_start = ((px_y + row) * img_w + px_x) as usize * 4;
                let dst_start = (row * tile_w) as usize * 4;
                pixels[dst_start..dst_start + (tile_w as usize * 4)]
                    .copy_from_slice(&raw[src_start..src_start + (tile_w as usize * 4)]);
            }

            let sprite = Sprite {
                pixels,
                pixel_width: tile_w,
                pixel_height: tile_h,
                spacing_cells_x: opts.spacing_cells_x,
                spacing_cells_y: opts.spacing_cells_y,
            };

            if self.sprites.insert(codepoint, sprite).is_some() {
                log::warn!(
                    "tileset codepoint collision: U+{:04X} '{}' overwritten",
                    codepoint as u32,
                    codepoint,
                );
            }
        }
        Ok(())
    }
}

impl Default for SpriteCache {
    fn default() -> Self {
        Self::new()
    }
}
```

### Image pixel format handling

`image::load_from_memory(...).into_rgba8()` converts any supported format (RGB8, indexed PNG,
grayscale) to RGBA8. This is the canonical `image` crate approach — no format dispatch needed. The
`UnsupportedPixelFormat` error variant is reserved for future direct-decode paths that bypass
`image`.

**Dimension validation:** Before extracting tiles:

- `img_w % tile_w == 0 && img_h % tile_h == 0` — if not, return `InvalidDimensions`.
- If `opts.columns` is `Some(n)` and `n * tile_w > img_w`, log a warning and clamp to

  `img_w / tile_w`.

### `Cargo.toml` additions

```toml
[features]
software-tilesets = ["software", "dep:image"]

[dependencies]
image = { version = "0.25", optional = true, default-features = false, features = ["png"] }
log  = { version = "0.4", optional = true }
```

### Registration on `SoftwareBackendBuilder`

```rust
impl SoftwareBackendBuilder {
    /// Registers a tileset. Multiple tilesets can be registered; they are
    /// all loaded when [`build`](Self::build) is called.
    ///
    /// Tilesets are loaded in registration order, so later registrations win
    /// on codepoint collision.
    #[cfg(feature = "software-tilesets")]
    pub fn tileset(mut self, opts: TilesetOptions) -> Self {
        self.options.tilesets.push(opts);
        self
    }
}
```

`SoftwareBackend` gains a `tilesets: Vec<TilesetOptions>` field (empty by default). During
`SoftwareBackend::new`, each tileset is loaded into an `Arc<SpriteCache>` shared between the game
thread and (if needed) the window thread.

### Acceptance criteria (2)

- Loading a valid 16×16 sprite sheet PNG with `Codepage::Cp437` populates the cache with up to 256

  entries. After loading, `cache.get('@')` returns `Some(sprite)` with
  `sprite.pixel_width == 16 && sprite.pixel_height == 16`.

- Loading a PNG where `img_width % tile_width != 0` returns `Err(TilesetError::InvalidDimensions)`.
- Loading an empty `bytes` vec returns `Err(TilesetError::PngDecode(...))`.
- Registering two tilesets that both map `'@'` results in the second sprite being returned by

  `cache.get('@')` (last-wins).

- `SpriteCache::load` with `Codepage::Custom` stops inserting sprites once the table is exhausted,

  even if more tiles remain in the sheet.

**Tests:**

```rust
// These tests use a programmatically generated PNG to avoid bundling test assets.

fn make_test_png(tile_w: u32, tile_h: u32, cols: u32, rows: u32) -> Vec<u8> {
    // Build an RGBA8 image with a distinct solid color per tile, encode as PNG.
    let img_w = tile_w * cols;
    let img_h = tile_h * rows;
    let mut pixels = vec![0u8; (img_w * img_h * 4) as usize];
    // Fill each tile quadrant with a unique color for assertion purposes.
    for row in 0..rows {
        for col in 0..cols {
            let r = ((col * 20) % 256) as u8;
            let g = ((row * 20) % 256) as u8;
            for py in 0..tile_h {
                for px in 0..tile_w {
                    let idx = ((row * tile_h + py) * img_w + col * tile_w + px) as usize * 4;
                    pixels[idx]     = r;
                    pixels[idx + 1] = g;
                    pixels[idx + 2] = 0;
                    pixels[idx + 3] = 255;
                }
            }
        }
    }
    let mut out = std::io::Cursor::new(Vec::new());
    let encoder = image::codecs::png::PngEncoder::new(&mut out);
    encoder.encode(&pixels, img_w, img_h, image::ExtendedColorType::Rgba8).unwrap();
    out.into_inner()
}

#[test]
fn sprite_cache_load_cp437_sheet() {
    let png = make_test_png(16, 16, 16, 16); // 256 tiles
    let opts = TilesetOptions::from_bytes(png)
        .tile_size(16, 16)
        .codepage(Codepage::Cp437)
        .build()
        .unwrap();
    let mut cache = SpriteCache::new();
    cache.load(&opts).unwrap();
    // '@' is CP437 index 64.
    let sprite = cache.get('@').expect("'@' must be in cache");
    assert_eq!(sprite.pixel_width, 16);
    assert_eq!(sprite.pixel_height, 16);
    assert_eq!(sprite.pixels.len(), 16 * 16 * 4);
}

#[test]
fn sprite_cache_rejects_bad_dimensions() {
    let png = make_test_png(17, 16, 1, 1); // 17px wide, tile_w=16 does not divide evenly
    let opts = TilesetOptions::from_bytes(png)
        .tile_size(16, 16)
        .build()
        .unwrap();
    let mut cache = SpriteCache::new();
    let err = cache.load(&opts).unwrap_err();
    assert!(matches!(err, TilesetError::InvalidDimensions(17, 16, 16, 16)));
}

#[test]
fn sprite_cache_load_empty_bytes_errors() {
    let opts = TilesetOptions::from_bytes(vec![])
        .tile_size(16, 16)
        .build()
        .unwrap();
    let mut cache = SpriteCache::new();
    assert!(matches!(cache.load(&opts), Err(TilesetError::PngDecode(_))));
}

#[test]
fn sprite_cache_last_registration_wins_on_collision() {
    let png1 = make_test_png(16, 16, 1, 1);
    let png2 = make_test_png(8, 8, 1, 1);
    let opts1 = TilesetOptions::from_bytes(png1)
        .tile_size(16, 16)
        .start_codepoint('A')
        .build()
        .unwrap();
    let opts2 = TilesetOptions::from_bytes(png2)
        .tile_size(8, 8)
        .start_codepoint('A')
        .build()
        .unwrap();
    let mut cache = SpriteCache::new();
    cache.load(&opts1).unwrap();
    cache.load(&opts2).unwrap();
    let sprite = cache.get('A').unwrap();
    assert_eq!(sprite.pixel_width, 8); // opts2 wins
}

#[test]
fn sprite_cache_custom_codepage_stops_at_table_end() {
    let png = make_test_png(16, 16, 4, 1); // 4 tiles
    let opts = TilesetOptions::from_bytes(png)
        .tile_size(16, 16)
        .codepage(Codepage::Custom(vec!['A', 'B'])) // only 2 entries
        .build()
        .unwrap();
    let mut cache = SpriteCache::new();
    cache.load(&opts).unwrap();
    assert!(cache.get('A').is_some());
    assert!(cache.get('B').is_some());
    assert!(cache.get('C').is_none()); // tile index 2 has no mapping
}
```

---

### M3: Blit Integration — Sprites in `SoftwareBackend`

**Goal:** During `draw_layers`, check `SpriteCache` before falling back to `BitmapFont`. Draw
multi-cell sprites correctly, blending each RGBA8 pixel into the buffer.

**Integration point:** `SoftwareBackend` gains an `Arc<SpriteCache>` field. `RenderContext` holds a
clone. During `draw_layers`, each `(layer_id, pos, &Tile)` pair is dispatched to `blit_cell` with
`Option<&SpriteCache>`.

### Sprite blitting function

```rust
use alpha_blend::rgba::U8x4Rgba;

/// Blit a decoded RGBA8 sprite into `buffer`.
///
/// The sprite's top-left corner is at pixel `(cell_px_x + tile.dx * scale,
/// cell_px_y + tile.dy * scale)`. If `spacing_cells > 1`, the sprite's pixels
/// extend beyond the anchor cell into adjacent cells.
///
/// Pixels outside `buffer` bounds are silently skipped.
fn blit_sprite(
    buffer: &mut [u32],
    buf_w: usize,
    buf_h: usize,
    cell_px_x: usize,
    cell_px_y: usize,
    tile: &Tile,
    sprite: &Sprite,
    scale: usize,
) {
    let origin_x = cell_px_x as i64 + i64::from(tile.dx) * scale as i64;
    let origin_y = cell_px_y as i64 + i64::from(tile.dy) * scale as i64;

    let src_w = sprite.pixel_width as usize;
    let src_h = sprite.pixel_height as usize;

    for src_y in 0..src_h {
        for src_x in 0..src_w {
            let src_idx = (src_y * src_w + src_x) * 4;
            let src_pixel = U8x4Rgba::new(
                sprite.pixels[src_idx],     // r
                sprite.pixels[src_idx + 1], // g
                sprite.pixels[src_idx + 2], // b
                sprite.pixels[src_idx + 3], // a
            );

            if src_pixel.is_transparent() {
                continue;
            }

            // Each source pixel maps to `scale × scale` destination pixels.
            for dy in 0..scale {
                let dst_y = origin_y + (src_y * scale + dy) as i64;
                if dst_y < 0 || dst_y as usize >= buf_h { continue; }
                let dst_y = dst_y as usize;

                for dx in 0..scale {
                    let dst_x = origin_x + (src_x * scale + dx) as i64;
                    if dst_x < 0 || dst_x as usize >= buf_w { continue; }
                    let dst_x = dst_x as usize;

                    let dst_idx = dst_y * buf_w + dst_x;
                    if dst_idx >= buffer.len() { continue; }

                    let dst_pixel = U8x4Rgba::from_rgb_u32(buffer[dst_idx]);
                    let blended = src_pixel.source_over(dst_pixel);
                    buffer[dst_idx] = blended.to_rgb_u32();
                }
            }
        }
    }
}
```

### Per-cell dispatch (called once per `(layer, pos, &Tile)` from `draw_layers`)

```rust
fn blit_cell(
    buffer: &mut [u32],
    buf_w: usize,
    buf_h: usize,
    pos: Pos,
    tile: &Tile,
    font: &Font,
    sprite_cache: Option<&SpriteCache>,
    cell_w: usize,
    cell_h: usize,
    scale: usize,
    is_layer_zero: bool,
) {
    let px_x = usize::from(pos.x) * cell_w;
    let px_y = usize::from(pos.y) * cell_h;

    if is_layer_zero {
        // Fill solid background for layer 0.
        fill_rect(buffer, buf_w, buf_h, px_x, px_y, cell_w, cell_h, tile.style.bg);
    }

    // Dispatch: sprite cache wins over bitmap font.
    if let Some(sprite) = sprite_cache.and_then(|c| c.get(tile.glyph)) {
        blit_sprite(buffer, buf_w, buf_h, px_x, px_y, tile, sprite, scale);
    } else {
        blit_tile(buffer, buf_w, px_x, px_y, tile, font, cell_w, cell_h, scale);
    }
}
```

### Multi-cell sprite invariants

- The game places a single `Tile` in the anchor cell `(x, y)`. The sprite's pixel data covers

  `spacing_cells_x * cell_w` × `spacing_cells_y * cell_h` pixels.

- The blit writes into the pixel buffer starting at `(px_x, px_y)` and extending right/down as

  needed. No other cells in the grid are modified; the pixel-buffer extension is a pure rendering
  side-effect.

- Cells adjacent to the anchor (right and below) should be set to a transparent/placeholder tile

  by the game code so they don't overwrite the sprite's pixels. This is application-level
  responsibility, not enforced by the library (same behavior as BearLibTerminal).

- If a multi-cell sprite exceeds `buf_w * buf_h` pixel bounds, pixels are clipped silently.

**`buf_h` plumbing:** The current `blit_cell` receives `buf_w` but not `buf_h`, relying on
`if idx < buffer.len()` for row-overflow protection. With diagonal out-of-bounds (large `dy`), a
pixel at row `buf_h + 1` has index `(buf_h + 1) * buf_w + x` which correctly exceeds `buffer.len()`.
The existing guard is sufficient; no `buf_h` argument is strictly required for correctness. However,
for the inner loop's row clipping optimization (avoid iterating pixels on out-of-bounds rows),
`buf_h = buffer.len() / buf_w` is computed once per blit call.

### Acceptance criteria (3)

- A cell containing `Tile { glyph: '@', .. }` where `'@'` is in `SpriteCache` uses sprite pixels,

  not bitmap font pixels, in the output buffer.

- A cell containing `Tile { glyph: 'A', .. }` where `'A'` is not in `SpriteCache` uses the

  `BitmapFont` glyph.

- A sprite with `spacing_cells_x = 2, spacing_cells_y = 2` (32×32 pixels on a 16×16 cell grid)

  placed at cell (1, 1) writes pixels into the buffer region covering cells (1,1), (2,1), (1,2),
  (2,2).

- A sprite at the last column of the grid with `spacing_cells_x = 2` does not panic; overflow

  pixels are clipped.

- A sprite pixel with `alpha = 0` does not modify the destination buffer pixel.
- A sprite pixel with `alpha = 255` replaces the destination buffer pixel (opaque).
- A sprite pixel with `alpha = 128` blends 50/50 with the existing pixel.

**Tests:**

```rust
use alpha_blend::rgba::U8x4Rgba;

#[test]
fn sprite_blit_opaque_pixel_overwrites_buffer() {
    let mut buf = vec![0x00FF0000u32; 4]; // Red background, 2×2
    let sprite = Sprite {
        pixels: vec![0, 255, 0, 255,  // RGBA: opaque green
                     0, 255, 0, 255,
                     0, 255, 0, 255,
                     0, 255, 0, 255],
        pixel_width: 2,
        pixel_height: 2,
        spacing_cells_x: 1,
        spacing_cells_y: 1,
    };
    let tile = Tile { glyph: 'X', ..Tile::default() };
    blit_sprite(&mut buf, 2, 2, 0, 0, &tile, &sprite, 1);
    assert_eq!(buf[0], 0x0000FF00); // Pure green.
}

#[test]
fn sprite_blit_transparent_pixel_preserves_buffer() {
    let mut buf = vec![0x00FF0000u32; 1]; // Red
    let sprite = Sprite {
        pixels: vec![0, 255, 0, 0], // RGBA: fully transparent
        pixel_width: 1,
        pixel_height: 1,
        spacing_cells_x: 1,
        spacing_cells_y: 1,
    };
    let tile = Tile { glyph: 'X', ..Tile::default() };
    blit_sprite(&mut buf, 1, 1, 0, 0, &tile, &sprite, 1);
    assert_eq!(buf[0], 0x00FF0000); // Unchanged.
}

#[test]
fn sprite_blit_alpha_blends_correctly() {
    use alpha_blend::rgba::F32x4Rgba;

    let mut buf = vec![0x00FF0000u32]; // Red bg (r=255, g=0, b=0)
    let sprite = Sprite {
        pixels: vec![0, 255, 0, 128], // Green at ~50% alpha
        pixel_width: 1,
        pixel_height: 1,
        spacing_cells_x: 1,
        spacing_cells_y: 1,
    };
    let tile = Tile { glyph: 'X', ..Tile::default() };
    blit_sprite(&mut buf, 1, 1, 0, 0, &tile, &sprite, 1);

    // Verify against the f32 reference (source_over integer path should match).
    let expected = U8x4Rgba::from(alpha_blend::BlendMode::SourceOver.apply(
        F32x4Rgba::from(U8x4Rgba::new(0, 255, 0, 128)),
        F32x4Rgba::from(U8x4Rgba::from_rgb_u32(0x00FF0000)),
    ));
    assert_eq!(buf[0], expected.to_rgb_u32());
}

#[test]
fn sprite_blit_multi_cell_extends_into_adjacent_region() {
    // 4×2 pixel buffer (buf_w=4, buf_h=2), 2×2 cell grid, cell_w=2, cell_h=1.
    // Place a 2-cell-wide sprite at cell (0,0): should write into columns 0–3.
    let mut buf = vec![0u32; 8]; // 4 wide × 2 tall
    let sprite = Sprite {
        pixels: vec![
            255, 0, 0, 255,  0, 255, 0, 255, // top row: red, green, red, green
            255, 0, 0, 255,  0, 255, 0, 255,  0, 255, 0, 255,  0, 255, 0, 255,
        ],
        pixel_width: 4,  // spans 2 cells of width 2
        pixel_height: 2,
        spacing_cells_x: 2,
        spacing_cells_y: 1,
    };
    let tile = Tile { glyph: 'X', ..Tile::default() };
    blit_sprite(&mut buf, 4, 2, 0, 0, &tile, &sprite, 1);
    assert_eq!((buf[0] >> 16) & 0xFF, 255); // col 0: red
    assert_eq!((buf[1] >>  8) & 0xFF, 255); // col 1: green
    assert_eq!((buf[2] >> 16) & 0xFF, 0);   // col 2: r channel of green = 0
}

#[test]
fn sprite_blit_clips_at_buffer_boundary() {
    // 2×2 buffer, sprite wider than buffer — should not panic.
    let mut buf = vec![0u32; 4];
    let sprite = Sprite {
        pixels: vec![255, 0, 0, 255; 16], // 4 pixels × 4 bytes, all opaque
        pixel_width: 4,
        pixel_height: 1,
        spacing_cells_x: 2,
        spacing_cells_y: 1,
    };
    let tile = Tile::default();
    blit_sprite(&mut buf, 2, 2, 0, 0, &tile, &sprite, 1);
    // No panic assertion is the point. Verify that some valid pixels were written.
    assert!(buf[0] != 0);
}
```

---

## `SoftwareBackend` Changes

```rust
pub struct SoftwareBackend {
    pub window_title: String,
    pub font: Option<BitmapFont>,
    pub cols: u16,
    pub rows: u16,
    pub scale: u8,
    #[cfg(feature = "software-tilesets")]
    pub tilesets: Vec<TilesetOptions>,  // new
}
```

During `SoftwareBackend::new`:

```rust
#[cfg(feature = "software-tilesets")]
let sprite_cache = {
    let mut cache = SpriteCache::new();
    for opts in &options.tilesets {
        cache.load(opts)?;  // Propagate TilesetError as SoftwareBackendError::Tileset(e).
    }
    Arc::new(cache)
};
```

`SoftwareBackendError` gains:

```rust
#[cfg(feature = "software-tilesets")]
Tileset(TilesetError),
```

---

## Migration Notes

- Games using only `BitmapFont` with no PNG sprites require no changes. The `software-tilesets`

  feature is off by default.

- `SoftwareBackendBuilder::tileset()` is available only when `software-tilesets` is enabled; gating

  it behind `#[cfg(feature = "software-tilesets")]` prevents accidental use.

- The `Codepage` enum is `non_exhaustive` to allow adding new variants (e.g., `Cp1252`) without

  a semver break. Match arms must include a `_` catch-all.

- `CP437_TO_UNICODE` is `pub` so game code can independently map CP437 indices to codepoints

  without loading a tileset (e.g., for map loading from CP437-encoded `.ron` files).
