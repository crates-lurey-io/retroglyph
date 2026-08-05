// Glyph pipeline: one instanced quad per grid cell, sampling a grid-packed R8 coverage atlas.
//
// Prepended by `shaders::source` with `common.wgsl`, which declares `Uniforms`, the `u` binding,
// the atlas bindings, and `corner_of`. See that module for why the two are concatenated rather
// than duplicated.
//
// The quad has no vertex buffer: `corner_of(vertex_index)` derives the four corners of a triangle
// strip, and `instance_index` (0..cols*rows, because each layer is bound as its own slice of the
// instance buffer) derives the cell's column and row. The only per-vertex input is the instance,
// so a cell costs 16 bytes of bandwidth and no index buffer at all.

// Per-cell instance, matching `instance::Cell`'s `#[repr(C)]` layout and the
// `VertexBufferLayout` in `renderer::cell_layout`.
struct CellInput {
    // `Uint16x2`: x = glyph atlas slot, y = compositing flags (see `instance::FLAG_*`).
    @location(0) glyph_flags: vec2<u32>,
    // `Unorm8x4`: foreground RGB in 0..1 (the fourth channel is padding).
    @location(1) fg: vec4<f32>,
    // `Unorm8x4`: background RGB in 0..1 (the fourth channel is padding).
    @location(2) bg: vec4<f32>,
    // `Sint16x2`: sub-cell (dx, dy) in unscaled font pixels.
    @location(3) offset: vec2<i32>,
}

struct CellVarying {
    @builtin(position) clip: vec4<f32>,
    // In-cell UV, equal to the quad corner: (0,0) is the cell's top-left.
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) slot: u32,
    @location(2) @interpolate(flat) flags: u32,
    @location(3) @interpolate(flat) fg: vec4<f32>,
    @location(4) @interpolate(flat) bg: vec4<f32>,
}

// Places a cell's quad, optionally shifted by its sub-cell offset.
//
// `offset_cells` is 0.0 for the background pass and 1.0 for the glyph pass, so a background stays
// pinned to its cell while a glyph is free to spill past the cell edge into its neighbours. That
// split is the two-pass mechanism of the sub-cell spill contract documented on
// `retroglyph_window::Presenter`; see `renderer::GpuResources::render` for why the passes must not
// be interleaved per cell.
fn place(vertex_index: u32, instance_index: u32, cell: CellInput, offset_scale: f32) -> CellVarying {
    let corner = corner_of(vertex_index);
    let col = f32(instance_index % u.cols);
    let row = f32(instance_index / u.cols);
    // dx/dy are in unscaled font pixels; `u.cell / u.glyph` is the integer render scale, so this
    // converts them to physical pixels.
    let shift = vec2<f32>(cell.offset) * (u.cell / u.glyph) * offset_scale;
    let px = (vec2<f32>(col, row) + corner) * u.cell + shift;

    var out: CellVarying;
    // Pixel space (y-down, origin top-left) -> clip space (y-up). Flipping y here means the atlas
    // can store a glyph's top row first and sample it with `uv.y = corner.y`.
    out.clip = vec4<f32>(
        px.x / u.screen.x * 2.0 - 1.0,
        1.0 - px.y / u.screen.y * 2.0,
        0.0,
        1.0,
    );
    out.uv = corner;
    out.slot = cell.glyph_flags.x;
    out.flags = cell.glyph_flags.y;
    out.fg = cell.fg;
    out.bg = cell.bg;
    return out;
}

@vertex
fn vs_background(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
    cell: CellInput,
) -> CellVarying {
    return place(vertex_index, instance_index, cell, 0.0);
}

@vertex
fn vs_glyph(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
    cell: CellInput,
) -> CellVarying {
    return place(vertex_index, instance_index, cell, 1.0);
}

// Background pass: the cell's opaque background, written with blending off. A cell with no
// background (a transparent cell in a higher grid layer) is discarded, so the layer beneath shows
// through.
@fragment
fn fs_background(in: CellVarying) -> @location(0) vec4<f32> {
    if (in.flags & FLAG_HAS_BG) == 0u {
        discard;
    }
    return vec4<f32>(in.bg.rgb, 1.0);
}

// Glyph pass: the foreground with atlas coverage as alpha, blended over the backgrounds laid down
// by the pass above. An empty cell (no glyph) is discarded so it can't erase the layer beneath.
@fragment
fn fs_glyph(in: CellVarying) -> @location(0) vec4<f32> {
    // `textureSample` requires uniform control flow, so the sample happens before the flag test
    // that may `discard`. The cost is one sample for a fragment that is about to be thrown away;
    // the alternative (`textureSampleLevel` after the discard) would trade that for an explicit
    // LOD on a texture that has exactly one mip level.
    let coverage = sample_atlas(in.slot, in.uv);
    if (in.flags & FLAG_HAS_GLYPH) == 0u {
        discard;
    }
    return vec4<f32>(in.fg.rgb, coverage);
}
