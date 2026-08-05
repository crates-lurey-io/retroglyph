//! Grid-packed glyph atlas layout shared by the GPU backends.
//!
//! A GPU backend uploads every glyph of a [`FontChain`] once and then addresses one by a flat
//! *slot* index that goes straight into an instance buffer. This module owns both halves of that:
//! [`AtlasData`] is the CPU-side coverage buffer to upload, and [`GlyphAtlas`] is the
//! `char` -> slot map to look up.
//!
//! # Why a grid, not one glyph per layer
//!
//! The obvious packing (one glyph per array-texture layer) caps a chain at the array-layer limit,
//! which is 256 on the OpenGL 3.3 / GL ES 3.0 floor and on wgpu's downlevel defaults. Packing a
//! fixed [`ATLAS_COLS`]x[`ATLAS_ROWS`] grid of glyphs into each layer instead means `N` glyphs need
//! only `ceil(N / 256)` layers, which lifts the cap to [`MAX_SLOTS`] glyphs while still fitting
//! inside that 256-layer minimum. A shader turns a slot back into its `(layer, column, row)`
//! sub-rect.
//!
//! # Coverage, not colour
//!
//! [`AtlasData::coverage`] is one byte per texel: `0xFF` where the glyph's bit is set and `0` where
//! it isn't, meant for a single-channel (`R8`) texture. A backend samples it with nearest filtering
//! and blends the cell's foreground over its background by that coverage, so glyphs stay crisp at
//! any integer scale and take the cell's colours like any other glyph.
//!
//! Those two values are the only ones that ever appear, and that is load-bearing rather than
//! incidental. Coverage used as an alpha is the usual place text rendering goes wrong on colour
//! space: a rasterizer's partial coverage is a linear quantity, so interpolating between an
//! sRGB-encoded foreground and background by it produces text that is too thin or too fat, and
//! correcting for that is fiddly. The question does not arise for a bitmap font, because a
//! blend factor of exactly 0 or exactly 1 selects one endpoint outright and every colour space
//! agrees on the result.
//!
//! Anything that introduces partial coverage (an antialiased or grayscale-AA font source,
//! multisampling, a non-integer render scale) reopens it.
//! This module's `coverage_is_strictly_binary` test fails if the first of those lands here.
//!
//! # Examples
//!
//! ```
//! # #[cfg(feature = "default-font")] {
//! use retroglyph_window::atlas::GlyphAtlas;
//! use retroglyph_window::font::{FontChain, unscii16};
//!
//! let atlas = GlyphAtlas::new(FontChain::from(unscii16::FONT), (8, 16));
//! // Unscii 16 is 256 CP437 glyphs, so it occupies exactly one 16x16 layer.
//! assert_eq!(atlas.slot_count(), 256);
//! assert_eq!(atlas.data().geometry.layers, 1);
//! // A character resolves to the slot its coverage was written to.
//! assert_eq!(atlas.resolve('A'), Some(u16::from(b'A')));
//! # }
//! ```

use crate::font::{BitmapFont, FontChain};

/// Glyph columns packed into one array layer.
pub const ATLAS_COLS: u32 = 16;

/// Glyph rows packed into one array layer.
pub const ATLAS_ROWS: u32 = 16;

/// Glyph slots per array layer (`ATLAS_COLS * ATLAS_ROWS`).
pub const SLOTS_PER_LAYER: u32 = ATLAS_COLS * ATLAS_ROWS;

/// The number of slots the atlas can address, set by the `u16` slot id an instance buffer carries.
///
/// A [`FontChain`] with more glyphs than this cannot be packed; a backend's builder is expected to
/// reject one, since [`GlyphAtlas::resolve`] has no slot to name them with.
pub const MAX_SLOTS: u32 = u16::MAX as u32 + 1;

/// The number of atlas slots `font` occupies: its glyph count, capped at the 256 a `u8` glyph
/// index can address (see [`BitmapFont::rows`]).
///
/// A font that declares more glyphs than that has no way to name them, so the atlas doesn't
/// reserve slots for them either.
#[must_use]
pub fn addressable_glyphs(font: &BitmapFont) -> u32 {
    u32::from(font.glyph_count()).min(256)
}

/// The packing of glyph cells into an array texture: a fixed [`ATLAS_COLS`]x[`ATLAS_ROWS`] grid of
/// `cell_w`x`cell_h` glyph cells per layer, across `layers` layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct AtlasGeometry {
    /// Glyph cell width in texels (one unscaled font pixel per texel).
    pub cell_w: u32,
    /// Glyph cell height in texels.
    pub cell_h: u32,
    /// Number of array layers.
    pub layers: u32,
}

impl AtlasGeometry {
    /// Geometry with enough layers to hold `capacity` glyph slots (at least one layer).
    #[must_use]
    pub const fn new(cell_w: u32, cell_h: u32, capacity: u32) -> Self {
        let layers = capacity.div_ceil(SLOTS_PER_LAYER);
        Self {
            cell_w,
            cell_h,
            layers: if layers == 0 { 1 } else { layers },
        }
    }

    /// One layer's texture width in texels.
    #[must_use]
    pub const fn tex_w(&self) -> u32 {
        self.cell_w * ATLAS_COLS
    }

    /// One layer's texture height in texels.
    #[must_use]
    pub const fn tex_h(&self) -> u32 {
        self.cell_h * ATLAS_ROWS
    }

    /// Maps a flat slot index to its `(layer, glyph_column, glyph_row)`.
    #[must_use]
    pub const fn locate(slot: u32) -> (u32, u32, u32) {
        let layer = slot / SLOTS_PER_LAYER;
        let within = slot % SLOTS_PER_LAYER;
        (layer, within % ATLAS_COLS, within / ATLAS_COLS)
    }
}

/// The CPU-side coverage buffer for a whole atlas, grid-packed per [`AtlasGeometry`].
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct AtlasData {
    /// The glyph packing.
    pub geometry: AtlasGeometry,
    /// Row-major coverage bytes, length `tex_w * tex_h * layers`. Texel `(x, y)` of layer `l` is at
    /// `((l * tex_h + y) * tex_w + x)`. Row 0 is the glyph's top row, so a shader that flips y when
    /// projecting to clip space samples a glyph's top at `uv.y == 0`.
    pub coverage: Vec<u8>,
}

impl AtlasData {
    /// Builds a fully-populated, grid-packed atlas for every glyph of every font in `fonts`, one
    /// slot per glyph, so a static bitmap font needs no runtime rasterization.
    ///
    /// The fonts are laid out back to back in chain order, so a font's slots start at the sum of
    /// the glyph counts before it, which is the same base [`GlyphAtlas::resolve`] adds to a
    /// resolved glyph's own index. Every font in the chain is assumed to share `cell_size`; a
    /// backend's builder checks that via [`FontChain::glyph_size`] before getting here.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn build(fonts: &FontChain<'static>, cell_size: (u32, u32)) -> Self {
        let (cell_w, cell_h) = cell_size;
        let count: u32 = fonts.fonts().map(addressable_glyphs).sum();
        let geometry = AtlasGeometry::new(cell_w, cell_h, count);

        let tex_w = geometry.tex_w();
        let tex_h = geometry.tex_h();
        let mut coverage = vec![0u8; (tex_w * tex_h * geometry.layers) as usize];

        let mut slot = 0;
        for font in fonts.fonts() {
            for index in 0..addressable_glyphs(font) {
                let (layer, gcol, grow) = AtlasGeometry::locate(slot);
                let (ox, oy) = (gcol * cell_w, grow * cell_h);
                // `glyph_pixels` yields each set pixel `(x, y)` decoded MSB-first (the bit order
                // lives in the font module, #164), so this stays width-agnostic.
                for (x, y) in font.glyph_pixels(index as u8) {
                    let px = ox + u32::from(x);
                    let py = oy + u32::from(y);
                    let idx = ((layer * tex_h + py) * tex_w + px) as usize;
                    coverage[idx] = 0xFF;
                }
                slot += 1;
            }
        }

        Self { geometry, coverage }
    }
}

/// A static [`FontChain`] plus the `char` -> slot map for its grid-packed atlas.
///
/// Every glyph of every font in the chain is uploaded once; a character maps to a flat slot, which
/// is the font's base offset in the atlas plus that font's own glyph index. A renderer never sees
/// characters past this point: [`resolve`](Self::resolve) hands back a `u16` that goes straight
/// into an instance buffer.
#[derive(Clone, Debug)]
pub struct GlyphAtlas {
    fonts: FontChain<'static>,
    /// Flat atlas slot at which each font in the chain's glyphs start, indexed by the font's
    /// position in the chain ([`ResolvedGlyph::font_index`](crate::font::ResolvedGlyph::font_index)).
    bases: Vec<u32>,
    cell_w: u32,
    cell_h: u32,
    space_slot: u16,
}

impl GlyphAtlas {
    /// An atlas over a static font chain, whose slots are assigned back to back in chain order.
    ///
    /// `glyph_size` is the cell size every font in `fonts` agrees on. Pass what
    /// [`FontChain::glyph_size`] returned, and reject a chain it returned `None` for: a grid has
    /// one cell size, so a chain whose fonts disagree has no atlas geometry to build.
    #[must_use]
    pub fn new(fonts: FontChain<'static>, glyph_size: (u8, u8)) -> Self {
        let mut bases = Vec::with_capacity(fonts.font_count());
        let mut next = 0;
        for font in fonts.fonts() {
            bases.push(next);
            next += addressable_glyphs(font);
        }
        let mut atlas = Self {
            fonts,
            bases,
            cell_w: u32::from(glyph_size.0),
            cell_h: u32::from(glyph_size.1),
            space_slot: 0,
        };
        atlas.space_slot = atlas.resolve(' ').unwrap_or(0);
        atlas
    }

    /// Glyph cell size in unscaled pixels.
    #[must_use]
    pub const fn cell_size(&self) -> (u32, u32) {
        (self.cell_w, self.cell_h)
    }

    /// The atlas slot of the space glyph, or `0` for a chain with no space glyph at all.
    ///
    /// Only ever used for cells that draw no glyph (a backend still needs *some* slot in the
    /// instance it writes), so a chain without a space still renders correctly.
    #[must_use]
    pub const fn space_slot(&self) -> u16 {
        self.space_slot
    }

    /// The total number of glyph slots the chain occupies in the atlas.
    ///
    /// Compare against [`MAX_SLOTS`] before building: a chain past that has glyphs
    /// [`resolve`](Self::resolve) cannot name.
    #[must_use]
    pub fn slot_count(&self) -> u32 {
        self.fonts.fonts().map(addressable_glyphs).sum()
    }

    /// The backing font chain.
    #[must_use]
    pub const fn fonts(&self) -> &FontChain<'static> {
        &self.fonts
    }

    /// The full coverage buffer to upload, built fresh each call.
    ///
    /// Not cached: a backend uploads it once at resource creation and, on some platforms, again
    /// after a lost device, which is rare enough that holding a second copy of the atlas for the
    /// renderer's whole life costs more than rebuilding it.
    #[must_use]
    pub fn data(&self) -> AtlasData {
        AtlasData::build(&self.fonts, self.cell_size())
    }

    /// Resolves `ch` to its atlas slot, or `None` when no font in the chain can draw it, not even
    /// as the substituted solid block, in which case the caller draws no glyph for that cell.
    ///
    /// # Panics
    ///
    /// Does not panic. The `u16` cast cannot truncate for a chain whose
    /// [`slot_count`](Self::slot_count) is within [`MAX_SLOTS`], which a backend's builder is
    /// expected to have checked.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn resolve(&self, ch: char) -> Option<u16> {
        let glyph = self.fonts.resolve(ch)?;
        Some((self.bases[glyph.font_index()] + u32::from(glyph.index())) as u16)
    }
}

#[cfg(test)]
mod tests {
    use super::{ATLAS_COLS, AtlasData, AtlasGeometry, GlyphAtlas, MAX_SLOTS, SLOTS_PER_LAYER};
    use crate::font::{BitmapFont, FontChain};

    #[test]
    fn geometry_layers_cover_capacity() {
        // 256 glyphs fit one 16x16 layer; 257 spill into a second.
        assert_eq!(AtlasGeometry::new(8, 16, 256).layers, 1);
        assert_eq!(AtlasGeometry::new(8, 16, 257).layers, 2);
        assert_eq!(AtlasGeometry::new(8, 16, 4096).layers, 16);
        // Never zero layers, even for an empty atlas.
        assert_eq!(AtlasGeometry::new(8, 16, 0).layers, 1);
    }

    #[test]
    fn a_full_slot_space_stays_within_the_256_layer_floor() {
        // The whole point of grid-packing: the largest addressable chain must still fit the
        // 256-layer minimum both GL ES 3.0 and wgpu's downlevel defaults guarantee.
        assert_eq!(AtlasGeometry::new(8, 16, MAX_SLOTS).layers, 256);
    }

    #[test]
    fn locate_walks_row_major_then_layer() {
        assert_eq!(AtlasGeometry::locate(0), (0, 0, 0));
        assert_eq!(AtlasGeometry::locate(1), (0, 1, 0));
        assert_eq!(AtlasGeometry::locate(ATLAS_COLS), (0, 0, 1));
        assert_eq!(AtlasGeometry::locate(SLOTS_PER_LAYER), (1, 0, 0));
        assert_eq!(AtlasGeometry::locate(SLOTS_PER_LAYER + 1), (1, 1, 0));
    }

    #[test]
    fn tex_dims_are_grid_times_cell() {
        let g = AtlasGeometry::new(8, 16, 256);
        assert_eq!(g.tex_w(), 8 * 16);
        assert_eq!(g.tex_h(), 16 * 16);
        assert_eq!(g.layers, 1);
    }

    /// Issue #539: every font in a chain is packed into the same atlas, back to back, so a
    /// fallback font's glyphs occupy the slots after the primary font's and carry their own
    /// coverage rather than the primary's.
    #[test]
    fn a_chain_packs_each_font_back_to_back() {
        // Primary: 256 blank glyphs. Fallback: one glyph with its top row fully set.
        static PRIMARY_DATA: [u8; 256 * 2] = [0; 256 * 2];
        const PRIMARY: BitmapFont = BitmapFont::new(&PRIMARY_DATA, 8, 2, 256);
        static FALLBACK_DATA: [u8; 2] = [0xFF, 0x00];
        const CHARSET: [(char, u8); 1] = [('▘', 0)];
        static FALLBACKS: [BitmapFont; 1] =
            [BitmapFont::with_charset(&FALLBACK_DATA, 8, 2, 1, &CHARSET)];

        let atlas = AtlasData::build(&FontChain::new(PRIMARY, &FALLBACKS), (8, 2));

        // 257 slots: the fallback font's only glyph is slot 256, the first cell of layer 1.
        assert_eq!(atlas.geometry.layers, 2);
        let (layer, gcol, grow) = AtlasGeometry::locate(256);
        assert_eq!((layer, gcol, grow), (1, 0, 0));

        let tex_w = atlas.geometry.tex_w();
        let tex_h = atlas.geometry.tex_h();
        let row0 = ((layer * tex_h) * tex_w) as usize;
        assert!(
            atlas.coverage[row0..row0 + 8].iter().all(|&c| c == 0xFF),
            "the fallback glyph's top row is covered at its own slot"
        );
        assert!(
            atlas.coverage[..row0].iter().all(|&c| c == 0),
            "the primary font's blank glyphs are untouched"
        );
    }

    /// Issue #539's other half: a character only a fallback font declares must resolve to its own
    /// slot instead of colliding with the primary's solid block.
    #[test]
    fn fallback_font_glyphs_get_slots_after_the_primary_font() {
        static PRIMARY_DATA: [u8; 256 * 16] = [0; 256 * 16];
        const PRIMARY: BitmapFont = BitmapFont::new(&PRIMARY_DATA, 8, 16, 256);

        static QUADRANT_DATA: [u8; 2 * 16] = [0; 2 * 16];
        const CHARSET: [(char, u8); 2] = [('▘', 0), ('▝', 1)];
        static FALLBACKS: [BitmapFont; 1] =
            [BitmapFont::with_charset(&QUADRANT_DATA, 8, 16, 2, &CHARSET)];

        let atlas = GlyphAtlas::new(FontChain::new(PRIMARY, &FALLBACKS), (8, 16));
        assert_eq!(atlas.slot_count(), 258);
        assert_eq!(atlas.resolve('A'), Some(u16::from(b'A')));
        assert_eq!(atlas.resolve('▘'), Some(256));
        assert_eq!(atlas.resolve('▝'), Some(257));
        // Still no coverage anywhere in the chain: the substituted solid block, not a quadrant.
        assert_eq!(atlas.resolve('あ'), Some(0xDB));
    }

    /// Every coverage byte is exactly `0x00` or `0xFF`, never anything between.
    ///
    /// This is what lets every backend treat coverage as a blend factor without worrying about
    /// colour space: an alpha of exactly 0 or 1 selects one endpoint outright, so sRGB-encoded and
    /// linear-light compositing agree bit for bit. Partial coverage would make that false and make
    /// gamma-correct text a real problem to solve, in three backends at once. A change that
    /// introduces it should fail here first and be a deliberate decision, not a silent regression.
    #[test]
    fn coverage_is_strictly_binary() {
        // A font with a half-set row, to prove the check reacts to bit patterns rather than
        // trivially passing on an all-zero atlas.
        static DATA: [u8; 4] = [0b1010_1010, 0x00, 0xFF, 0b0000_1111];
        const FONT: BitmapFont = BitmapFont::new(&DATA, 8, 4, 1);
        let atlas = AtlasData::build(&FontChain::from(FONT), (8, 4));
        assert!(
            atlas.coverage.contains(&0xFF),
            "the fixture should cover some texels, or this proves nothing"
        );
        for (index, &byte) in atlas.coverage.iter().enumerate() {
            assert!(
                byte == 0x00 || byte == 0xFF,
                "texel {index} has partial coverage ({byte:#04x}); see this module's docs on why \
                 that reopens the colour-space question for every backend"
            );
        }
    }

    #[cfg(feature = "default-font")]
    #[test]
    fn coverage_is_strictly_binary_for_the_bundled_font() {
        use crate::font::unscii16;
        let atlas = AtlasData::build(&FontChain::from(unscii16::FONT), (8, 16));
        assert!(
            atlas.coverage.iter().all(|&c| c == 0x00 || c == 0xFF),
            "the bundled font produced partial coverage"
        );
        assert!(atlas.coverage.contains(&0xFF));
    }

    #[cfg(feature = "default-font")]
    #[test]
    fn unscii16_packs_into_one_layer() {
        use crate::font::unscii16;
        let atlas = AtlasData::build(&FontChain::from(unscii16::FONT), (8, 16));
        assert_eq!(atlas.geometry.cell_w, 8);
        assert_eq!(atlas.geometry.cell_h, 16);
        assert_eq!(atlas.geometry.layers, 1);
        assert_eq!(
            atlas.coverage.len(),
            (atlas.geometry.tex_w() * atlas.geometry.tex_h()) as usize
        );
    }

    #[cfg(feature = "default-font")]
    #[test]
    fn space_is_blank_and_full_block_is_solid_in_their_cells() {
        use crate::font::unscii16;
        let atlas = AtlasData::build(&FontChain::from(unscii16::FONT), (8, 16));
        let g = atlas.geometry;
        let tex_w = g.tex_w();

        let cell_covered = |slot: u32| -> (bool, bool) {
            let (_, col, row) = AtlasGeometry::locate(slot);
            let (ox, oy) = (col * g.cell_w, row * g.cell_h);
            let mut any = false;
            let mut all = true;
            for y in 0..g.cell_h {
                for x in 0..g.cell_w {
                    let idx = (((oy + y) * tex_w) + ox + x) as usize;
                    let set = atlas.coverage[idx] != 0;
                    any |= set;
                    all &= set;
                }
            }
            (any, all)
        };

        // 0x20 space: entirely clear. 0xDB full block: entirely set.
        assert!(!cell_covered(0x20).0, "space must be blank");
        assert!(cell_covered(0xDB).1, "full block must be solid");
    }

    #[cfg(feature = "default-font")]
    #[test]
    fn atlas_maps_char_to_font_index() {
        use crate::font::unscii16;
        let atlas = GlyphAtlas::new(FontChain::from(unscii16::FONT), (8, 16));
        assert_eq!(
            atlas.resolve('A'),
            unscii16::FONT.glyph_index('A').map(u16::from)
        );
        assert_eq!(
            atlas.space_slot(),
            unscii16::FONT.glyph_index(' ').map(u16::from).unwrap()
        );
        assert_eq!(atlas.cell_size(), (8, 16));
    }
}
