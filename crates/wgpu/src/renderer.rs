//! GPU resources and the per-frame draw.
//!
//! [`GpuResources`] owns the pipelines, the uniform buffer, the glyph atlas texture, and the
//! instance buffers. It is created once the device exists and driven once per frame by
//! [`render`](GpuResources::render), which encodes every grid layer into a single render pass.
//!
//! # One pass, one upload, one submit
//!
//! Every layer's cells live back to back in one instance buffer, so a frame is one
//! [`Queue::write_buffer`](wgpu::Queue::write_buffer) followed by one render pass. A layer is drawn
//! by binding its own slice of that buffer, which makes each layer's instance indices start at 0
//! again; the vertex shader can therefore derive a cell's `(column, row)` from the instance index
//! without ever consulting a base offset.
//!
//! Re-uploading one shared buffer between layers would be simpler to write and wrong to run:
//! `write_buffer` is ordered against submits, not against encoded passes, so a second write before
//! the single submit would land under the first layer's draw as well.
//!
//! # Two pipelines, not one pipeline with a mode uniform
//!
//! Backgrounds and glyphs differ in two ways: whether the sub-cell offset applies, and whether
//! blending is on. Both are pipeline state in WebGPU, and a uniform can't be rewritten mid-pass
//! without dynamic offsets, so the two passes are two pipelines built from the same shader module.
//! Switching pipelines inside a pass is cheap; the alternative would be a second pass, a second
//! encoder, or a dynamic-offset uniform, all of which cost more than a pipeline bind.

// `redundant_pub_crate` fires on `pub(crate)` items in this private module; the module boundary is
// intentional, so it's allowed crate-locally.
#![allow(clippy::redundant_pub_crate)]

use crate::error::SurfaceError;
use crate::instance::{CELL_STRIDE, Cell};
use crate::shaders::{self, Shader};
use bytemuck::{Pod, Zeroable};
use retroglyph_window::atlas::{ATLAS_COLS, ATLAS_ROWS, AtlasData};
use wgpu::util::DeviceExt as _;

#[cfg(feature = "tilesets")]
use crate::sprite_set::{SPRITE_INSTANCE_BYTES, SPRITE_STRIDE, SpriteInstance, SpriteSet};

/// The uniform block every pipeline reads, mirroring `common.wgsl`'s `Uniforms`.
///
/// `#[repr(C)]` and padded to 48 bytes: WGSL's uniform address space requires a block's size to be
/// a multiple of 16, and each `vec2` member to sit on an 8-byte boundary, which the field order
/// below satisfies without any implicit padding.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct Uniforms {
    /// Surface size in physical pixels.
    screen: [f32; 2],
    /// Cell size in physical pixels (glyph size x integer scale).
    cell: [f32; 2],
    /// Glyph size in unscaled font pixels.
    glyph: [f32; 2],
    /// One sprite-atlas layer's size in texels; zero without a tileset. Present regardless of the
    /// `tilesets` feature so the uniform block has a single layout: the shaders declare it in
    /// shared WGSL that both modules include.
    sprite_tex: [f32; 2],
    /// Grid columns, to unpack a cell's instance index into `(column, row)`.
    cols: u32,
    /// Glyph columns and rows packed per glyph-atlas array layer.
    atlas_cols: u32,
    atlas_rows: u32,
    /// Pads the block to a multiple of 16 bytes.
    _pad: u32,
}

/// The vertex buffer layout for [`Cell`], as declared to the cell pipelines.
///
/// Kept next to [`Cell`]'s own offset test: the two together are the whole contract between the
/// Rust struct and `cells.wgsl`'s `CellInput`.
const fn cell_layout() -> wgpu::VertexBufferLayout<'static> {
    // `glyph`+`flags` at 0, `fg` at 4, `bg` at 8, `dx`+`dy` at 12.
    const ATTRS: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
        0 => Uint16x2,
        1 => Unorm8x4,
        2 => Unorm8x4,
        3 => Sint16x2,
    ];
    wgpu::VertexBufferLayout {
        array_stride: CELL_STRIDE,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &ATTRS,
    }
}

/// The vertex buffer layout for [`SpriteInstance`], as declared to the sprite pipeline.
///
/// The two color stages are `Uint8x4`, not `Unorm8x4`: the fragment shader reproduces
/// [`Tint::apply`](retroglyph_core::color::Tint::apply)'s `u8` arithmetic exactly, which needs the
/// raw 0..255 channel values rather than normalized floats.
#[cfg(feature = "tilesets")]
const fn sprite_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRS: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
        0 => Uint16x2,
        1 => Uint16x2,
        2 => Sint16x2,
        3 => Uint16x2,
        4 => Uint8x4,
        5 => Uint8x4,
    ];
    wgpu::VertexBufferLayout {
        array_stride: SPRITE_STRIDE,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &ATTRS,
    }
}

/// The blend state for the glyph pass.
///
/// The alpha factors are `Zero`/`One`, so `A = src.a * 0 + dst.a * 1` leaves the destination alpha
/// untouched. Coverage must drive the color channels only: it is a glyph mask, not a surface
/// transparency, and letting it reach the alpha channel would punch every non-glyph texel down to
/// alpha 0, which a compositing window manager would then show through.
const GLYPH_BLEND: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::SrcAlpha,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
};

/// The blend state for the sprite pass: source-over, so a sprite's transparent pixels reveal what
/// is beneath, with the framebuffer alpha kept at 1 so the surface stays opaque.
#[cfg(feature = "tilesets")]
const SPRITE_BLEND: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::SrcAlpha,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};

/// A grid layer's slice of the instance buffer, in instances.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LayerRange {
    /// First instance, as an index into the buffer.
    pub start: u32,
    /// Number of instances.
    pub count: u32,
}

impl LayerRange {
    /// The byte range this layer occupies in a buffer of `stride`-byte instances.
    const fn bytes(self, stride: u64) -> std::ops::Range<u64> {
        let start = self.start as u64 * stride;
        start..start + self.count as u64 * stride
    }
}

/// Everything the draw path needs on the device: pipelines, bind groups, and buffers.
pub(crate) struct GpuResources {
    background: wgpu::RenderPipeline,
    glyph: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    uniform_group: wgpu::BindGroup,
    glyph_atlas_group: wgpu::BindGroup,
    /// Every layer's cells, back to back. Grown by [`upload_cells`](Self::upload_cells).
    cells: wgpu::Buffer,
    /// Capacity of `cells`, in instances.
    cell_capacity: u32,
    /// The sprite pipeline, atlas bind group, and instance buffer; `None` without a tileset.
    #[cfg(feature = "tilesets")]
    sprites: Option<SpriteGpu>,
    /// The uniform values last written, so a frame that changes nothing skips the write.
    last_uniforms: Uniforms,
}

impl GpuResources {
    /// Builds every pipeline and uploads the glyph atlas.
    ///
    /// `target_format` is the format the pipelines render into, which is the surface's non-sRGB
    /// view format (see [`gpu`](crate::gpu)) or the offscreen texture's.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError::Init`] if the atlas is larger than the device's array-layer or
    /// texture-dimension limits, which is the one failure here that isn't a validation error the
    /// device reports asynchronously.
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
        atlas: &AtlasData,
        cell_capacity: u32,
    ) -> Result<Self, SurfaceError> {
        let uniform_layout = uniform_bind_group_layout(device);
        let atlas_layout = atlas_bind_group_layout(device);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("retroglyph cells"),
            bind_group_layouts: &[Some(&uniform_layout), Some(&atlas_layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("retroglyph cells"),
            source: wgpu::ShaderSource::Wgsl(shaders::source(Shader::Cells).into()),
        });

        let background = cell_pipeline(
            device,
            &pipeline_layout,
            &module,
            target_format,
            "vs_background",
            "fs_background",
            None,
        );
        let glyph = cell_pipeline(
            device,
            &pipeline_layout,
            &module,
            target_format,
            "vs_glyph",
            "fs_glyph",
            Some(GLYPH_BLEND),
        );

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("retroglyph uniforms"),
            size: size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("retroglyph uniforms"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let g = atlas.geometry;
        check_atlas_fits(device, g.tex_w(), g.tex_h(), g.layers)?;
        let glyph_atlas = upload_array_texture(
            device,
            queue,
            "retroglyph glyph atlas",
            wgpu::TextureFormat::R8Unorm,
            (g.tex_w(), g.tex_h(), g.layers),
            &atlas.coverage,
        );
        let glyph_atlas_group = atlas_bind_group(device, &atlas_layout, &glyph_atlas);

        let cells = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("retroglyph cells"),
            size: u64::from(cell_capacity.max(1)) * CELL_STRIDE,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Ok(Self {
            background,
            glyph,
            uniform_buffer,
            uniform_group,
            glyph_atlas_group,
            cells,
            cell_capacity: cell_capacity.max(1),
            #[cfg(feature = "tilesets")]
            sprites: None,
            last_uniforms: Uniforms::default(),
        })
    }

    /// Builds the sprite pipeline and uploads the RGBA sprite atlas.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceError::Init`] if the sprite atlas exceeds the device's array-layer or
    /// texture-dimension limits.
    #[cfg(feature = "tilesets")]
    pub(crate) fn attach_sprites(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
        set: &SpriteSet,
    ) -> Result<(), SurfaceError> {
        let uniform_layout = uniform_bind_group_layout(device);
        let atlas_layout = atlas_bind_group_layout(device);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("retroglyph sprites"),
            bind_group_layouts: &[Some(&uniform_layout), Some(&atlas_layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("retroglyph sprites"),
            source: wgpu::ShaderSource::Wgsl(shaders::source(Shader::Sprites).into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("retroglyph sprites"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_sprite"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(sprite_layout())],
            },
            primitive: QUAD_STRIP,
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_sprite"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(SPRITE_BLEND),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let (tex_w, tex_h) = set.tex_size();
        check_atlas_fits(device, tex_w, tex_h, set.layers())?;
        let texture = upload_array_texture(
            device,
            queue,
            "retroglyph sprite atlas",
            wgpu::TextureFormat::Rgba8Unorm,
            (tex_w, tex_h, set.layers()),
            set.rgba(),
        );
        let atlas_group = atlas_bind_group(device, &atlas_layout, &texture);
        let instances = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("retroglyph sprite instances"),
            contents: &[0u8; SPRITE_INSTANCE_BYTES],
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });
        self.sprites = Some(SpriteGpu {
            pipeline,
            atlas_group,
            instances,
            capacity: 1,
        });
        Ok(())
    }

    /// Writes the projection uniforms, skipping the write when nothing changed.
    #[allow(clippy::cast_precision_loss, clippy::too_many_arguments)]
    pub(crate) fn set_uniforms(
        &mut self,
        queue: &wgpu::Queue,
        screen: (u32, u32),
        cell: (u32, u32),
        glyph: (u32, u32),
        cols: u16,
        sprite_tex: (u32, u32),
    ) {
        let uniforms = Uniforms {
            screen: [screen.0 as f32, screen.1 as f32],
            cell: [cell.0 as f32, cell.1 as f32],
            glyph: [glyph.0 as f32, glyph.1 as f32],
            sprite_tex: [sprite_tex.0 as f32, sprite_tex.1 as f32],
            cols: u32::from(cols),
            atlas_cols: ATLAS_COLS,
            atlas_rows: ATLAS_ROWS,
            _pad: 0,
        };
        if uniforms == self.last_uniforms {
            return;
        }
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
        self.last_uniforms = uniforms;
    }

    /// Uploads every layer's cells as one contiguous block, growing the buffer if the frame needs
    /// more room than the last one did.
    pub(crate) fn upload_cells(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        cells: &[Cell],
    ) {
        let needed = u32::try_from(cells.len()).unwrap_or(u32::MAX).max(1);
        if needed > self.cell_capacity {
            // Grow geometrically: a layer appearing mid-frame would otherwise reallocate the whole
            // buffer on every frame that adds one.
            let capacity = needed.max(self.cell_capacity.saturating_mul(2));
            self.cells = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("retroglyph cells"),
                size: u64::from(capacity) * CELL_STRIDE,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.cell_capacity = capacity;
        }
        if !cells.is_empty() {
            queue.write_buffer(&self.cells, 0, bytemuck::cast_slice(cells));
        }
    }

    /// Uploads every layer's sprites as one contiguous block. No-op without a sprite atlas.
    #[cfg(feature = "tilesets")]
    pub(crate) fn upload_sprites(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        sprites: &[SpriteInstance],
    ) {
        let Some(gpu) = &mut self.sprites else {
            return;
        };
        gpu.upload(device, queue, sprites);
    }

    /// Encodes one frame: clear, then every layer back to front, into `view`.
    ///
    /// A layer is three draws over the same instance slice: backgrounds (opaque, unshifted), then
    /// glyphs (coverage-blended, shifted by each cell's sub-cell offset), then sprites. Laying
    /// down every background before any glyph is what lets an offset glyph spill past its cell
    /// edge into a neighbour in all four directions; interleaving them per cell would let a later
    /// cell's background overwrite an earlier neighbour's spill. That is the two-pass mechanism of
    /// the sub-cell offset contract documented on [`Presenter`](retroglyph_window::Presenter),
    /// shared with `retroglyph-software`.
    ///
    /// Compositing layers in one pass is what makes an empty cell in a higher layer transparent:
    /// each pass `discard`s a cell whose matching flag is clear, so the layer beneath survives.
    pub(crate) fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        layers: &[LayerRange],
        #[cfg(feature = "tilesets")] sprite_layers: &[LayerRange],
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("retroglyph frame"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    // Opaque black, matching `retroglyph-software`'s cleared pixel buffer. The base
                    // layer's opaque backgrounds paint over it immediately; the clear only shows
                    // through in the sub-cell strip a non-exact-multiple window size leaves.
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        pass.set_bind_group(0, &self.uniform_group, &[]);

        for (index, layer) in layers.iter().enumerate() {
            if layer.count > 0 {
                // Binding this layer's own slice restarts the instance index at 0, so the vertex
                // shader's `instance_index % cols` addressing needs no per-layer base.
                pass.set_bind_group(1, &self.glyph_atlas_group, &[]);
                pass.set_vertex_buffer(0, self.cells.slice(layer.bytes(CELL_STRIDE)));
                pass.set_pipeline(&self.background);
                pass.draw(0..4, 0..layer.count);
                pass.set_pipeline(&self.glyph);
                pass.draw(0..4, 0..layer.count);
            }

            #[cfg(feature = "tilesets")]
            if let (Some(gpu), Some(range)) = (&self.sprites, sprite_layers.get(index)) {
                gpu.draw(&mut pass, *range);
            }
            #[cfg(not(feature = "tilesets"))]
            let _ = index;
        }
    }
}

/// The sprite pipeline, its atlas bind group, and its instance buffer.
#[cfg(feature = "tilesets")]
struct SpriteGpu {
    pipeline: wgpu::RenderPipeline,
    atlas_group: wgpu::BindGroup,
    instances: wgpu::Buffer,
    /// Capacity of `instances`, in sprites.
    capacity: u32,
}

#[cfg(feature = "tilesets")]
impl SpriteGpu {
    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, sprites: &[SpriteInstance]) {
        let needed = u32::try_from(sprites.len()).unwrap_or(u32::MAX).max(1);
        if needed > self.capacity {
            let capacity = needed.max(self.capacity.saturating_mul(2));
            self.instances = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("retroglyph sprite instances"),
                size: u64::from(capacity) * SPRITE_STRIDE,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.capacity = capacity;
        }
        if !sprites.is_empty() {
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(sprites));
        }
    }

    fn draw(&self, pass: &mut wgpu::RenderPass<'_>, range: LayerRange) {
        if range.count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(1, &self.atlas_group, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(range.bytes(SPRITE_STRIDE)));
        pass.draw(0..4, 0..range.count);
    }
}

/// A unit quad as a four-vertex triangle strip, with culling off: the shader emits corners in
/// screen space with y flipped, so winding is not a meaningful signal here.
const QUAD_STRIP: wgpu::PrimitiveState = wgpu::PrimitiveState {
    topology: wgpu::PrimitiveTopology::TriangleStrip,
    strip_index_format: None,
    front_face: wgpu::FrontFace::Ccw,
    cull_mode: None,
    unclipped_depth: false,
    polygon_mode: wgpu::PolygonMode::Fill,
    conservative: false,
};

/// Builds one of the two cell pipelines. They differ only in entry points and blend state.
fn cell_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    vertex_entry: &str,
    fragment_entry: &str,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fragment_entry),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(vertex_entry),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(cell_layout())],
        },
        primitive: QUAD_STRIP,
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fragment_entry),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// Group 0: the shared uniform block, read by both shader stages.
fn uniform_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("retroglyph uniforms"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(size_of::<Uniforms>() as u64),
            },
            count: None,
        }],
    })
}

/// Group 1: an array-texture atlas and its sampler.
///
/// One layout serves both atlases. The glyph atlas is `R8Unorm` and the sprite atlas `Rgba8Unorm`,
/// but a bind group layout constrains the sample *type*, not the format, and both are non-filterable
/// float, so switching atlases mid-pass is a bind-group change rather than a second layout.
fn atlas_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("retroglyph atlas"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    // Non-filterable, paired with the `NonFiltering` sampler below: this backend
                    // only ever samples `Nearest`, and declaring that keeps the pipeline valid on
                    // devices where a filterable float texture is a stricter requirement.
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                count: None,
            },
        ],
    })
}

/// Binds `texture` and a nearest/clamp sampler as group 1.
fn atlas_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    texture: &wgpu::Texture,
) -> wgpu::BindGroup {
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("retroglyph atlas"),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    // Nearest filtering keeps glyphs crisp at any integer scale; clamping stops a glyph cell from
    // bleeding into its neighbour in the grid-packed atlas.
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("retroglyph atlas"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("retroglyph atlas"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    })
}

/// Creates an array texture and uploads `data` to it in one call.
///
/// `Queue::write_texture` takes a tightly-packed source: the 256-byte row alignment that applies to
/// buffer-to-texture copies is wgpu's problem here, not the caller's, so an 8-texel-wide `R8` atlas
/// row uploads as 8 bytes.
fn upload_array_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    format: wgpu::TextureFormat,
    (width, height, layers): (u32, u32, u32),
    data: &[u8],
) -> wgpu::Texture {
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: layers,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let bytes_per_texel = format.block_copy_size(None).unwrap_or(1);
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * bytes_per_texel),
            rows_per_image: Some(height),
        },
        size,
    );
    texture
}

/// Rejects an atlas the device cannot hold, with a message naming the limit it exceeded.
///
/// Every other resource here is sized by the caller's grid, which the builder already bounds. The
/// atlas is sized by the font chain, so this is the one place a legal configuration can still be
/// too big for the hardware, and a clear error beats a device-lost panic from the validation layer.
fn check_atlas_fits(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    layers: u32,
) -> Result<(), SurfaceError> {
    let limits = device.limits();
    if layers > limits.max_texture_array_layers {
        return Err(SurfaceError::Init(format!(
            "atlas needs {layers} array layers; this device allows {}",
            limits.max_texture_array_layers
        )));
    }
    let max = limits.max_texture_dimension_2d;
    if width > max || height > max {
        return Err(SurfaceError::Init(format!(
            "atlas layer is {width}x{height} texels; this device allows {max}x{max}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CELL_STRIDE, LayerRange, Uniforms, cell_layout};

    #[test]
    fn uniform_block_is_a_multiple_of_sixteen_bytes() {
        // WGSL's uniform address space requires it, and `min_binding_size` above declares this
        // exact size to the validator.
        assert_eq!(size_of::<Uniforms>(), 48);
    }

    #[test]
    fn cell_layout_declares_the_instance_stride() {
        let layout = cell_layout();
        assert_eq!(layout.array_stride, CELL_STRIDE);
        assert_eq!(layout.step_mode, wgpu::VertexStepMode::Instance);
        assert_eq!(layout.attributes.len(), 4);
        let offsets: Vec<u64> = layout.attributes.iter().map(|a| a.offset).collect();
        assert_eq!(offsets, vec![0, 4, 8, 12]);
    }

    #[cfg(feature = "tilesets")]
    #[test]
    fn sprite_layout_declares_the_sprite_stride() {
        use super::{SPRITE_STRIDE, sprite_layout};
        let layout = sprite_layout();
        assert_eq!(layout.array_stride, SPRITE_STRIDE);
        assert_eq!(layout.step_mode, wgpu::VertexStepMode::Instance);
        let offsets: Vec<u64> = layout.attributes.iter().map(|a| a.offset).collect();
        assert_eq!(offsets, vec![0, 4, 8, 12, 16, 20]);
    }

    #[test]
    fn layer_ranges_slice_the_shared_buffer_without_overlapping() {
        // Two layers of six cells each: the second starts exactly where the first ends, which is
        // what makes one upload serve every layer.
        let first = LayerRange { start: 0, count: 6 };
        let second = LayerRange { start: 6, count: 6 };
        assert_eq!(first.bytes(CELL_STRIDE), 0..96);
        assert_eq!(second.bytes(CELL_STRIDE), 96..192);
    }

    #[test]
    fn an_empty_layer_slices_an_empty_range() {
        let empty = LayerRange { start: 4, count: 0 };
        assert!(empty.bytes(CELL_STRIDE).is_empty());
    }
}
