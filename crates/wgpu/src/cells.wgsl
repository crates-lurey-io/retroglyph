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

// Neither fragment stage uses `discard`. A cell that draws nothing writes transparent black
// instead, which source-over blending resolves to "leave the destination alone", so the result is
// identical while the shader stays free of a fragment-killing branch.
//
// That distinction is not a micro-optimization. `discard` makes a fragment's visibility unknowable
// until the fragment stage has run, so a tile-based deferred renderer (every mobile GPU, and Apple
// silicon) has to disable early depth/stencil rejection and hidden-surface removal for the whole
// draw. Encoding "draws nothing" as alpha 0 keeps every fragment's contribution decidable up front.

// Both stages emit *premultiplied* alpha, which is why their blend states use a `One` source
// factor rather than `SrcAlpha`: the multiply has already happened here. Keeping the two in step
// matters even though the alpha is only ever 0 or 1 today, where the two conventions happen to
// agree; a premultiplied color composited with a `SrcAlpha` factor would multiply twice the moment
// any fractional alpha appeared.

// Background pass: the cell's opaque background. A cell with no background (a transparent cell in a
// higher grid layer) contributes alpha 0, so the layer beneath survives untouched.
@fragment
fn fs_background(in: CellVarying) -> @location(0) vec4<f32> {
    let opaque = f32(in.flags & FLAG_HAS_BG);
    return vec4<f32>(in.bg.rgb * opaque, opaque);
}

// Glyph pass: the foreground with atlas coverage as alpha, blended over the backgrounds laid down
// by the pass above. An empty cell contributes coverage 0 everywhere, so it can't erase the layer
// beneath.
@fragment
fn fs_glyph(in: CellVarying) -> @location(0) vec4<f32> {
    let drawn = f32((in.flags & FLAG_HAS_GLYPH) >> 1u);
    let coverage = sample_atlas(in.slot, in.uv) * drawn;
    return vec4<f32>(in.fg.rgb * coverage, coverage);
}
