//! Glyph atlas layout: a grid-packed `R8` coverage `TEXTURE_2D_ARRAY` (issue #367).
//!
//! v1 uploaded one glyph per array layer, which capped a font at
//! `GL_MAX_ARRAY_TEXTURE_LAYERS` (256 on the GL 3.3 / GL ES 3.0 floor). This module instead packs a
//! fixed [`ATLAS_COLS`]x[`ATLAS_ROWS`] grid of glyphs into each layer, so `N` glyphs need only
//! `ceil(N / 256)` layers, lifting the cap to 65536 glyphs while staying within the 256-layer
//! minimum. A glyph is addressed by a flat *slot* index; the shader turns that back into a
//! `(layer, column, row)` sub-rect (see `shaders.rs`).
//!
//! This module owns the CPU-side byte layout ([`AtlasGeometry`] plus the initial coverage buffer
//! for the [`FontChain`]); the whole-texture GL upload lives in [`renderer`](crate::renderer),
//! which owns the `glow` context.
//!
//! A set bit / non-zero coverage sample becomes up to `0xFF`; the fragment shader samples with
//! `NEAREST` and blends foreground over background by coverage, so glyphs stay crisp at any integer
//! scale.

// `pub(crate)` on items in this private module is intentional (crate-internal API surface); the
// nursery `redundant_pub_crate` lint conflicts with keeping the module structure explicit.
#![allow(clippy::redundant_pub_crate)]

use retroglyph_window::font::{BitmapFont, FontChain};

/// Glyph columns packed into one array layer.
pub(crate) const ATLAS_COLS: u32 = 16;
/// Glyph rows packed into one array layer.
pub(crate) const ATLAS_ROWS: u32 = 16;
/// Glyph slots per array layer (`ATLAS_COLS * ATLAS_ROWS`).
pub(crate) const SLOTS_PER_LAYER: u32 = ATLAS_COLS * ATLAS_ROWS;

/// The number of atlas slots `font` occupies: its glyph count, capped at the 256 a `u8` glyph
/// index can address (see [`BitmapFont::rows`]). A font that declares more glyphs than that has no
/// way to name them, so the atlas doesn't reserve slots for them either.
pub(crate) fn addressable_glyphs(font: &BitmapFont) -> u32 {
    u32::from(font.glyph_count()).min(256)
}

/// The packing of glyph cells into a `TEXTURE_2D_ARRAY`: a fixed [`ATLAS_COLS`]x[`ATLAS_ROWS`] grid
/// of `cell_w`x`cell_h` glyph cells per layer, across `layers` layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AtlasGeometry {
    /// Glyph cell width in texels (one unscaled font pixel per texel).
    pub cell_w: u32,
    /// Glyph cell height in texels.
    pub cell_h: u32,
    /// Number of array layers.
    pub layers: u32,
}

impl AtlasGeometry {
    /// Geometry with enough layers to hold `capacity` glyph slots (at least one layer).
    pub(crate) const fn new(cell_w: u32, cell_h: u32, capacity: u32) -> Self {
        let layers = capacity.div_ceil(SLOTS_PER_LAYER);
        Self {
            cell_w,
            cell_h,
            layers: if layers == 0 { 1 } else { layers },
        }
    }

    /// One layer's texture width in texels.
    pub(crate) const fn tex_w(&self) -> u32 {
        self.cell_w * ATLAS_COLS
    }

    /// One layer's texture height in texels.
    pub(crate) const fn tex_h(&self) -> u32 {
        self.cell_h * ATLAS_ROWS
    }

    /// Maps a flat slot index to its `(layer, glyph_column, glyph_row)`.
    pub(crate) const fn locate(slot: u32) -> (u32, u32, u32) {
        let layer = slot / SLOTS_PER_LAYER;
        let within = slot % SLOTS_PER_LAYER;
        (layer, within % ATLAS_COLS, within / ATLAS_COLS)
    }
}

/// The CPU-side coverage buffer for the whole atlas, grid-packed per [`AtlasGeometry`].
pub(crate) struct AtlasData {
    /// The glyph packing.
    pub geometry: AtlasGeometry,
    /// Row-major coverage bytes, length `tex_w * tex_h * layers`. Texel `(x, y)` of layer `l` is at
    /// `((l * tex_h + y) * tex_w + x)`. Row 0 is the glyph's top row, matching the vertex shader's
    /// y-flip so `v_uv.y = 0` samples a glyph's top.
    pub coverage: Vec<u8>,
}

impl AtlasData {
    /// Builds a fully-populated, grid-packed atlas for every glyph of every font in `fonts`, one
    /// slot per glyph, so the static bitmap path needs no runtime rasterization.
    ///
    /// The fonts are laid out back to back in chain order, so a font's slots start at the sum of
    /// the glyph counts before it: the same base [`GlyphCache`](crate::glyphs::GlyphCache) adds to
    /// a resolved glyph's own index. Every font in the chain shares the atlas cell size, which
    /// `GlBackendBuilder::build` has already checked they agree on.
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn build(fonts: &FontChain<'static>, cell_size: (u32, u32)) -> Self {
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
                // lives in `retroglyph-window`'s font module, #164), so this stays
                // width-agnostic.
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

#[cfg(test)]
mod tests {
    use super::{ATLAS_COLS, AtlasData, AtlasGeometry, SLOTS_PER_LAYER};
    use retroglyph_window::font::FontChain;

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
        use retroglyph_window::font::BitmapFont;

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

    #[cfg(feature = "default-font")]
    #[test]
    fn unscii16_packs_into_one_layer() {
        use retroglyph_window::font::unscii16;
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
        use retroglyph_window::font::unscii16;
        let atlas = AtlasData::build(&FontChain::from(unscii16::FONT), (8, 16));
        let g = atlas.geometry;
        let tex_w = g.tex_w();

        let cell_covered = |slot: u32| -> (bool, bool) {
            let (_, ox, oy) = {
                let (l, c, r) = AtlasGeometry::locate(slot);
                (l, c * g.cell_w, r * g.cell_h)
            };
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
}
