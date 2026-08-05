// Declarations shared by every pipeline: the uniform block, the two texture bindings, and the
// quad-corner helper. Prepended to each shader body by `shaders::source`.

// Mirrors `renderer::Uniforms`, field for field. The Rust struct is the source of truth for the
// byte layout; `shaders::tests::uniform_block_matches_the_rust_struct` pins the size the two agree
// on.
struct Uniforms {
    // Surface size in physical pixels.
    screen: vec2<f32>,
    // Cell size in physical pixels (glyph size x integer scale).
    cell: vec2<f32>,
    // Glyph size in unscaled font pixels. `cell / glyph` is the integer render scale.
    glyph: vec2<f32>,
    // One sprite-atlas layer's size in texels, i.e. the largest sprite in the set. Zero when no
    // tileset is loaded. Declared unconditionally, and in both shader bodies, so the uniform block
    // has one layout regardless of the `tilesets` feature.
    sprite_tex: vec2<f32>,
    // Grid columns, to unpack a cell's instance index into (column, row).
    cols: u32,
    // Glyph columns and rows packed per glyph-atlas array layer.
    atlas_cols: u32,
    atlas_rows: u32,
    // Pads the block to 48 bytes so its size is a multiple of 16, as the uniform address space
    // requires.
    pad: u32,
}

@group(0) @binding(0) var<uniform> u: Uniforms;

// The active atlas: R8 coverage for the glyph pipelines, RGBA8 for the sprite pipeline. Both are
// bound through one bind-group layout (see `renderer::GpuResources::atlas_layout`), so the two
// atlases differ only in which bind group is set.
@group(1) @binding(0) var atlas: texture_2d_array<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;

// `instance::FLAG_HAS_BG`: paint this cell's opaque background.
const FLAG_HAS_BG: u32 = 1u;
// `instance::FLAG_HAS_GLYPH`: draw this cell's glyph.
const FLAG_HAS_GLYPH: u32 = 2u;

// The corner of a unit quad for `vertex_index`, in triangle-strip order: top-left, top-right,
// bottom-left, bottom-right. Doubles as the in-cell UV, which is why the atlas stores a glyph's
// top row first.
fn corner_of(vertex_index: u32) -> vec2<f32> {
    return vec2<f32>(f32(vertex_index & 1u), f32(vertex_index >> 1u));
}

// Samples the coverage of glyph `slot` at in-cell UV `uv`.
//
// `slot` is a flat index into the whole atlas; this unpacks it into the (layer, column, row)
// sub-rect it occupies in the grid-packed array texture (see the `atlas` module).
fn sample_atlas(slot: u32, uv: vec2<f32>) -> f32 {
    let per_layer = u.atlas_cols * u.atlas_rows;
    let layer = slot / per_layer;
    let within = slot % per_layer;
    let grid = vec2<f32>(f32(within % u.atlas_cols), f32(within / u.atlas_cols));
    let grid_size = vec2<f32>(f32(u.atlas_cols), f32(u.atlas_rows));
    return textureSample(atlas, atlas_sampler, (grid + uv) / grid_size, layer).r;
}
