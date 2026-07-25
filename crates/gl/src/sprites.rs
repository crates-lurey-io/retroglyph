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

#![allow(clippy::redundant_pub_crate)]

use retroglyph_window::sprite_cache::SpriteCache;
use std::collections::HashMap;

/// One sprite instance for the sprite draw pass: which cell, which atlas layer, the sprite's pixel
/// size, and the sub-cell offset. Matches the `a_cell`/`a_layer`/`a_sprite`/`a_offset` attributes
/// in the sprite vertex shader (see `shaders.rs`). `#[repr(C)]`, 16 bytes.
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
    /// Pad to 16 bytes so the stride matches the shader's expectation.
    pub _pad: u16,
}

impl SpriteInstance {
    pub(crate) const fn new(
        col: u16,
        row: u16,
        layer: u16,
        w: u16,
        h: u16,
        dx: i16,
        dy: i16,
    ) -> Self {
        Self {
            col,
            row,
            layer,
            w,
            h,
            dx,
            dy,
            _pad: 0,
        }
    }
}

/// The decoded sprite set, packed into a single RGBA array-texture atlas (one sprite per layer).
pub(crate) struct SpriteSet {
    /// `char` -> array layer.
    slots: HashMap<char, u16>,
    /// Per-layer sprite pixel size `(w, h)`.
    sizes: Vec<(u16, u16)>,
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

        for (layer, (ch, sprite)) in cache.iter().enumerate() {
            slots.insert(ch, layer as u16);
            sizes.push((sprite.pixel_width as u16, sprite.pixel_height as u16));

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
            tex_w,
            tex_h,
            layers,
            rgba,
        })
    }

    /// The atlas layer + pixel size for `ch`, if it has a sprite.
    pub(crate) fn slot(&self, ch: char) -> Option<(u16, u16, u16)> {
        let layer = *self.slots.get(&ch)?;
        let (w, h) = self.sizes[layer as usize];
        Some((layer, w, h))
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
