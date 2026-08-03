//! CPU-side sprite atlas for the GL sprite/tileset path (issue #366).
//!
//! Sprites (decoded RGBA tiles from a [`SpriteCache`]) are packed one-per-layer into an RGBA
//! `TEXTURE_2D_ARRAY`, each at the top-left of a layer sized to the largest sprite. A `char` maps
//! to a flat slot (== array layer) plus the sprite's pixel size, which the sprite shader uses to
//! scale the quad (so a sprite larger than a cell spills into neighbours, like
//! `retroglyph-software`'s blit) and to sample only the sprite's sub-rect of its layer.
//!
//! Unlike the `R8` glyph atlas (one coverage byte, grid-packed 16x16 per layer), sprites are full
//! RGBA and variable-sized, so they get their own layer each and their own draw pass.

// `redundant_pub_crate` fires on `pub(crate)` items in this private module; the module boundary
// is intentional, so it's allowed crate-locally.
#![allow(clippy::redundant_pub_crate)]

use retroglyph_core::Tint;
use retroglyph_window::sprite_cache::{SpriteCache, SpriteTint};
use retroglyph_window::tileset::{SheetColor, SpriteAlign};
use std::collections::HashMap;

/// Everything the draw path needs about one `char`'s sprite: where it lives in the atlas, how
/// big it is, and how it sits inside the multi-cell box a span reserves for it.
#[derive(Clone, Copy)]
pub(crate) struct SpriteSlot {
    /// Sprite atlas array layer.
    pub layer: u16,
    /// Sprite size in unscaled pixels.
    pub w: u16,
    pub h: u16,
    /// Placement within a span's cell box.
    pub align: SpriteAlign,
    /// What the sheet this sprite came from declared its pixels to mean.
    pub color: SheetColor,
}

impl SpriteSlot {
    /// The offset, in unscaled pixels, from the anchor cell's top-left corner to this sprite's
    /// own top-left pixel, for a span of `span_w` x `span_h` cells of `glyph_w` x `glyph_h`.
    pub(crate) fn align_offset(
        self,
        span_w: u16,
        span_h: u16,
        glyph_w: u8,
        glyph_h: u8,
    ) -> (i16, i16) {
        self.align.offset_in_span(
            u32::from(self.w),
            u32::from(self.h),
            span_w,
            span_h,
            glyph_w,
            glyph_h,
        )
    }
}

/// One sprite instance for the sprite draw pass: which cell, which atlas layer, the sprite's pixel
/// size, the sub-cell offset, and how the sprite is recoloured. Matches the
/// `a_cell`/`a_layer`/`a_sprite`/`a_offset`/`a_mask`/`a_tint`/`a_tint_op` attributes in the
/// sprite vertex shader (see `shaders.rs`). `#[repr(C)]`, 24 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct SpriteInstance {
    /// Grid cell column and row of the sprite's top-left cell.
    pub col: u16,
    pub row: u16,
    /// Sprite atlas array layer.
    pub layer: u16,
    /// Sprite size in unscaled pixels (`a_sprite`).
    pub w: u16,
    pub h: u16,
    /// Sub-cell offset in unscaled pixels (`a_offset`).
    pub dx: i16,
    pub dy: i16,
    /// Sheet stage (`a_mask`): RGB multiply factor, with `a` = 255 when the sheet is a mask and
    /// 0 when it is art, so the shader can select without a branch.
    pub mask: [u8; 4],
    /// Cell stage (`a_tint`): RGB colour, with `a` carrying `Tint::Mix`'s amount.
    pub tint: [u8; 4],
    /// Which operation the cell stage is: 0 none, 1 multiply, 2 mix. Matches `Tint`'s variants.
    ///
    /// A `u16` rather than a `u8` so the struct lands on 24 bytes with no tail padding, which is
    /// the stride `renderer.rs` declares to the vertex array.
    pub tint_op: u16,
}

impl SpriteInstance {
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn new(
        col: u16,
        row: u16,
        layer: u16,
        w: u16,
        h: u16,
        dx: i16,
        dy: i16,
        recolour: SpriteTint,
    ) -> Self {
        let mask = match recolour.mask {
            Tint::Multiply { r, g, b } => [r, g, b, 255],
            _ => [255, 255, 255, 0],
        };
        let (tint, tint_op) = match recolour.tint {
            Tint::Multiply { r, g, b } => ([r, g, b, 255], 1),
            Tint::Mix { r, g, b, amount } => ([r, g, b, amount], 2),
            // `Tint::None`, and any operation added to that `#[non_exhaustive]` enum after this
            // renderer was written: dropped rather than guessed at, so the artwork renders as
            // authored instead of being recoloured by a misread payload.
            _ => ([0, 0, 0, 0], 0),
        };
        Self {
            col,
            row,
            layer,
            w,
            h,
            dx,
            dy,
            mask,
            tint,
            tint_op,
        }
    }
}

/// The decoded sprite set, packed into a single RGBA array-texture atlas (one sprite per layer).
pub(crate) struct SpriteSet {
    /// `char` -> array layer.
    slots: HashMap<char, u16>,
    /// Per-layer sprite pixel size `(w, h)`.
    sizes: Vec<(u16, u16)>,
    /// Per-layer sprite placement within a span's cell box.
    aligns: Vec<SpriteAlign>,
    /// Per-layer sheet colour mode, carried through so a mask sheet can be recoloured by `fg`.
    colors: Vec<SheetColor>,
    /// One layer's texture size in texels: the max sprite `(w, h)` across the set.
    tex_w: u32,
    tex_h: u32,
    /// Number of array layers (== sprite count).
    layers: u32,
    /// Row-major RGBA8 bytes, `layers * tex_h * tex_w * 4`. Each sprite sits at the top-left of its
    /// layer; the rest of the layer is transparent padding.
    rgba: Vec<u8>,
}

impl SpriteSet {
    /// Builds an atlas from every sprite in `cache`, or `None` if the cache is empty.
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn from_cache(cache: &SpriteCache) -> Option<Self> {
        if cache.is_empty() {
            return None;
        }
        let tex_w = cache
            .iter()
            .map(|(_, s)| s.pixel_width)
            .max()
            .unwrap_or(1)
            .max(1);
        let tex_h = cache
            .iter()
            .map(|(_, s)| s.pixel_height)
            .max()
            .unwrap_or(1)
            .max(1);
        let layers = cache.iter().len() as u32;

        let layer_texels = (tex_w * tex_h * 4) as usize;
        let mut rgba = vec![0u8; layer_texels * layers as usize];
        let mut slots = HashMap::new();
        let mut sizes = Vec::with_capacity(layers as usize);
        let mut aligns = Vec::with_capacity(layers as usize);
        let mut colors = Vec::with_capacity(layers as usize);

        for (layer, (ch, sprite)) in cache.iter().enumerate() {
            slots.insert(ch, layer as u16);
            sizes.push((sprite.pixel_width as u16, sprite.pixel_height as u16));
            aligns.push(sprite.align);
            colors.push(sprite.color);

            let base = layer * layer_texels;
            let src_row = (sprite.pixel_width * 4) as usize;
            let dst_row = (tex_w * 4) as usize;
            for row in 0..sprite.pixel_height as usize {
                let s = row * src_row;
                let d = base + row * dst_row;
                rgba[d..d + src_row].copy_from_slice(&sprite.pixels[s..s + src_row]);
            }
        }

        Some(Self {
            slots,
            sizes,
            aligns,
            colors,
            tex_w,
            tex_h,
            layers,
            rgba,
        })
    }

    /// The atlas slot for `ch`, if it has a sprite.
    pub(crate) fn slot(&self, ch: char) -> Option<SpriteSlot> {
        let layer = *self.slots.get(&ch)?;
        let (w, h) = self.sizes[layer as usize];
        Some(SpriteSlot {
            layer,
            w,
            h,
            align: self.aligns[layer as usize],
            color: self.colors[layer as usize],
        })
    }

    /// One layer's texture size in texels `(w, h)`.
    pub(crate) const fn tex_size(&self) -> (u32, u32) {
        (self.tex_w, self.tex_h)
    }

    /// Number of array layers.
    pub(crate) const fn layers(&self) -> u32 {
        self.layers
    }

    /// The packed RGBA8 atlas bytes.
    pub(crate) fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}

#[cfg(test)]
mod tests {
    use super::{SpriteInstance, SpriteTint};
    use retroglyph_core::Tint;

    /// The vertex-array setup in `renderer.rs` describes this struct to the GPU by hand: a byte
    /// stride and a per-attribute offset. Nothing checks that description against the struct, so
    /// a field added here without updating `SPRITE_STRIDE` would have every instance read from
    /// the wrong offset and render garbage, with no compile error.
    #[test]
    fn layout_matches_the_stride_the_vertex_array_declares() {
        assert_eq!(
            size_of::<SpriteInstance>(),
            crate::renderer::SPRITE_STRIDE as usize
        );
    }

    fn inst(recolour: SpriteTint) -> SpriteInstance {
        SpriteInstance::new(0, 0, 0, 8, 16, 0, 0, recolour)
    }

    #[test]
    fn an_art_sheet_encodes_an_inert_mask_stage() {
        let i = inst(SpriteTint::default());
        // `a_mask.a` of 0 is what makes the shader's `mix(vec3(1.0), v_mask.rgb, v_mask.a)`
        // resolve to white, i.e. multiply by one.
        assert_eq!(i.mask[3], 0, "art sheets must not engage the mask stage");
        assert_eq!(i.tint_op, 0);
    }

    #[test]
    fn a_mask_sheet_encodes_its_multiply_with_the_stage_enabled() {
        let recolour = SpriteTint {
            mask: Tint::multiply(10, 20, 30),
            tint: Tint::None,
        };
        assert_eq!(inst(recolour).mask, [10, 20, 30, 255]);
    }

    #[test]
    fn each_tint_op_gets_its_own_discriminant() {
        let mul = SpriteTint {
            mask: Tint::None,
            tint: Tint::multiply(1, 2, 3),
        };
        let mix = SpriteTint {
            mask: Tint::None,
            tint: Tint::mix(4, 5, 6, 7),
        };
        // The shader branches on these, so the numbers are a contract with `shaders.rs`.
        assert_eq!((inst(mul).tint_op, inst(mul).tint), (1, [1, 2, 3, 255]));
        assert_eq!((inst(mix).tint_op, inst(mix).tint), (2, [4, 5, 6, 7]));
    }

    #[test]
    fn an_unknown_tint_op_renders_the_artwork_as_authored() {
        // `Tint` is `#[non_exhaustive]`. A variant added after this renderer was written must
        // fall through to "no recolour" rather than being encoded as a misread payload.
        let none = SpriteTint {
            mask: Tint::None,
            tint: Tint::None,
        };
        assert_eq!(inst(none).tint_op, 0);
    }
}
