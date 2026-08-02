//! Glyph source + slot map: maps characters to grid-packed atlas slots.
//!
//! The backend renders a static [`FontChain`]: every glyph of every font in the chain is uploaded
//! once at init, and a character maps to a flat *slot* -- the font's base offset in the atlas plus
//! that font's own glyph index. Slots are packed into a `TEXTURE_2D_ARRAY` in a fixed `NxM` grid
//! per layer (issue #367's grid-packing half), so a chain with more than 256 glyphs in total is no
//! longer capped by `GL_MAX_ARRAY_TEXTURE_LAYERS`. The bundled Unscii 16 font is 256 glyphs, so
//! alone it occupies a single layer; the slot -> (layer, cell) addressing that spans layers is
//! covered by the `atlas` module's unit tests.
//!
//! The renderer never sees characters: [`GlyphCache::resolve`] hands back a `u16` atlas slot that
//! goes straight into the instance buffer.

// `redundant_pub_crate` fires on `pub(crate)` items in this private module; the module boundary
// is intentional, so it's allowed crate-locally.
#![allow(clippy::redundant_pub_crate)]

use crate::atlas::{AtlasData, addressable_glyphs};
use retroglyph_window::font::FontChain;

/// Maps characters to grid-packed atlas slots for a static chain of bitmap fonts.
pub(crate) struct GlyphCache {
    fonts: FontChain<'static>,
    /// Flat atlas slot at which each font in the chain's glyphs start, indexed by the font's
    /// position in the chain ([`ResolvedGlyph::font_index`](retroglyph_window::font::ResolvedGlyph::font_index)).
    bases: Vec<u32>,
    /// Glyph cell size in unscaled pixels (equals the atlas cell size; drives on-screen geometry).
    cell_w: u32,
    cell_h: u32,
    /// The slot for the space glyph (used for blank base cells).
    space_slot: u16,
}

impl GlyphCache {
    /// A cache over a static font chain: the atlas is fully populated at init and a character's
    /// slot is its font's base plus that font's own glyph index.
    ///
    /// `glyph_size` is the cell size every font in `fonts` agrees on; `GlBackendBuilder::build`
    /// rejects a chain that doesn't have one, so this cannot pick a size some font overflows.
    pub(crate) fn bitmap(fonts: FontChain<'static>, glyph_size: (u8, u8)) -> Self {
        let mut bases = Vec::with_capacity(fonts.font_count());
        let mut next = 0;
        for font in fonts.fonts() {
            bases.push(next);
            next += addressable_glyphs(font);
        }
        let mut cache = Self {
            fonts,
            bases,
            cell_w: u32::from(glyph_size.0),
            cell_h: u32::from(glyph_size.1),
            space_slot: 0,
        };
        cache.space_slot = cache.resolve(' ').unwrap_or(0);
        cache
    }

    /// Glyph cell size in unscaled pixels.
    pub(crate) const fn cell_size(&self) -> (u32, u32) {
        (self.cell_w, self.cell_h)
    }

    /// The atlas slot of the space glyph, or `0` for a chain with no space glyph at all. Only ever
    /// used for cells that draw no glyph, so a chain without a space still renders correctly.
    pub(crate) const fn space_slot(&self) -> u16 {
        self.space_slot
    }

    /// The total number of glyph slots the chain occupies in the atlas.
    pub(crate) fn slot_count(&self) -> u32 {
        self.fonts.fonts().map(addressable_glyphs).sum()
    }

    /// The backing font chain (used by the Linux headless render test to compare against the
    /// chain's own coverage bits).
    #[cfg(all(test, target_os = "linux"))]
    pub(crate) const fn font_chain(&self) -> &FontChain<'static> {
        &self.fonts
    }

    /// The full-atlas coverage to upload once at resource build.
    pub(crate) fn initial_atlas(&self) -> AtlasData {
        AtlasData::build(&self.fonts, self.cell_size())
    }

    /// Resolves `ch` to its atlas slot, or `None` when no font in the chain can draw it (not even
    /// the substituted solid block), in which case the caller draws no glyph for that cell.
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn resolve(&self, ch: char) -> Option<u16> {
        let glyph = self.fonts.resolve(ch)?;
        // Slots are capped at 65536 by `GlBackendBuilder::build`, so the cast is lossless.
        Some((self.bases[glyph.font_index()] + u32::from(glyph.index())) as u16)
    }
}

#[cfg(test)]
mod tests {
    use super::GlyphCache;
    use retroglyph_window::font::{BitmapFont, FontChain};

    #[cfg(feature = "default-font")]
    #[test]
    fn bitmap_cache_maps_char_to_font_index() {
        use retroglyph_window::font::unscii16;
        let cache = GlyphCache::bitmap(FontChain::from(unscii16::FONT), (8, 16));
        assert_eq!(
            cache.resolve('A'),
            unscii16::FONT.glyph_index('A').map(u16::from)
        );
        assert_eq!(
            cache.space_slot(),
            unscii16::FONT.glyph_index(' ').map(u16::from).unwrap()
        );
    }

    /// Issue #539: a fallback font's glyphs live past the primary font's slots in the same atlas,
    /// so a character only that font declares resolves to its own slot instead of colliding with
    /// the primary's solid block.
    #[test]
    fn fallback_font_glyphs_get_slots_after_the_primary_font() {
        static PRIMARY_DATA: [u8; 256 * 16] = [0; 256 * 16];
        const PRIMARY: BitmapFont = BitmapFont::new(&PRIMARY_DATA, 8, 16, 256);

        static QUADRANT_DATA: [u8; 2 * 16] = [0; 2 * 16];
        const CHARSET: [(char, u8); 2] = [('▘', 0), ('▝', 1)];
        static FALLBACKS: [BitmapFont; 1] =
            [BitmapFont::with_charset(&QUADRANT_DATA, 8, 16, 2, &CHARSET)];

        let cache = GlyphCache::bitmap(FontChain::new(PRIMARY, &FALLBACKS), (8, 16));
        assert_eq!(cache.slot_count(), 258);
        assert_eq!(cache.resolve('A'), Some(u16::from(b'A')));
        assert_eq!(cache.resolve('▘'), Some(256));
        assert_eq!(cache.resolve('▝'), Some(257));
        // Still no coverage anywhere in the chain: the substituted solid block, not a quadrant.
        assert_eq!(cache.resolve('あ'), Some(0xDB));
    }
}
