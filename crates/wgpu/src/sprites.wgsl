// Sprite pipeline: one instanced quad per sprite cell, sampling an RGBA8 array-texture atlas.
//
// Prepended by `shaders::source` with `common.wgsl`. Unlike the glyph pipelines, sprite instances
// carry an explicit grid cell (sprite cells are sparse, so the instance index can't derive one)
// and a sprite pixel size, so the quad scales to the sprite. A sprite larger than a cell spills
// into its neighbours, exactly like `retroglyph-software`'s sprite blit.

// Per-sprite instance, matching `sprite_set::SpriteInstance`'s `#[repr(C)]` layout and the
// `VertexBufferLayout` in `renderer::sprite_layout`.
struct SpriteInput {
    // `Uint16x2`: grid (column, row) of the sprite's top-left cell.
    @location(0) cell: vec2<u32>,
    // `Uint16x2`: sprite size in unscaled pixels (may exceed one cell).
    @location(1) size: vec2<u32>,
    // `Sint16x2`: sub-cell (dx, dy) in unscaled pixels, span alignment already folded in.
    @location(2) offset: vec2<i32>,
    // `Uint16x2`: x = sprite atlas array layer, y = the cell tint's operation (see `tint_op`).
    @location(3) layer_op: vec2<u32>,
    // `Uint8x4`: the sheet stage, raw 0..255. rgb is a multiply factor, a is 255 for a mask sheet
    // and 0 for an art sheet.
    @location(4) mask: vec4<u32>,
    // `Uint8x4`: the cell stage, raw 0..255. rgb is the tint colour, a carries `Tint::Mix`'s
    // amount.
    @location(5) tint: vec4<u32>,
}

struct SpriteVarying {
    @builtin(position) clip: vec4<f32>,
    // UV within the sprite's own sub-rect of its atlas layer.
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) layer: u32,
    @location(2) @interpolate(flat) mask: vec4<u32>,
    @location(3) @interpolate(flat) tint: vec4<u32>,
    @location(4) @interpolate(flat) tint_op: u32,
}

@vertex
fn vs_sprite(
    @builtin(vertex_index) vertex_index: u32,
    sprite: SpriteInput,
) -> SpriteVarying {
    let corner = corner_of(vertex_index);
    let scale = u.cell / u.glyph;
    let origin = vec2<f32>(sprite.cell) * u.cell + vec2<f32>(sprite.offset) * scale;
    let px = origin + corner * (vec2<f32>(sprite.size) * scale);

    var out: SpriteVarying;
    out.clip = vec4<f32>(
        px.x / u.screen.x * 2.0 - 1.0,
        1.0 - px.y / u.screen.y * 2.0,
        0.0,
        1.0,
    );
    // Each sprite sits at the top-left of its layer; the rest of the layer is transparent padding,
    // so the quad's [0,1] corners map onto the sprite's own fraction of the layer.
    out.uv = corner * (vec2<f32>(sprite.size) / u.sprite_tex);
    out.layer = sprite.layer_op.x;
    out.tint_op = sprite.layer_op.y;
    out.mask = sprite.mask;
    out.tint = sprite.tint;
    return out;
}

// Mirrors `gem::channel::multiply_u8`: `(a * b + 127) / 255` in integer math.
fn multiply_u8(a: u32, b: u32) -> u32 {
    return (a * b + 127u) / 255u;
}

// Mirrors `gem::channel::mix_u8`. `b - a` can be negative, so this runs signed, matching the
// round-half-away-from-zero behaviour of the Rust reference.
fn mix_u8(a: i32, b: i32, t: i32) -> i32 {
    let delta = (b - a) * t;
    var rounded: i32;
    if delta >= 0 {
        rounded = (delta + 127) / 255;
    } else {
        rounded = (delta - 127) / 255;
    }
    return a + rounded;
}

// Samples the sprite, recolours it, and outputs straight-alpha RGBA. The caller enables
// source-over blending, so a sprite's transparent pixels let the layers below show through.
//
// The recolour is the two stages `retroglyph_window::sprite_cache::SpriteTint::apply` defines, in
// that order: the sheet's own treatment (a multiply by the cell's foreground for a mask sheet),
// then the cell's tint. Alpha is never touched by either, so which pixels are opaque, and
// therefore the blending above, is identical tinted or not. The math is a bit-exact translation of
// the two `u8` helpers `retroglyph_core::color::Tint::apply` calls rather than an approximation in
// normalized float: the CPU rasterizer calls those functions directly, and `headless`'s parity
// test renders the same sprite through both to keep them honest.
@fragment
fn fs_sprite(in: SpriteVarying) -> @location(0) vec4<f32> {
    let src = textureSample(atlas, atlas_sampler, in.uv, in.layer);
    // Reconstruct the exact 0..255 source channel. Exact for a nearest-filtered, non-mipmapped,
    // non-sRGB UNORM8 texture (which is what the sprite atlas is), since every representable u8
    // value round-trips through `/255.0` and back via `round()` with no rounding error.
    let src_u8 = vec3<u32>(round(src.rgb * 255.0));

    // Stage 1: the sheet's own treatment. `mask.a` is 255 for a mask sheet and 0 for art, so an
    // art sheet is left exactly as authored.
    var rgb = src_u8;
    if in.mask.a != 0u {
        rgb = vec3<u32>(
            multiply_u8(rgb.r, in.mask.r),
            multiply_u8(rgb.g, in.mask.g),
            multiply_u8(rgb.b, in.mask.b),
        );
    }

    // Stage 2: the cell's tint. Op 0 (including any `Tint` variant added after this renderer was
    // written) leaves the artwork alone rather than guessing at a misread payload.
    if in.tint_op == 1u {
        rgb = vec3<u32>(
            multiply_u8(rgb.r, in.tint.r),
            multiply_u8(rgb.g, in.tint.g),
            multiply_u8(rgb.b, in.tint.b),
        );
    } else if in.tint_op == 2u {
        let t = i32(in.tint.a);
        rgb = vec3<u32>(
            u32(mix_u8(i32(rgb.r), i32(in.tint.r), t)),
            u32(mix_u8(i32(rgb.g), i32(in.tint.g), t)),
            u32(mix_u8(i32(rgb.b), i32(in.tint.b), t)),
        );
    }

    return vec4<f32>(vec3<f32>(rgb) / 255.0, src.a);
}
