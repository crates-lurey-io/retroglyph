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

use crate::atlas::AtlasData;
use crate::error::SurfaceError;
use crate::shaders::{GlslFlavor, Shader, source};
use glow::HasContext as _;

/// Per-cell instance data, tightly packed to 12 bytes and uploaded straight to the GPU.
///
/// `#[repr(C)]` with explicit padding so the field offsets match the vertex-attribute pointers in
/// [`GlResources::configure_instance_attribs`]: `glyph` at 0, `fg` at 4, `bg` at 8.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Instance {
    /// Atlas layer (glyph id) for this cell.
    pub glyph: u16,
    _pad: u16,
    /// Foreground RGB (uploaded as normalized `u8`).
    pub fg: [u8; 3],
    _fg_pad: u8,
    /// Background RGB (uploaded as normalized `u8`).
    pub bg: [u8; 3],
    _bg_pad: u8,
}

impl Instance {
    /// A cell with the given glyph and colors.
    pub(crate) const fn new(glyph: u16, fg: [u8; 3], bg: [u8; 3]) -> Self {
        Self {
            glyph,
            _pad: 0,
            fg,
            _fg_pad: 0,
            bg,
            _bg_pad: 0,
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
const INSTANCE_STRIDE: i32 = 12;
/// The same stride as a `usize`, for buffer-size arithmetic.
const INSTANCE_BYTES: usize = 12;

/// GL objects for the instanced cell renderer.
pub(crate) struct GlResources {
    program: glow::Program,
    vao: glow::VertexArray,
    quad_vbo: glow::Buffer,
    index_buffer: glow::Buffer,
    instance_vbo: glow::Buffer,
    atlas: glow::Texture,
    u_screen: Option<glow::UniformLocation>,
    u_cell: Option<glow::UniformLocation>,
    u_cols: Option<glow::UniformLocation>,
    u_atlas: Option<glow::UniformLocation>,
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

            Ok(Self {
                program,
                vao,
                quad_vbo,
                index_buffer,
                instance_vbo,
                atlas: atlas_tex,
                u_screen,
                u_cell,
                u_cols,
                u_atlas,
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

    /// Uploads only `instances[start..]` (the dirty sub-range) at the matching byte offset in the
    /// instance VBO, leaving the rest of the buffer untouched. `start` is a cell index; the caller
    /// passes the already-sliced dirty range, so `sub` covers `[start, start + sub.len())`.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    pub(crate) fn upload_range(&self, gl: &glow::Context, start: usize, sub: &[Instance]) {
        if sub.is_empty() {
            return;
        }
        let offset = (start * INSTANCE_BYTES) as i32;
        unsafe {
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.instance_vbo));
            gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, offset, instances_as_bytes(sub));
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
    }

    /// Clears the framebuffer and draws every cell in one instanced call.
    pub(crate) fn draw(&self, gl: &glow::Context, cell_count: i32) {
        unsafe {
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);

            gl.use_program(Some(self.program));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D_ARRAY, Some(self.atlas));
            gl.uniform_1_i32(self.u_atlas.as_ref(), 0);

            gl.bind_vertex_array(Some(self.vao));
            gl.draw_elements_instanced(glow::TRIANGLES, 6, glow::UNSIGNED_SHORT, 0, cell_count);
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
    }
}

/// Compiles both stages and links the program, returning a descriptive [`SurfaceError::Init`] on
/// any compile/link failure (with the GL info log).
unsafe fn build_program(
    gl: &glow::Context,
    flavor: GlslFlavor,
) -> Result<glow::Program, SurfaceError> {
    unsafe {
        let program = gl
            .create_program()
            .map_err(|e| SurfaceError::Init(format!("create program: {e}")))?;

        let stages = [
            (glow::VERTEX_SHADER, source(flavor, Shader::Vertex)),
            (glow::FRAGMENT_SHADER, source(flavor, Shader::Fragment)),
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
        // Glyph rows are 8px wide -> not 4-byte aligned; unpack one byte at a time.
        gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
        #[allow(clippy::cast_possible_wrap)]
        gl.tex_image_3d(
            glow::TEXTURE_2D_ARRAY,
            0,
            glow::R8 as i32,
            atlas.width as i32,
            atlas.height as i32,
            atlas.layers as i32,
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
