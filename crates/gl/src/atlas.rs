//! Building the glyph atlas: a `TEXTURE_2D_ARRAY` of `R8` coverage, one layer per glyph.
//!
//! This module only builds the CPU-side coverage bytes from a [`BitmapFont`]; the GL upload lives
//! in [`renderer`](crate::renderer) (which owns the `glow` context). Keeping the byte-layout logic
//! here makes it unit-testable without a GPU.
//!
//! Each glyph becomes one array layer of `glyph_width x glyph_height` texels. A set bit in the
//! 1-bit font (MSB = leftmost pixel) becomes `0xFF` coverage; a clear bit becomes `0x00`. The
//! fragment shader samples this with `NEAREST` filtering and blends `mix(bg, fg, coverage)`, so a
//! 1-bit glyph stays crisp at any integer scale.

// `pub(crate)` on items in this private module is intentional (crate-internal API surface); the
// nursery `redundant_pub_crate` lint conflicts with keeping the module structure explicit.
#![allow(clippy::redundant_pub_crate)]

use retroglyph_window::font::BitmapFont;

/// CPU-side glyph atlas: the raw `R8` coverage bytes plus the dimensions needed to upload them as a
/// `TEXTURE_2D_ARRAY` and to index a glyph's layer in the shader.
pub(crate) struct AtlasData {
    /// Glyph width in texels (one array-layer column count).
    pub width: u32,
    /// Glyph height in texels (one array-layer row count).
    pub height: u32,
    /// Number of array layers (one per glyph).
    pub layers: u32,
    /// Row-major coverage bytes, length `layers * height * width`, each `0x00` or `0xFF`.
    ///
    /// Ordered layer-major then row-major then column-major: layer `l`'s texel `(x, y)` is at
    /// `((l * height + y) * width + x)`. Row 0 is the glyph's top row, matching the vertex
    /// shader's y-flip so `v_uv.y = 0` samples the glyph top.
    pub coverage: Vec<u8>,
}

impl AtlasData {
    /// Builds the atlas coverage buffer for every glyph in `font`.
    // Casts are bounded: a font never has more than 256 glyphs (u8 index) and glyph dimensions are
    // single-byte, so `layer`, `y`, and `x` all fit `u32` and `u8` without loss.
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn build(font: &BitmapFont) -> Self {
        let width = u32::from(font.glyph_width);
        let height = u32::from(font.glyph_height);
        let layers = u32::from(font.glyph_count());

        let mut coverage = vec![0u8; (layers * height * width) as usize];
        for layer in 0..layers {
            // `glyph_count()` guarantees every index in `0..layers` is a valid `rows()` argument.
            let rows = font.rows(layer as u8);
            for (y, &row_bits) in rows.iter().enumerate() {
                for x in 0..width {
                    // Bit 7 (MSB) is the leftmost pixel.
                    let set = (row_bits >> (7 - x)) & 1 == 1;
                    if set {
                        let idx = ((layer * height + y as u32) * width + x) as usize;
                        coverage[idx] = 0xFF;
                    }
                }
            }
        }

        Self {
            width,
            height,
            layers,
            coverage,
        }
    }
}

#[cfg(all(test, feature = "default-font"))]
mod tests {
    use super::AtlasData;
    use retroglyph_window::font::unscii16;

    #[test]
    fn atlas_dims_match_unscii16() {
        let atlas = AtlasData::build(&unscii16::FONT);
        assert_eq!(atlas.width, 8);
        assert_eq!(atlas.height, 16);
        assert_eq!(atlas.layers, 256);
        assert_eq!(atlas.coverage.len(), 256 * 16 * 8);
    }

    #[test]
    fn space_glyph_is_blank_and_solid_block_is_full() {
        let atlas = AtlasData::build(&unscii16::FONT);
        let layer_len = (atlas.width * atlas.height) as usize;

        // 0x20 (space) is entirely clear.
        let space = &atlas.coverage[0x20 * layer_len..0x21 * layer_len];
        assert!(space.iter().all(|&b| b == 0), "space glyph must be blank");

        // 0xDB (full block) is entirely set.
        let block = &atlas.coverage[0xDB * layer_len..0xDC * layer_len];
        assert!(
            block.iter().all(|&b| b == 0xFF),
            "solid-block glyph must be fully covered"
        );
    }
}
