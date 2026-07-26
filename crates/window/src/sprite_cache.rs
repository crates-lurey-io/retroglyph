//! Decoded sprite cache: PNG decoding, tile extraction, and runtime lookup.
//!
//! The [`SpriteCache`] is built from [`TilesetOptions`]
//! and provides O(1) lookup of decoded RGBA8 sprites by codepoint.

use crate::tileset::{SheetColor, SpriteAlign, TilesetError, TilesetOptions};
use alpha_blend::rgba::U8x4Rgba;
use retroglyph_core::dev_only;
use retroglyph_core::{Color, Tint};
use std::collections::{BTreeMap, BTreeSet};

/// A decoded, ready-to-blit sprite.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Sprite {
    /// RGBA8 pixel data, row-major, `pixel_width * pixel_height * 4` bytes.
    pub pixels: Vec<u8>,
    /// Pixel width of the sprite.
    pub pixel_width: u32,
    /// Pixel height of the sprite.
    pub pixel_height: u32,
    /// Where this sprite sits inside the multi-cell box a span reserves for it.
    pub align: SpriteAlign,
    /// What this sprite's own sheet declared its pixels to mean.
    ///
    /// Copied from the sheet at load time rather than looked up at draw time: a `SpriteCache` is
    /// a flat map keyed by codepoint, so a sprite loses track of which sheet it came from the
    /// moment it lands there. One byte per sprite keeps the sheet's declaration with the pixels
    /// it describes.
    pub color: SheetColor,
}

impl Sprite {
    /// Returns the offset, in unscaled pixels, from the top-left corner of a `span_w` x `span_h`
    /// cell box to where this sprite's own top-left pixel belongs, per [`align`](Self::align).
    ///
    /// `span_w`/`span_h` come from [`Tile::span`](retroglyph_core::Tile::span) and `glyph_w`/
    /// `glyph_h` are the unscaled cell size, so the box is `span_w * glyph_w` x
    /// `span_h * glyph_h` pixels. A zero cell size is treated as one pixel, leaving the sprite on
    /// its anchor rather than offsetting it by a meaningless amount.
    ///
    /// Returns `(0, 0)` whenever the art already fills its box, which is the common case, so a
    /// backend can add the result to a tile's
    /// [`dx`](retroglyph_core::Tile::dx)/[`dy`](retroglyph_core::Tile::dy) unconditionally.
    #[must_use]
    pub const fn align_offset(
        &self,
        span_w: u16,
        span_h: u16,
        glyph_w: u8,
        glyph_h: u8,
    ) -> (i16, i16) {
        // A zero cell size would make the box degenerate; treat it as one pixel so the sprite
        // stays pinned to its anchor rather than being offset by a nonsense amount.
        let box_w = span_w as u32 * if glyph_w == 0 { 1 } else { glyph_w as u32 };
        let box_h = span_h as u32 * if glyph_h == 0 { 1 } else { glyph_h as u32 };
        self.align
            .offset(self.pixel_width, self.pixel_height, box_w, box_h)
    }
}

/// Cache of decoded sprites, keyed by Unicode codepoint.
///
/// # Reload / hot-swap is not supported
///
/// [`load`](Self::load) is append-only: it decodes a tileset and merges its sprites into the
/// existing map, with later registrations winning on codepoint collision (see [`load`](Self::load)
/// docs). There is no `unload` or `clear`, and nothing observes or invalidates sprites already
/// handed out via [`get`](Self::get).
///
/// This is a deliberate scope decision, not an oversight: games generally don't hot-swap tilesets
/// at runtime, and a `SpriteCache` is only ever populated once, when a backend is built. If you
/// need to iterate on a sprite sheet (e.g. during dev-mode asset editing) or otherwise want a
/// tileset change to take effect, rebuild the whole renderer from a fresh backend configuration
/// rather than mutating an existing cache in place.
#[derive(Debug)]
pub struct SpriteCache {
    sprites: BTreeMap<char, Sprite>,
}

impl SpriteCache {
    /// Creates an empty sprite cache.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            sprites: BTreeMap::new(),
        }
    }

    /// Returns the sprite for `ch`, if registered.
    #[must_use]
    pub fn get(&self, ch: char) -> Option<&Sprite> {
        self.sprites.get(&ch)
    }

    /// Iterates every registered `(codepoint, sprite)` in codepoint order.
    ///
    /// Used by GPU backends to build a sprite atlas from the whole decoded set (the software
    /// backend only ever needs per-glyph [`get`](Self::get) at blit time).
    #[must_use]
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (char, &Sprite)> {
        self.sprites.iter().map(|(&ch, sprite)| (ch, sprite))
    }

    /// Whether any sprite is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sprites.is_empty()
    }

    /// Loads a tileset, decoding the PNG and inserting all sprites.
    ///
    /// On codepoint collision, the new sprite replaces the old one and a
    /// message is logged via `log::warn`.
    ///
    /// # Errors
    ///
    /// Returns [`TilesetError::PngDecode`] if the bytes are not a valid PNG,
    /// [`TilesetError::ZeroTileSize`] if `opts.tile_width` or `opts.tile_height`
    /// is 0, or [`TilesetError::InvalidDimensions`] if the decoded image
    /// dimensions are not evenly divisible by the tile size.
    #[allow(clippy::cast_possible_truncation, clippy::cast_lossless)]
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
                img_w,
                img_h,
                opts.tile_width,
                opts.tile_height,
            ));
        }

        let columns = opts.columns.map_or(img_w / tile_w, u32::from);
        let rows = img_h / tile_h;
        let total_tiles = (columns * rows) as usize;

        let raw = img.as_raw();

        for tile_idx in 0..total_tiles {
            let Some(codepoint) = opts.codepage.codepoint(tile_idx) else {
                break;
            };

            let tile_col = (tile_idx as u32) % columns;
            let tile_row = (tile_idx as u32) / columns;

            // Extract RGBA8 sub-image for this tile.
            let px_x = tile_col * tile_w;
            let px_y = tile_row * tile_h;
            let mut pixels = vec![0u8; (tile_w * tile_h * 4) as usize];

            for row in 0..tile_h {
                let src_start = ((px_y + row) * img_w + px_x) as usize * 4;
                let dst_start = (row * tile_w) as usize * 4;
                pixels[dst_start..dst_start + (tile_w as usize * 4)]
                    .copy_from_slice(&raw[src_start..src_start + (tile_w as usize * 4)]);
            }

            // Apply transparent colour key if set.
            if let Some((kr, kg, kb)) = opts.transparent_color {
                for px in pixels.chunks_exact_mut(4) {
                    if px[0] == kr && px[1] == kg && px[2] == kb {
                        px[3] = 0;
                    }
                }
            }

            let sprite = Sprite {
                pixels,
                pixel_width: tile_w,
                pixel_height: tile_h,
                align: opts.align,
                color: opts.color,
            };

            if self.sprites.insert(codepoint, sprite).is_some() {
                #[allow(clippy::cast_lossless)]
                let cp = codepoint as u32;
                log::warn!("tileset codepoint collision: U+{cp:04X} '{codepoint}' overwritten");
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

/// The complete recolouring one sprite goes through in one cell: the sheet's own treatment,
/// then the cell's tint.
///
/// Two stages rather than one because they do not always fold together. A [`SheetColor::Mask`]
/// sheet is a multiply by the cell's foreground, and a multiply composes with another multiply,
/// but not with a [`Tint::Mix`]: "colour this mask red, then flash it half-way to white" is two
/// operations and cannot be written as one.
///
/// Both pixel backends resolve through here, so a sprite recoloured on the software rasteriser
/// and the same sprite recoloured in the GL fragment shader cannot disagree. The GL side uploads
/// the two stages as instance attributes and mirrors [`apply`](Self::apply)'s order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpriteTint {
    /// The sheet's own treatment, applied first.
    ///
    /// [`Tint::Multiply`] by the cell's resolved foreground colour for a [`SheetColor::Mask`]
    /// sheet, [`Tint::None`] for [`SheetColor::Art`].
    pub mask: Tint,
    /// The cell's own tint, applied second.
    pub tint: Tint,
}

impl SpriteTint {
    /// Resolves what a sprite from a `sheet`-coloured tileset should look like in a cell with
    /// foreground `fg` and tint `tint`.
    ///
    /// Takes the sheet's declaration rather than the [`Sprite`] itself, because that is all the
    /// answer depends on: the GPU backend resolves against an atlas slot and never holds the
    /// pixels at draw time.
    ///
    /// `default_fg` is the palette fallback for [`Color::Default`], which has no reading as a
    /// modulation value on its own (see [`Tint`]); [`palette::DEFAULT_FG`](crate::palette) is
    /// what both backends pass.
    #[must_use]
    pub const fn resolve(
        sheet: SheetColor,
        fg: Color,
        tint: Tint,
        default_fg: (u8, u8, u8),
    ) -> Self {
        let mask = match sheet {
            SheetColor::Art => Tint::None,
            SheetColor::Mask => {
                let (r, g, b) = fg.resolve_rgb(default_fg);
                Tint::multiply(r, g, b)
            }
        };
        Self { mask, tint }
    }

    /// Whether this leaves every pixel exactly as authored, so a renderer can take its untinted
    /// path.
    #[must_use]
    pub const fn is_identity(&self) -> bool {
        self.mask.is_identity() && self.tint.is_identity()
    }

    /// Applies both stages to one straight-alpha RGB triple, sheet treatment first.
    #[must_use]
    pub const fn apply(&self, rgb: (u8, u8, u8)) -> (u8, u8, u8) {
        self.tint.apply(self.mask.apply(rgb))
    }
}

/// Warns, at most once per glyph, that `glyph`'s sprite is larger than one cell but was drawn
/// without a span reserving the cells it covers.
///
/// For backend implementors: both graphical backends call this from their sprite blit, so the
/// diagnostic and the fix it names are identical on each. Such a sprite still draws at its
/// natural size, but its pixels land in neighbouring cells that go on painting their own
/// background and glyph over it, which is a confusing thing to debug from the rendered output
/// alone.
///
/// `sprite` and `cell` are `(width, height)` in unscaled pixels; a sprite fitting within `cell`
/// on both axes is silent. `seen` is caller-owned state so a redraw loop reports each offending
/// glyph once rather than every frame; entries are only ever added.
///
/// Returns whether a warning was emitted, which is always `false` in a build that compiles
/// diagnostics out: the size comparison, the `seen` bookkeeping, and the message all sit inside
/// [`dev_only!`], so a release build does none of them. See
/// [`BuildMode`](retroglyph_core::BuildMode).
pub fn warn_sprite_needs_span(
    seen: &mut BTreeSet<char>,
    glyph: char,
    sprite: (u32, u32),
    cell: (u32, u32),
) -> bool {
    dev_only!({
        let ((w, h), (cell_w, cell_h)) = (sprite, cell);
        if w <= cell_w && h <= cell_h {
            return false;
        }
        if !seen.insert(glyph) {
            return false;
        }
        log::warn!(
            "sprite for {glyph:?} is {w}x{h}px, larger than the {cell_w}x{cell_h}px cell, but was \
             drawn without a span: neighbouring cells will paint over it. Reserve the cells it \
             covers with `Surface::put_span`."
        );
        return true;
    });
    false
}

/// Blends `src` over `dst` using the Porter-Duff `SRC_OVER` operator for
/// straight-alpha pixels: `out = src + dst * (1 - src.a)` for each channel,
/// including alpha.
///
/// Delegates to the `alpha-blend` crate's `BlendMode::SourceOver`, converting
/// through `F32x4Rgba` for the blend math.
#[inline]
#[must_use]
pub fn source_over(src: U8x4Rgba, dst: U8x4Rgba) -> U8x4Rgba {
    use alpha_blend::rgba::F32x4Rgba;
    use alpha_blend::{BlendMode, RgbaBlend};
    BlendMode::SourceOver
        .apply(F32x4Rgba::from(src), F32x4Rgba::from(dst))
        .into()
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tileset::{Codepage, SpriteAlign, TilesetOptions};
    use image::ImageEncoder;

    /// Build a programmatic RGBA8 PNG for testing.
    ///
    /// Each tile is filled with a unique color derived from its column/row
    /// position so that tests can verify tile extraction.
    #[allow(clippy::cast_possible_truncation)]
    fn make_test_png(tile_w: u32, tile_h: u32, cols: u32, rows: u32) -> Vec<u8> {
        let img_w = tile_w * cols;
        let img_h = tile_h * rows;
        let mut pixels = vec![0u8; (img_w * img_h * 4) as usize];

        for row in 0..rows {
            for col in 0..cols {
                let r = ((col * 20) % 256) as u8;
                let g = ((row * 20) % 256) as u8;
                for py in 0..tile_h {
                    for px in 0..tile_w {
                        let idx = ((row * tile_h + py) * img_w + col * tile_w + px) as usize * 4;
                        pixels[idx] = r;
                        pixels[idx + 1] = g;
                        pixels[idx + 2] = 0;
                        pixels[idx + 3] = 255;
                    }
                }
            }
        }

        let mut out = std::io::Cursor::new(Vec::new());
        let encoder = image::codecs::png::PngEncoder::new(&mut out);
        encoder
            .write_image(&pixels, img_w, img_h, image::ExtendedColorType::Rgba8)
            .unwrap();
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
        let sprite = cache.get('@').expect("'@' must be in CP437 cache");
        assert_eq!(sprite.pixel_width, 16);
        assert_eq!(sprite.pixel_height, 16);
        assert_eq!(sprite.pixels.len(), 16 * 16 * 4);
    }

    #[test]
    fn sprite_cache_rejects_bad_dimensions() {
        let png = make_test_png(17, 16, 1, 1);
        let opts = TilesetOptions::from_bytes(png)
            .tile_size(16, 16)
            .build()
            .unwrap();
        let mut cache = SpriteCache::new();
        let err = cache.load(&opts).unwrap_err();
        assert!(matches!(
            err,
            TilesetError::InvalidDimensions(17, 16, 16, 16)
        ));
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
    fn sprite_cache_load_identity_codepage() {
        let png = make_test_png(16, 16, 4, 1); // 4 tiles: index 0..3
        let opts = TilesetOptions::from_bytes(png)
            .tile_size(16, 16)
            .codepage(Codepage::Identity)
            .build()
            .unwrap();
        let mut cache = SpriteCache::new();
        cache.load(&opts).unwrap();
        // Tile 0 -> char '\0', tile 1 -> '\x01', etc.
        assert!(cache.get('\0').is_some());
        assert!(cache.get('\x01').is_some());
        assert!(cache.get('\x03').is_some());
        assert!(cache.get('\x04').is_none()); // only 4 tiles
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
        assert!(cache.get('C').is_none()); // tile index 2 unmapped
    }

    // ── Alignment inside a span's cell box ─────────────────────────────

    /// Loads a single-tile `tile_w` x `tile_h` sheet mapped to `'A'` with the given alignment.
    fn one_sprite(tile_w: u32, tile_h: u32, align: SpriteAlign) -> Sprite {
        let png = make_test_png(tile_w, tile_h, 1, 1);
        #[allow(clippy::cast_possible_truncation)]
        let opts = TilesetOptions::from_bytes(png)
            .tile_size(tile_w as u16, tile_h as u16)
            .codepage(Codepage::Custom(vec!['A']))
            .align(align)
            .build()
            .unwrap();
        let mut cache = SpriteCache::new();
        cache.load(&opts).unwrap();
        cache.get('A').unwrap().clone()
    }

    #[test]
    fn sprite_align_offset_centres_art_in_a_multi_cell_box() {
        // An 8x16 sprite in a 2x1 span of 8x16 cells: 8 pixels of horizontal slack, none vertical.
        let sprite = one_sprite(8, 16, SpriteAlign::Center);
        assert_eq!(sprite.align_offset(2, 1, 8, 16), (4, 0));
        assert_eq!(sprite.align_offset(2, 2, 8, 16), (4, 8));
    }

    #[test]
    fn sprite_align_offset_is_zero_when_the_art_fills_its_span() {
        // A 16x32 sprite in the 2x2 span of 8x16 cells it was drawn for.
        let sprite = one_sprite(16, 32, SpriteAlign::Center);
        assert_eq!(sprite.align_offset(2, 2, 8, 16), (0, 0));
    }

    #[test]
    fn sprite_align_offset_defaults_to_top_left() {
        let sprite = one_sprite(8, 16, SpriteAlign::TopLeft);
        assert_eq!(sprite.align_offset(4, 4, 8, 16), (0, 0));
    }

    #[test]
    fn sprite_align_offset_tolerates_a_zero_cell_size() {
        // A degenerate cell size must not offset the sprite off its anchor.
        let sprite = one_sprite(8, 16, SpriteAlign::Center);
        assert_eq!(sprite.align_offset(2, 2, 0, 0), (0, 0));
    }

    // ── source_over tests ────────────────────────────────────────────────

    #[test]
    fn source_over_opaque_overwrites_destination() {
        let src = U8x4Rgba::new(0, 255, 0, 255); // opaque green
        let dst = U8x4Rgba::new(255, 0, 0, 255); // opaque red
        let result = source_over(src, dst);
        assert_eq!(result, src);
    }

    #[test]
    fn source_over_transparent_preserves_destination() {
        let src = U8x4Rgba::TRANSPARENT;
        let dst = U8x4Rgba::new(255, 0, 0, 255);
        let result = source_over(src, dst);
        assert_eq!(result, dst);
    }

    #[test]
    fn source_over_half_alpha_blends() {
        // Green at 50% over red at 100%.
        let src = U8x4Rgba::new(0, 255, 0, 128);
        let dst = U8x4Rgba::new(255, 0, 0, 255);
        let result = source_over(src, dst);
        // Expected (using float reference):
        //   out.r = 0*0.5 + 255*0.5 = 127.5  -> 127
        //   out.g = 255*0.5 + 0*0.5 = 127.5  -> 127
        //   out.b = 0*0.5 + 0*0.5 = 0
        //   out.a = 128 + 255*(1-128/255) = 128 + 127 = 255
        // Porter-Duff SRC_OVER applied uniformly to all channels (including alpha):
        //   out.r = 0.0*128 + 255.0*127 ≈ 127   (0.498*255)
        //   out.g = 255.0*128 + 0.0*127 ≈ 128   (0.502*255)
        //   out.b = 0
        //   out.a = 128*128 + 255*127 ≈ 191      (0.750*255)
        assert_eq!(result.r, 127);
        assert_eq!(result.g, 128);
        assert_eq!(result.b, 0);
        assert_eq!(result.a, 191);
    }

    // ── SpriteTint resolution ─────────────────────────────────────────

    fn sprite_with(color: SheetColor) -> Sprite {
        Sprite {
            pixels: vec![255, 255, 255, 255],
            pixel_width: 1,
            pixel_height: 1,
            align: SpriteAlign::TopLeft,
            color,
        }
    }

    const DEFAULT_FG: (u8, u8, u8) = (0xD4, 0xD4, 0xD4);

    #[test]
    fn art_sheet_ignores_fg_entirely() {
        let art = sprite_with(SheetColor::Art);
        let resolved = SpriteTint::resolve(art.color, Color::RED, Tint::None, DEFAULT_FG);

        assert_eq!(resolved.mask, Tint::None);
        assert!(resolved.is_identity());
        // The whole point of #537: a full-colour sheet renders as authored, whatever fg says.
        assert_eq!(resolved.apply((10, 200, 30)), (10, 200, 30));
    }

    #[test]
    fn mask_sheet_takes_its_colour_from_fg() {
        let mask = sprite_with(SheetColor::Mask);
        let (r, g, b) = Color::RED.resolve_rgb(DEFAULT_FG);
        let resolved = SpriteTint::resolve(mask.color, Color::RED, Tint::None, DEFAULT_FG);

        assert_eq!(resolved.mask, Tint::multiply(r, g, b));
        // A white mask pixel takes the foreground exactly.
        assert_eq!(resolved.apply((255, 255, 255)), (r, g, b));
    }

    #[test]
    fn mask_sheet_shades_a_grey_pixel_proportionally() {
        let mask = sprite_with(SheetColor::Mask);
        let resolved = SpriteTint::resolve(
            mask.color,
            Color::Rgb {
                r: 200,
                g: 100,
                b: 50,
            },
            Tint::None,
            DEFAULT_FG,
        );

        // Half-grey artwork lands on a proportionally darker shade of the foreground, which is
        // how a libtcod/Dwarf Fortress style tileset is authored.
        let (r, _, _) = resolved.apply((128, 128, 128));
        assert!(r > 0 && r < 200, "expected a shade of the fg, got {r}");
    }

    #[test]
    fn mask_sheet_resolves_default_fg_through_the_palette() {
        let mask = sprite_with(SheetColor::Mask);
        let resolved = SpriteTint::resolve(mask.color, Color::Default, Tint::None, DEFAULT_FG);

        // `Color::Default` has no reading as a modulation value on its own, so it goes through
        // the palette rather than being treated as white.
        assert_eq!(resolved.mask, Tint::multiply(0xD4, 0xD4, 0xD4));
    }

    #[test]
    fn the_cell_tint_applies_on_top_of_an_art_sheet() {
        let art = sprite_with(SheetColor::Art);
        let resolved = SpriteTint::resolve(
            art.color,
            Color::RED,
            Tint::multiply(128, 128, 128),
            DEFAULT_FG,
        );

        assert!(!resolved.is_identity());
        assert_eq!(resolved.apply((200, 180, 60)), (100, 90, 30));
    }

    #[test]
    fn both_stages_apply_in_order_on_a_mask_sheet() {
        let mask = sprite_with(SheetColor::Mask);
        let flash = Tint::mix(255, 255, 255, 255);
        let resolved = SpriteTint::resolve(
            mask.color,
            Color::Rgb { r: 255, g: 0, b: 0 },
            flash,
            DEFAULT_FG,
        );

        // Mask first would give red; the flash then takes it all the way to white. The other
        // order would give red, which is why the order is part of the contract.
        assert_eq!(resolved.apply((255, 255, 255)), (255, 255, 255));
    }

    #[test]
    fn an_untouched_art_cell_is_identity_so_renderers_can_skip_the_work() {
        let art = sprite_with(SheetColor::Art);
        assert!(
            SpriteTint::resolve(art.color, Color::Default, Tint::None, DEFAULT_FG).is_identity()
        );
        // A mask sheet is never identity: its colour always comes from somewhere.
        let mask = sprite_with(SheetColor::Mask);
        assert!(
            !SpriteTint::resolve(mask.color, Color::Default, Tint::None, DEFAULT_FG).is_identity()
        );
    }

    // `warn_sprite_needs_span` reports only in a build that compiles diagnostics in, so every
    // expectation below is written against `DEV` rather than a literal. Under `cargo test` that
    // is `true`; the point of spelling it out is that a release-profile test run still passes.

    #[test]
    fn warn_sprite_needs_span_reports_an_oversized_sprite_once() {
        let mut seen = BTreeSet::new();
        assert_eq!(
            warn_sprite_needs_span(&mut seen, '@', (32, 32), (16, 16)),
            retroglyph_core::DEV
        );
        // Second call for the same glyph is silent even in a reporting build.
        assert!(!warn_sprite_needs_span(&mut seen, '@', (32, 32), (16, 16)));
    }

    #[test]
    fn warn_sprite_needs_span_is_silent_for_a_sprite_that_fits() {
        let mut seen = BTreeSet::new();
        assert!(!warn_sprite_needs_span(&mut seen, '@', (16, 16), (16, 16)));
        assert!(seen.is_empty());
    }

    #[test]
    fn warn_sprite_needs_span_touches_no_state_outside_a_reporting_build() {
        let mut seen = BTreeSet::new();
        warn_sprite_needs_span(&mut seen, '@', (32, 32), (16, 16));
        // The dedup set is the allocation a release build should not be paying for.
        assert_eq!(seen.is_empty(), !retroglyph_core::DEV);
    }
}
