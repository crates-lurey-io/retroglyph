//! CPU-side sprite atlas and instance for the tileset path.
//!
//! Sprites (decoded RGBA tiles from a [`SpriteCache`]) are packed one per array layer, each at the
//! top-left of a layer sized to the largest sprite in the set. A `char` maps to a [`SpriteSlot`]:
//! its layer plus the sprite's pixel size, which the sprite shader uses both to scale the quad (so
//! a sprite larger than a cell spills into its neighbours, exactly like `retroglyph-software`'s
//! blit) and to sample only the sprite's own sub-rect of its layer.
//!
//! One sprite per layer, rather than the grid packing the glyph atlas uses: glyphs are all one
//! size, so a grid wastes nothing, while sprites are variable-sized and full RGBA, so packing them
//! into a shared grid would either clip the large ones or pad the small ones to the largest
//! sprite's footprint anyway.

// `redundant_pub_crate` fires on `pub(crate)` items in this private module; the module boundary is
// intentional, so it's allowed crate-locally.
#![allow(clippy::redundant_pub_crate)]

use bytemuck::{Pod, Zeroable};
use retroglyph_core::color::Tint;
use retroglyph_window::sprite_cache::{SpriteCache, SpriteTint};
use retroglyph_window::tileset::{SheetColor, SpriteAlign};
use std::collections::HashMap;

/// Byte size of one [`SpriteInstance`], as a `usize` for slice and array arithmetic.
pub(crate) const SPRITE_INSTANCE_BYTES: usize = 24;

/// Byte stride of one [`SpriteInstance`], which is also the vertex buffer's `array_stride`.
pub(crate) const SPRITE_STRIDE: u64 = SPRITE_INSTANCE_BYTES as u64;

/// Everything the draw path needs about one `char`'s sprite: where it lives in the atlas, how big
/// it is, and how it sits inside the multi-cell box a span reserves for it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SpriteSlot {
    /// Sprite atlas array layer.
    pub layer: u16,
    /// Sprite width in unscaled pixels.
    pub w: u16,
    /// Sprite height in unscaled pixels.
    pub h: u16,
    /// Placement within a span's cell box.
    pub align: SpriteAlign,
    /// What the sheet this sprite came from declared its pixels to mean.
    pub color: SheetColor,
}

impl SpriteSlot {
    /// The offset, in unscaled pixels, from the anchor cell's top-left corner to this sprite's own
    /// top-left pixel, for a span of `span_w` x `span_h` cells of `glyph_w` x `glyph_h`.
    pub(crate) fn align_offset(
        self,
        span_w: u16,
        span_h: u16,
        glyph_w: u16,
        glyph_h: u16,
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

/// One sprite instance for the sprite pass, matching `sprites.wgsl`'s `SpriteInput` and the
/// `VertexBufferLayout` in [`renderer`](crate::renderer).
///
/// `#[repr(C)]`, 24 bytes, every attribute 4 bytes wide on a 4-byte offset. The `u16` pairs are
/// grouped so each becomes one `Uint16x2`/`Sint16x2` attribute rather than costing its own.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
pub(crate) struct SpriteInstance {
    /// Grid column of the sprite's top-left cell.
    pub col: u16,
    /// Grid row of the sprite's top-left cell.
    pub row: u16,
    /// Sprite width in unscaled pixels.
    pub w: u16,
    /// Sprite height in unscaled pixels.
    pub h: u16,
    /// Sub-cell x offset in unscaled pixels, with span alignment already folded in.
    pub dx: i16,
    /// Sub-cell y offset in unscaled pixels, with span alignment already folded in.
    pub dy: i16,
    /// Sprite atlas array layer.
    pub layer: u16,
    /// Which operation the cell's tint stage is: 0 none, 1 multiply, 2 mix. The shader branches on
    /// these, so the numbers are a contract with `sprites.wgsl`.
    pub tint_op: u16,
    /// Sheet stage: RGB multiply factor, with `a` = 255 when the sheet is a mask and 0 when it is
    /// art, so the shader can select without carrying a second flag.
    pub mask: [u8; 4],
    /// Cell stage: RGB color, with `a` carrying [`Tint::Mix`]'s amount.
    pub tint: [u8; 4],
}

impl SpriteInstance {
    /// A sprite drawn at `(col, row)`, offset by `(dx, dy)` unscaled pixels, recolored by
    /// `recolor`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn new(
        col: u16,
        row: u16,
        layer: u16,
        w: u16,
        h: u16,
        dx: i16,
        dy: i16,
        recolor: SpriteTint,
    ) -> Self {
        let mask = match recolor.mask {
            Tint::Multiply { r, g, b } => [r, g, b, 255],
            _ => [255, 255, 255, 0],
        };
        let (tint, tint_op) = match recolor.tint {
            Tint::Multiply { r, g, b } => ([r, g, b, 255], 1),
            Tint::Mix { r, g, b, amount } => ([r, g, b, amount], 2),
            // `Tint::None`, and any operation added to that `#[non_exhaustive]` enum after this
            // renderer was written: dropped rather than guessed at, so the artwork renders as
            // authored instead of being recolored by a misread payload.
            _ => ([0, 0, 0, 0], 0),
        };
        Self {
            col,
            row,
            w,
            h,
            dx,
            dy,
            layer,
            tint_op,
            mask,
            tint,
        }
    }
}

/// The decoded sprite set, packed into a single RGBA array-texture atlas, one sprite per layer.
pub(crate) struct SpriteSet {
    /// `char` -> array layer.
    slots: HashMap<char, u16>,
    /// Per-layer sprite pixel size `(w, h)`.
    sizes: Vec<(u16, u16)>,
    /// Per-layer sprite placement within a span's cell box.
    aligns: Vec<SpriteAlign>,
    /// Per-layer sheet color mode, carried through so a mask sheet can be recolored by `fg`.
    colors: Vec<SheetColor>,
    /// One layer's texture size in texels: the largest sprite `(w, h)` across the set.
    tex_w: u32,
    tex_h: u32,
    /// Number of array layers, which equals the sprite count.
    layers: u32,
    /// Row-major RGBA8 bytes, `layers * tex_h * tex_w * 4`. Each sprite sits at the top-left of its
    /// layer; the rest of the layer is transparent padding.
    rgba: Vec<u8>,
}

impl SpriteSet {
    /// Builds an atlas from every sprite in `cache`, or `None` if the cache is empty.
    #[must_use]
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
    use super::{SPRITE_INSTANCE_BYTES, SPRITE_STRIDE, SpriteInstance, SpriteTint};
    use core::mem::offset_of;
    use retroglyph_core::color::Tint;

    #[test]
    fn layout_matches_the_vertex_attribute_offsets() {
        // `renderer::sprite_layout` describes this struct to the GPU by hand. A field added or
        // reordered here without updating it would render garbage with no compile error.
        assert_eq!(size_of::<SpriteInstance>(), SPRITE_INSTANCE_BYTES);
        assert_eq!(SPRITE_STRIDE, SPRITE_INSTANCE_BYTES as u64);
        assert_eq!(offset_of!(SpriteInstance, col), 0);
        assert_eq!(offset_of!(SpriteInstance, w), 4);
        assert_eq!(offset_of!(SpriteInstance, dx), 8);
        assert_eq!(offset_of!(SpriteInstance, layer), 12);
        assert_eq!(offset_of!(SpriteInstance, mask), 16);
        assert_eq!(offset_of!(SpriteInstance, tint), 20);
    }

    fn inst(recolor: SpriteTint) -> SpriteInstance {
        SpriteInstance::new(0, 0, 0, 8, 16, 0, 0, recolor)
    }

    #[test]
    fn an_art_sheet_encodes_an_inert_mask_stage() {
        let i = inst(SpriteTint::default());
        // `mask.a == 0` is what makes the shader leave the source texel alone.
        assert_eq!(i.mask[3], 0, "art sheets must not engage the mask stage");
        assert_eq!(i.tint_op, 0);
    }

    #[test]
    fn a_mask_sheet_encodes_its_multiply_with_the_stage_enabled() {
        let recolor = SpriteTint {
            mask: Tint::multiply(10, 20, 30),
            tint: Tint::None,
        };
        assert_eq!(inst(recolor).mask, [10, 20, 30, 255]);
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
        assert_eq!((inst(mul).tint_op, inst(mul).tint), (1, [1, 2, 3, 255]));
        assert_eq!((inst(mix).tint_op, inst(mix).tint), (2, [4, 5, 6, 7]));
    }

    #[test]
    fn an_unknown_tint_op_renders_the_artwork_as_authored() {
        // `Tint` is `#[non_exhaustive]`. A variant added after this renderer was written must fall
        // through to "no recolor" rather than being encoded as a misread payload.
        let none = SpriteTint {
            mask: Tint::None,
            tint: Tint::None,
        };
        assert_eq!(inst(none).tint_op, 0);
    }
}
