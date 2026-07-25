//! Glyph source + slot map: maps characters to grid-packed atlas slots.
//!
//! The backend renders a static [`BitmapFont`]: every glyph is uploaded once at init and a
//! character maps to the font's own fixed glyph index, which doubles as its atlas slot. Slots are
//! packed into a `TEXTURE_2D_ARRAY` in a fixed `NxM` grid per layer (issue #367's grid-packing half),
//! so a font with more than 256 glyphs is no longer capped by `GL_MAX_ARRAY_TEXTURE_LAYERS`. The
//! bundled Unscii 16 font is 256 glyphs, so it occupies a single layer; the slot -> (layer, cell)
//! addressing that would span layers is covered by the `atlas` module's unit tests.
//!
//! The renderer never sees characters: [`GlyphCache::resolve`] hands back a `u16` atlas slot that
//! goes straight into the instance buffer.

#![allow(clippy::redundant_pub_crate)]

use crate::atlas::AtlasData;
use retroglyph_window::font::BitmapFont;

/// Maps characters to grid-packed atlas slots for a static bitmap font.
pub(crate) struct GlyphCache {
    font: BitmapFont,
    /// Glyph cell size in unscaled pixels (equals the atlas cell size; drives on-screen geometry).
    cell_w: u32,
    cell_h: u32,
    /// The slot for the space glyph (used for blank base cells).
    space_slot: u32,
}

impl GlyphCache {
    /// A cache over a static bitmap font: the atlas is fully populated at init and the map is the
    /// font's fixed glyph index.
    pub(crate) fn bitmap(font: BitmapFont) -> Self {
        let cell_w = u32::from(font.glyph_width);
        let cell_h = u32::from(font.glyph_height);
        let space_slot = u32::from(font.char_to_index(' '));
        Self {
            font,
            cell_w,
            cell_h,
            space_slot,
        }
    }

    /// Glyph cell size in unscaled pixels.
    pub(crate) const fn cell_size(&self) -> (u32, u32) {
        (self.cell_w, self.cell_h)
    }

    /// The atlas slot of the space glyph.
    pub(crate) const fn space_slot(&self) -> u32 {
        self.space_slot
    }

    /// The backing bitmap font (used by the Linux headless render test to compare against the
    /// font's own coverage bits).
    #[cfg(all(test, target_os = "linux"))]
    pub(crate) const fn bitmap_font(&self) -> &BitmapFont {
        &self.font
    }

    /// The full-atlas coverage to upload once at resource build.
    pub(crate) fn initial_atlas(&self) -> AtlasData {
        AtlasData::build(&self.font)
    }

    /// Resolves `ch` to its atlas slot (the font's fixed glyph index).
    pub(crate) fn resolve(&self, ch: char) -> u32 {
        u32::from(self.font.char_to_index(ch))
    }
}

#[cfg(test)]
mod tests {
    use super::GlyphCache;

    #[cfg(feature = "default-font")]
    #[test]
    fn bitmap_cache_maps_char_to_font_index() {
        use retroglyph_window::font::unscii16;
        let cache = GlyphCache::bitmap(unscii16::FONT);
        assert_eq!(
            cache.resolve('A'),
            u32::from(unscii16::FONT.char_to_index('A'))
        );
        assert_eq!(
            cache.space_slot(),
            u32::from(unscii16::FONT.char_to_index(' '))
        );
    }
}
