//! GL resources and the instanced draw call.
//!
//! [`GlResources`] owns the shader program, the shared quad geometry, the per-cell instance
//! buffer, the projection uniforms, and the glyph atlas texture. It is created once the GL context
//! exists (from [`Presenter::init_surface`](retroglyph_window::Presenter::init_surface)) and driven
//! once per frame: [`upload`](GlResources::upload) pushes changed cells, [`draw`](GlResources::draw)
//! issues a single `draw_elements_instanced`.

// `pub(crate)` items in this private module are the crate-internal renderer API, and the GL enum
// constants are `u32` that GL wants as `i32`; the nursery/pedantic lints for those conflict with
// idiomatic GL code, so they're allowed crate-locally here.
#![allow(clippy::redundant_pub_crate)]

use crate::atlas::{ATLAS_COLS, ATLAS_ROWS, AtlasData};
use crate::error::SurfaceError;
use crate::shaders::{GlslFlavor, Shader, source};
#[cfg(feature = "tilesets")]
use crate::sprites::{SpriteInstance, SpriteSet};
use glow::HasContext as _;

/// Per-cell instance data, tightly packed to 16 bytes and uploaded straight to the GPU.
///
/// `#[repr(C)]` with explicit padding so the field offsets match the vertex-attribute pointers in
/// [`GlResources::configure_instance_attribs`]: `glyph` at 0, `fg` at 4, `bg` at 8, `dx`/`dy` at
/// 12/14.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Instance {
    /// Atlas layer (glyph id) for this cell.
    pub glyph: u16,
    /// Compositing flags ([`FLAG_HAS_BG`] | [`FLAG_HAS_GLYPH`]). A cleared bit makes the matching
    /// pass `discard` this cell, so a transparent background or empty glyph in a higher layer lets
    /// the layer beneath show through -- the GPU form of `Grid::flatten_into`'s occlusion rule.
    pub flags: u8,
    _pad: u8,
    /// Foreground RGB (uploaded as normalized `u8`).
    pub fg: [u8; 3],
    _fg_pad: u8,
    /// Background RGB (uploaded as normalized `u8`).
    pub bg: [u8; 3],
    _bg_pad: u8,
    /// Sub-cell pixel offset from the cell's left edge, in unscaled font pixels (from
    /// [`Tile::dx`](retroglyph_core::tile::Tile::dx)). Shifts the glyph horizontally; negative is
    /// left.
    pub dx: i16,
    /// Sub-cell pixel offset from the cell's top edge, in unscaled font pixels (from
    /// [`Tile::dy`](retroglyph_core::tile::Tile::dy)). Shifts the glyph vertically; negative is up.
    pub dy: i16,
}

/// [`Instance::flags`] bit: paint this cell's opaque background. Cleared = transparent background
/// (the background pass `discard`s the cell, so a lower layer shows through).
pub(crate) const FLAG_HAS_BG: u8 = 1 << 0;
/// [`Instance::flags`] bit: draw this cell's glyph. Cleared = empty cell (the glyph pass `discard`s
/// it), so a higher layer's untouched cells don't erase the layer beneath.
pub(crate) const FLAG_HAS_GLYPH: u8 = 1 << 1;

impl Instance {
    /// A cell with the given glyph, colors, sub-cell pixel offset, and compositing flags.
    pub(crate) const fn new(
        glyph: u16,
        fg: [u8; 3],
        bg: [u8; 3],
        dx: i16,
        dy: i16,
        flags: u8,
    ) -> Self {
        Self {
            glyph,
            flags,
            _pad: 0,
            fg,
            _fg_pad: 0,
            bg,
            _bg_pad: 0,
            dx,
            dy,
        }
    }
}

/// Reinterprets a slice of [`Instance`] as raw bytes for `buffer_(sub_)data`.
///
/// `Instance` is `#[repr(C)]`, `Copy`, and contains only integer fields (no padding bytes that
/// carry meaning, no pointers), so viewing it as `&[u8]` is sound.
const fn instances_as_bytes(instances: &[Instance]) -> &[u8] {
    // SAFETY: `Instance` is `#[repr(C)]` plain-old-data; the byte view covers exactly
    // `size_of::<Instance>() * len` bytes owned by `instances`.
    unsafe { core::slice::from_raw_parts(instances.as_ptr().cast::<u8>(), size_of_val(instances)) }
}

/// Unit-quad corners in `[0, 1]` (also the in-cell glyph UV): top-left, top-right, bottom-left,
/// bottom-right.
#[rustfmt::skip]
const QUAD_CORNERS: [f32; 8] = [
    0.0, 0.0,
    1.0, 0.0,
    0.0, 1.0,
    1.0, 1.0,
];

/// Two triangles covering the quad.
const QUAD_INDICES: [u16; 6] = [0, 1, 2, 2, 1, 3];

/// Byte stride of one [`Instance`] in the instance buffer, as GL wants it (`i32`).
const INSTANCE_STRIDE: i32 = 16;
/// The same stride as a `usize`, for buffer-size arithmetic.
const INSTANCE_BYTES: usize = 16;

/// GL objects for the instanced cell renderer.
pub(crate) struct GlResources {
    program: glow::Program,
    vao: glow::VertexArray,
    quad_vbo: glow::Buffer,
    index_buffer: glow::Buffer,
    instance_vbo: glow::Buffer,
    atlas: glow::Texture,
    /// The RGBA sprite atlas + its own program/VAO/instances (issue #366); `None` unless a tileset
    /// was loaded.
    #[cfg(feature = "tilesets")]
    sprites: Option<SpriteGpu>,
    u_screen: Option<glow::UniformLocation>,
    u_cell: Option<glow::UniformLocation>,
    u_cols: Option<glow::UniformLocation>,
    u_atlas: Option<glow::UniformLocation>,
    u_glyph: Option<glow::UniformLocation>,
    u_draw_glyph: Option<glow::UniformLocation>,
    /// Number of instances the instance VBO is currently sized for (`cols * rows`).
    capacity: usize,
}

impl GlResources {
    /// Compiles the program, uploads the atlas, and allocates the instance buffer for
    /// `cell_count` cells.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError::Init`] if a shader fails to compile or the program fails to link.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    pub(crate) fn new(
        gl: &glow::Context,
        flavor: GlslFlavor,
        atlas: &AtlasData,
        cell_count: usize,
    ) -> Result<Self, SurfaceError> {
        unsafe {
            let program = build_program(gl, flavor)?;

            let vao = gl
                .create_vertex_array()
                .map_err(|e| SurfaceError::Init(format!("create VAO: {e}")))?;
            gl.bind_vertex_array(Some(vao));

            // Static quad geometry (attribute 0, divisor 0).
            let quad_vbo = gl
                .create_buffer()
                .map_err(|e| SurfaceError::Init(format!("create quad VBO: {e}")))?;
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(quad_vbo));
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                bytemuck_f32(&QUAD_CORNERS),
                glow::STATIC_DRAW,
            );
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 8, 0);
            gl.enable_vertex_attrib_array(0);

            // Index buffer.
            let index_buffer = gl
                .create_buffer()
                .map_err(|e| SurfaceError::Init(format!("create index buffer: {e}")))?;
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(index_buffer));
            gl.buffer_data_u8_slice(
                glow::ELEMENT_ARRAY_BUFFER,
                bytemuck_u16(&QUAD_INDICES),
                glow::STATIC_DRAW,
            );

            // Per-cell instance buffer (attributes 1..=3, divisor 1). Allocated now, filled by
            // `upload`.
            let instance_vbo = gl
                .create_buffer()
                .map_err(|e| SurfaceError::Init(format!("create instance VBO: {e}")))?;
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(instance_vbo));
            gl.buffer_data_size(
                glow::ARRAY_BUFFER,
                (cell_count * INSTANCE_BYTES) as i32,
                glow::DYNAMIC_DRAW,
            );
            Self::configure_instance_attribs(gl);

            gl.bind_vertex_array(None);

            let atlas_tex = upload_atlas(gl, atlas)?;

            let u_screen = gl.get_uniform_location(program, "u_screen");
            let u_cell = gl.get_uniform_location(program, "u_cell");
            let u_cols = gl.get_uniform_location(program, "u_cols");
            let u_atlas = gl.get_uniform_location(program, "u_atlas");
            let u_glyph = gl.get_uniform_location(program, "u_glyph");
            let u_draw_glyph = gl.get_uniform_location(program, "u_draw_glyph");

            // The atlas grid packing is fixed, so set its uniforms once here.
            let u_atlas_cols = gl.get_uniform_location(program, "u_atlas_cols");
            let u_atlas_rows = gl.get_uniform_location(program, "u_atlas_rows");
            gl.use_program(Some(program));
            gl.uniform_1_i32(u_atlas_cols.as_ref(), ATLAS_COLS as i32);
            gl.uniform_1_i32(u_atlas_rows.as_ref(), ATLAS_ROWS as i32);

            Ok(Self {
                program,
                vao,
                quad_vbo,
                index_buffer,
                instance_vbo,
                atlas: atlas_tex,
                #[cfg(feature = "tilesets")]
                sprites: None,
                u_screen,
                u_cell,
                u_cols,
                u_atlas,
                u_glyph,
                u_draw_glyph,
                capacity: cell_count,
            })
        }
    }

    /// Sets up the instance vertex attributes (1: glyph, 2: fg, 3: bg), each with divisor 1 so they
    /// advance per instance rather than per vertex. Assumes the instance VBO is bound.
    unsafe fn configure_instance_attribs(gl: &glow::Context) {
        unsafe {
            // glyph: one unsigned short, read as an integer attribute (`uint` in the shader).
            gl.vertex_attrib_pointer_i32(1, 1, glow::UNSIGNED_SHORT, INSTANCE_STRIDE, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_divisor(1, 1);
            // fg: three normalized unsigned bytes -> vec3 in 0..1.
            gl.vertex_attrib_pointer_f32(2, 3, glow::UNSIGNED_BYTE, true, INSTANCE_STRIDE, 4);
            gl.enable_vertex_attrib_array(2);
            gl.vertex_attrib_divisor(2, 1);
            // bg: three normalized unsigned bytes -> vec3 in 0..1.
            gl.vertex_attrib_pointer_f32(3, 3, glow::UNSIGNED_BYTE, true, INSTANCE_STRIDE, 8);
            gl.enable_vertex_attrib_array(3);
            gl.vertex_attrib_divisor(3, 1);
            // offset: two signed shorts (dx, dy) in unscaled font pixels, read as an integer
            // attribute (`ivec2` in the shader).
            gl.vertex_attrib_pointer_i32(4, 2, glow::SHORT, INSTANCE_STRIDE, 12);
            gl.enable_vertex_attrib_array(4);
            gl.vertex_attrib_divisor(4, 1);
            // flags: one unsigned byte at offset 2, read as an integer attribute (`uint`).
            gl.vertex_attrib_pointer_i32(5, 1, glow::UNSIGNED_BYTE, INSTANCE_STRIDE, 2);
            gl.enable_vertex_attrib_array(5);
            gl.vertex_attrib_divisor(5, 1);
        }
    }

    /// Reallocates the instance buffer for a new cell count (on grid resize). Marks the whole
    /// buffer for re-upload by the caller.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    pub(crate) fn resize_instances(&mut self, gl: &glow::Context, cell_count: usize) {
        unsafe {
            gl.bind_vertex_array(Some(self.vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.instance_vbo));
            gl.buffer_data_size(
                glow::ARRAY_BUFFER,
                (cell_count * INSTANCE_BYTES) as i32,
                glow::DYNAMIC_DRAW,
            );
            gl.bind_vertex_array(None);
        }
        self.capacity = cell_count;
    }

    /// Uploads the full instance array to the GPU.
    pub(crate) fn upload(&self, gl: &glow::Context, instances: &[Instance]) {
        unsafe {
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.instance_vbo));
            gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, instances_as_bytes(instances));
        }
    }

    /// Sets the glyph size (in unscaled font pixels) used to convert a cell's `dx`/`dy` pixel
    /// offset into a texture-space shift in the vertex shader. Constant for the renderer's
    /// lifetime, so this is called once at init rather than per frame.
    pub(crate) fn set_glyph_size(&self, gl: &glow::Context, glyph_w: f32, glyph_h: f32) {
        unsafe {
            gl.use_program(Some(self.program));
            gl.uniform_2_f32(self.u_glyph.as_ref(), glyph_w, glyph_h);
        }
    }

    /// Sets the GL viewport and the projection uniforms. Call once per frame before
    /// [`draw`](Self::draw) so the surface size, cell size, and column count always agree with the
    /// instance count, regardless of the order surface- and grid-resize events arrive in.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub(crate) fn set_projection(
        &self,
        gl: &glow::Context,
        screen_w: f32,
        screen_h: f32,
        cell_w: f32,
        cell_h: f32,
        cols: i32,
    ) {
        unsafe {
            gl.viewport(0, 0, screen_w as i32, screen_h as i32);
            gl.use_program(Some(self.program));
            gl.uniform_2_f32(self.u_screen.as_ref(), screen_w, screen_h);
            gl.uniform_2_f32(self.u_cell.as_ref(), cell_w, cell_h);
            gl.uniform_1_i32(self.u_cols.as_ref(), cols);
        }
        #[cfg(feature = "tilesets")]
        if let Some(sprites) = &self.sprites {
            sprites.set_projection(gl, screen_w, screen_h, cell_w, cell_h);
        }
    }

    /// Clears the framebuffer to opaque black. Call once per frame before compositing layers with
    /// [`draw_layer`](Self::draw_layer); the base layer's opaque backgrounds then paint over it.
    //
    // Takes `&self` for call-site symmetry with `draw_layer`/`upload` (all invoked as `res.*`);
    // the clear itself is framebuffer state, not resource state, so `self` is unused.
    #[allow(clippy::unused_self)]
    pub(crate) fn clear(&self, gl: &glow::Context) {
        unsafe {
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
        }
    }

    /// Draws one layer's currently-uploaded instances in two instanced passes, compositing over
    /// whatever is already in the framebuffer (so calling this per layer back-to-front composites
    /// the layers).
    ///
    /// Pass 1 fills each cell's opaque background at its unshifted origin; pass 2 draws the glyphs
    /// with their sub-cell `dx`/`dy` offset applied to the quad *position* and coverage as alpha,
    /// alpha-blended over the backgrounds. Splitting the passes is what lets an offset glyph spill
    /// past its cell edge into neighbors: a single interleaved pass would let a later cell's
    /// background overwrite an earlier neighbor's spill. This is the two-pass mechanism of the
    /// "Sub-cell offsets and spill" contract documented on [`Presenter`](retroglyph_window::Presenter),
    /// shared with `retroglyph-software`.
    ///
    /// A cell whose [`Instance::flags`] clears [`FLAG_HAS_BG`] / [`FLAG_HAS_GLYPH`] is `discard`ed
    /// by the matching pass, so a transparent background or empty glyph in a higher layer leaves
    /// the layer beneath visible.
    pub(crate) fn draw_layer(&self, gl: &glow::Context, cell_count: i32) {
        unsafe {
            gl.use_program(Some(self.program));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(self.atlas));
            gl.uniform_1_i32(self.u_atlas.as_ref(), 0);
            gl.bind_vertex_array(Some(self.vao));

            // Pass 1: opaque backgrounds, no offset, no blending (discarded where FLAG_HAS_BG is
            // clear, i.e. a transparent-background cell).
            gl.disable(glow::BLEND);
            gl.uniform_1_i32(self.u_draw_glyph.as_ref(), 0);
            gl.draw_elements_instanced(glow::TRIANGLES, 6, glow::UNSIGNED_SHORT, 0, cell_count);

            // Pass 2: offset glyphs, coverage as alpha, blended over the backgrounds (discarded
            // where FLAG_HAS_GLYPH is clear, i.e. an empty cell). The alpha factors are
            // `ZERO`/`ONE` -- `A = src.a * 0 + dst.a * 1`, so the destination alpha survives
            // untouched. Coverage must drive the color channels only: it is a glyph mask, not a
            // surface transparency, and letting it reach the alpha channel would punch every
            // non-glyph texel down to alpha 0. A WebGL2 canvas is composited by the page (winit
            // requests `alpha: true`), so those texels would then show the document background
            // instead of the cell background painted in pass 1.
            gl.enable(glow::BLEND);
            gl.blend_func_separate(
                glow::SRC_ALPHA,
                glow::ONE_MINUS_SRC_ALPHA,
                glow::ZERO,
                glow::ONE,
            );
            gl.uniform_1_i32(self.u_draw_glyph.as_ref(), 1);
            gl.draw_elements_instanced(glow::TRIANGLES, 6, glow::UNSIGNED_SHORT, 0, cell_count);
            gl.disable(glow::BLEND);

            gl.bind_vertex_array(None);
        }
    }

    /// The instance-buffer capacity in cells.
    pub(crate) const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Deletes every GL object. Call before dropping the context.
    pub(crate) fn delete(&self, gl: &glow::Context) {
        unsafe {
            gl.delete_program(self.program);
            gl.delete_vertex_array(self.vao);
            gl.delete_buffer(self.quad_vbo);
            gl.delete_buffer(self.index_buffer);
            gl.delete_buffer(self.instance_vbo);
            gl.delete_texture(self.atlas);
        }
        #[cfg(feature = "tilesets")]
        if let Some(sprites) = &self.sprites {
            sprites.delete(gl);
        }
    }

    /// Sets the glyph size on the sprite program too (issue #366), so a sprite cell's `dx`/`dy`
    /// pixel offset scales correctly. No-op without a sprite atlas.
    #[cfg(feature = "tilesets")]
    pub(crate) fn set_sprite_glyph_size(&self, gl: &glow::Context, glyph_w: f32, glyph_h: f32) {
        if let Some(sprites) = &self.sprites {
            sprites.set_glyph_size(gl, glyph_w, glyph_h);
        }
    }

    /// Builds the RGBA sprite atlas + program from `set` and attaches it, reusing the shared quad
    /// and index buffers (issue #366).
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError::Init`] if the sprite program or atlas texture can't be created.
    #[cfg(feature = "tilesets")]
    pub(crate) fn attach_sprites(
        &mut self,
        gl: &glow::Context,
        flavor: GlslFlavor,
        set: &SpriteSet,
    ) -> Result<(), SurfaceError> {
        // SAFETY: `gl` is the live context these resources belong to; `SpriteGpu::new` only issues
        // GL calls against it, matching every other resource-creation path in this module.
        let sprites = unsafe { SpriteGpu::new(gl, flavor, self.quad_vbo, set)? };
        self.sprites = Some(sprites);
        Ok(())
    }

    /// Uploads and draws one layer's sprite instances over the glyph passes, source-over blended
    /// (issue #366). No-op without a sprite atlas or with no sprite cells on the layer.
    #[cfg(feature = "tilesets")]
    pub(crate) fn draw_sprites(&mut self, gl: &glow::Context, instances: &[SpriteInstance]) {
        if let Some(sprites) = &mut self.sprites {
            sprites.draw(gl, instances);
        }
    }
}

/// Compiles the glyph program (its vertex + fragment stages).
unsafe fn build_program(
    gl: &glow::Context,
    flavor: GlslFlavor,
) -> Result<glow::Program, SurfaceError> {
    unsafe { build_program_stages(gl, flavor, Shader::Vertex, Shader::Fragment) }
}

/// Compiles the given vertex + fragment stages and links the program, returning a descriptive
/// [`SurfaceError::Init`] on any compile/link failure (with the GL info log).
unsafe fn build_program_stages(
    gl: &glow::Context,
    flavor: GlslFlavor,
    vs: Shader,
    fs: Shader,
) -> Result<glow::Program, SurfaceError> {
    unsafe {
        let program = gl
            .create_program()
            .map_err(|e| SurfaceError::Init(format!("create program: {e}")))?;

        let stages = [
            (glow::VERTEX_SHADER, source(flavor, vs)),
            (glow::FRAGMENT_SHADER, source(flavor, fs)),
        ];
        let mut compiled = Vec::with_capacity(stages.len());
        for (stage, src) in stages {
            let shader = gl
                .create_shader(stage)
                .map_err(|e| SurfaceError::Init(format!("create shader: {e}")))?;
            gl.shader_source(shader, &src);
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                let log = gl.get_shader_info_log(shader);
                return Err(SurfaceError::Init(format!("shader compile failed: {log}")));
            }
            gl.attach_shader(program, shader);
            compiled.push(shader);
        }

        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            let log = gl.get_program_info_log(program);
            return Err(SurfaceError::Init(format!("program link failed: {log}")));
        }

        for shader in compiled {
            gl.detach_shader(program, shader);
            gl.delete_shader(shader);
        }
        Ok(program)
    }
}

/// Uploads the glyph atlas as an `R8` `TEXTURE_2D_ARRAY` with `NEAREST` filtering and
/// `CLAMP_TO_EDGE` wrapping (crisp, no glyph bleeding).
#[allow(clippy::cast_possible_wrap)]
unsafe fn upload_atlas(
    gl: &glow::Context,
    atlas: &AtlasData,
) -> Result<glow::Texture, SurfaceError> {
    unsafe {
        let tex = gl
            .create_texture()
            .map_err(|e| SurfaceError::Init(format!("create atlas texture: {e}")))?;
        gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(tex));
        // R8 rows are 1-byte-per-texel and not 4-byte aligned; unpack one byte at a time.
        gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
        let g = atlas.geometry;
        #[allow(clippy::cast_possible_wrap)]
        gl.tex_image_3d(
            glow::TEXTURE_2D_ARRAY,
            0,
            glow::R8 as i32,
            g.tex_w() as i32,
            g.tex_h() as i32,
            g.layers as i32,
            0,
            glow::RED,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(Some(&atlas.coverage)),
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D_ARRAY,
            glow::TEXTURE_MIN_FILTER,
            glow::NEAREST as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D_ARRAY,
            glow::TEXTURE_MAG_FILTER,
            glow::NEAREST as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D_ARRAY,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D_ARRAY,
            glow::TEXTURE_WRAP_T,
            glow::CLAMP_TO_EDGE as i32,
        );
        Ok(tex)
    }
}

/// Reinterprets an `f32` slice as bytes for `buffer_data`.
const fn bytemuck_f32(data: &[f32]) -> &[u8] {
    // SAFETY: `f32` has no invalid bit patterns and no padding; the byte view covers exactly the
    // slice's bytes.
    unsafe { core::slice::from_raw_parts(data.as_ptr().cast::<u8>(), size_of_val(data)) }
}

/// Reinterprets a `u16` slice as bytes for `buffer_data`.
const fn bytemuck_u16(data: &[u16]) -> &[u8] {
    // SAFETY: `u16` has no invalid bit patterns and no padding; the byte view covers exactly the
    // slice's bytes.
    unsafe { core::slice::from_raw_parts(data.as_ptr().cast::<u8>(), size_of_val(data)) }
}

/// Byte stride of one [`SpriteInstance`], as GL wants it (`i32`).
#[cfg(feature = "tilesets")]
const SPRITE_STRIDE: i32 = 16;

/// Initial sprite-instance buffer capacity, in sprites. `draw` grows it when a layer needs more.
#[cfg(feature = "tilesets")]
const INITIAL_SPRITES: usize = 64;

/// The RGBA sprite atlas plus its own program, VAO, and instance buffer (issue #366). Shares
/// [`GlResources`]'s quad and index buffers; owns everything else.
#[cfg(feature = "tilesets")]
struct SpriteGpu {
    program: glow::Program,
    vao: glow::VertexArray,
    instance_vbo: glow::Buffer,
    atlas: glow::Texture,
    u_screen: Option<glow::UniformLocation>,
    u_cell: Option<glow::UniformLocation>,
    u_glyph: Option<glow::UniformLocation>,
    u_sprites: Option<glow::UniformLocation>,
    /// Instance-buffer capacity in sprites (grows as layers need more sprite cells).
    capacity: usize,
}

#[cfg(feature = "tilesets")]
impl SpriteGpu {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    unsafe fn new(
        gl: &glow::Context,
        flavor: GlslFlavor,
        quad_vbo: glow::Buffer,
        set: &SpriteSet,
    ) -> Result<Self, SurfaceError> {
        unsafe {
            let program =
                build_program_stages(gl, flavor, Shader::SpriteVertex, Shader::SpriteFragment)?;

            let vao = gl
                .create_vertex_array()
                .map_err(|e| SurfaceError::Init(format!("create sprite VAO: {e}")))?;
            gl.bind_vertex_array(Some(vao));

            // Shared unit-quad geometry (attribute 0, divisor 0). Drawn as a triangle strip, so no
            // index buffer is needed.
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(quad_vbo));
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 8, 0);
            gl.enable_vertex_attrib_array(0);

            let instance_vbo = gl
                .create_buffer()
                .map_err(|e| SurfaceError::Init(format!("create sprite instance VBO: {e}")))?;
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(instance_vbo));
            // Pre-allocate a small data store before configuring the attribs (matching the glyph
            // instance buffer), so the VAO records a real buffer binding; `draw` grows it as needed.
            gl.buffer_data_size(
                glow::ARRAY_BUFFER,
                (INITIAL_SPRITES * SPRITE_STRIDE as usize) as i32,
                glow::DYNAMIC_DRAW,
            );
            // a_cell (2 u16 @0), a_layer (1 u16 @4), a_sprite (2 u16 @6), a_offset (2 i16 @10).
            gl.vertex_attrib_pointer_i32(1, 2, glow::UNSIGNED_SHORT, SPRITE_STRIDE, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_divisor(1, 1);
            gl.vertex_attrib_pointer_i32(2, 1, glow::UNSIGNED_SHORT, SPRITE_STRIDE, 4);
            gl.enable_vertex_attrib_array(2);
            gl.vertex_attrib_divisor(2, 1);
            gl.vertex_attrib_pointer_i32(3, 2, glow::UNSIGNED_SHORT, SPRITE_STRIDE, 6);
            gl.enable_vertex_attrib_array(3);
            gl.vertex_attrib_divisor(3, 1);
            gl.vertex_attrib_pointer_i32(4, 2, glow::SHORT, SPRITE_STRIDE, 10);
            gl.enable_vertex_attrib_array(4);
            gl.vertex_attrib_divisor(4, 1);
            gl.bind_vertex_array(None);

            let atlas = upload_sprite_atlas(gl, set)?;

            let u_screen = gl.get_uniform_location(program, "u_screen");
            let u_cell = gl.get_uniform_location(program, "u_cell");
            let u_glyph = gl.get_uniform_location(program, "u_glyph");
            let u_sprite_tex = gl.get_uniform_location(program, "u_sprite_tex");
            let u_sprites = gl.get_uniform_location(program, "u_sprites");

            // The atlas layer size is fixed for the renderer's life, so set it once.
            let (tw, th) = set.tex_size();
            gl.use_program(Some(program));
            #[allow(clippy::cast_precision_loss)]
            gl.uniform_2_f32(u_sprite_tex.as_ref(), tw as f32, th as f32);

            Ok(Self {
                program,
                vao,
                instance_vbo,
                atlas,
                u_screen,
                u_cell,
                u_glyph,
                u_sprites,
                capacity: INITIAL_SPRITES,
            })
        }
    }

    fn set_projection(&self, gl: &glow::Context, sw: f32, sh: f32, cw: f32, ch: f32) {
        unsafe {
            gl.use_program(Some(self.program));
            gl.uniform_2_f32(self.u_screen.as_ref(), sw, sh);
            gl.uniform_2_f32(self.u_cell.as_ref(), cw, ch);
        }
    }

    fn set_glyph_size(&self, gl: &glow::Context, gw: f32, gh: f32) {
        unsafe {
            gl.use_program(Some(self.program));
            gl.uniform_2_f32(self.u_glyph.as_ref(), gw, gh);
        }
    }

    #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    fn draw(&mut self, gl: &glow::Context, instances: &[SpriteInstance]) {
        if instances.is_empty() {
            return;
        }
        unsafe {
            gl.use_program(Some(self.program));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(self.atlas));
            gl.uniform_1_i32(self.u_sprites.as_ref(), 0);
            gl.bind_vertex_array(Some(self.vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.instance_vbo));
            let bytes = sprite_instances_as_bytes(instances);
            if instances.len() > self.capacity {
                gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::DYNAMIC_DRAW);
                self.capacity = instances.len();
            } else {
                gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, bytes);
            }

            // Source-over blend so a sprite's transparent pixels reveal what's beneath; keep the
            // framebuffer alpha at 1 (`ONE`/`ONE_MINUS_SRC_ALPHA`) so the surface stays opaque.
            gl.enable(glow::BLEND);
            gl.blend_func_separate(
                glow::SRC_ALPHA,
                glow::ONE_MINUS_SRC_ALPHA,
                glow::ONE,
                glow::ONE_MINUS_SRC_ALPHA,
            );
            // The quad corners are in triangle-strip order (TL, TR, BL, BR), so draw the sprite
            // quads without an index buffer.
            gl.draw_arrays_instanced(glow::TRIANGLE_STRIP, 0, 4, instances.len() as i32);
            gl.disable(glow::BLEND);
            gl.bind_vertex_array(None);
        }
    }

    fn delete(&self, gl: &glow::Context) {
        unsafe {
            gl.delete_program(self.program);
            gl.delete_vertex_array(self.vao);
            gl.delete_buffer(self.instance_vbo);
            gl.delete_texture(self.atlas);
        }
    }
}

/// Reinterprets a [`SpriteInstance`] slice as bytes for `buffer_data`.
#[cfg(feature = "tilesets")]
const fn sprite_instances_as_bytes(data: &[SpriteInstance]) -> &[u8] {
    // SAFETY: `SpriteInstance` is `#[repr(C)]`, all-integer, no padding beyond the explicit field;
    // the byte view covers exactly the slice's bytes.
    unsafe { core::slice::from_raw_parts(data.as_ptr().cast::<u8>(), size_of_val(data)) }
}

/// Uploads the RGBA sprite atlas as an `RGBA8` `TEXTURE_2D_ARRAY` with `NEAREST` filtering and
/// `CLAMP_TO_EDGE` wrapping (issue #366).
#[cfg(feature = "tilesets")]
#[allow(clippy::cast_possible_wrap)]
unsafe fn upload_sprite_atlas(
    gl: &glow::Context,
    set: &SpriteSet,
) -> Result<glow::Texture, SurfaceError> {
    unsafe {
        let tex = gl
            .create_texture()
            .map_err(|e| SurfaceError::Init(format!("create sprite atlas texture: {e}")))?;
        gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(tex));
        gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
        let (tw, th) = set.tex_size();
        gl.tex_image_3d(
            glow::TEXTURE_2D_ARRAY,
            0,
            glow::RGBA8 as i32,
            tw as i32,
            th as i32,
            set.layers() as i32,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(Some(set.rgba())),
        );
        for (param, value) in [
            (glow::TEXTURE_MIN_FILTER, glow::NEAREST),
            (glow::TEXTURE_MAG_FILTER, glow::NEAREST),
            (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
            (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
        ] {
            gl.tex_parameter_i32(glow::TEXTURE_2D_ARRAY, param, value as i32);
        }
        Ok(tex)
    }
}

#[cfg(test)]
mod tests {
    use super::{INSTANCE_BYTES, INSTANCE_STRIDE, Instance};
    use core::mem::offset_of;

    #[test]
    fn instance_layout_matches_vertex_attrib_offsets() {
        // The `configure_instance_attribs` pointer offsets (glyph@0, fg@4, bg@8, offset@12) and
        // the stride constants must match the actual `#[repr(C)]` layout, or the GPU reads garbage.
        assert_eq!(size_of::<Instance>(), INSTANCE_BYTES);
        assert_eq!(INSTANCE_STRIDE, i32::try_from(INSTANCE_BYTES).unwrap());
        assert_eq!(offset_of!(Instance, glyph), 0);
        assert_eq!(offset_of!(Instance, flags), 2);
        assert_eq!(offset_of!(Instance, fg), 4);
        assert_eq!(offset_of!(Instance, bg), 8);
        assert_eq!(offset_of!(Instance, dx), 12);
        assert_eq!(offset_of!(Instance, dy), 14);
    }
}
