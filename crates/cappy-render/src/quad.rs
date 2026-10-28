//! Instanced rounded-rectangle pipeline, used for the scene, region clears and the debug overlay.

use std::borrow::Cow;
use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use cappy_core::{Color, Primitive, Rect};
use wgpu::util::DeviceExt;

use crate::gpu_buffer::GrowableBuffer;

const SHADER_SOURCE: &str = concat!(
    include_str!("shaders/color.wgsl"),
    include_str!("shaders/quad.wgsl")
);
const VERTICES_PER_QUAD: u32 = 6;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct QuadInstance {
    rect: [f32; 4],
    background: [f32; 4],
    border_color: [f32; 4],
    shape: [f32; 2],
}

impl QuadInstance {
    const ATTRIBUTES: [wgpu::VertexAttribute; 4] =
        wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x2];

    /// The instance for a display-list quad, scaled to physical pixels.
    pub fn from_primitive(primitive: &Primitive, scale_factor: f32) -> Option<Self> {
        let Primitive::Quad(quad) = primitive else {
            return None;
        };
        Some(Self::new(
            quad.rect.scale(scale_factor),
            quad.background,
            quad.border_color,
            quad.border_width * scale_factor,
            quad.corner_radius * scale_factor,
        ))
    }

    /// A transparent quad that, drawn with [`QuadBlend::Replace`], erases `rect`.
    pub fn eraser(rect: Rect) -> Self {
        Self::new(rect, Color::TRANSPARENT, Color::TRANSPARENT, 0.0, 0.0)
    }

    pub fn new(
        rect: Rect,
        background: Color,
        border: Color,
        border_width: f32,
        radius: f32,
    ) -> Self {
        Self {
            rect: [rect.x, rect.y, rect.width, rect.height],
            background: background.to_linear_premultiplied(),
            border_color: border.to_linear_premultiplied(),
            shape: [border_width, radius],
        }
    }

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum QuadBlend {
    /// Normal painting over what is already there.
    Over,
    /// Overwrites the destination, used to erase damaged regions.
    Replace,
}

impl QuadBlend {
    fn state(self) -> wgpu::BlendState {
        match self {
            Self::Over => wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
            Self::Replace => wgpu::BlendState::REPLACE,
        }
    }
}

/// Whether shaders must sRGB-encode by hand: a non-sRGB variant of an sRGB-capable format.
pub(crate) fn needs_manual_srgb(format: wgpu::TextureFormat) -> bool {
    format != format.add_srgb_suffix()
}

/// Lazily built pipelines, one per target format and blend mode.
pub(crate) struct QuadPipelines {
    shader: wgpu::ShaderModule,
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    pipelines: HashMap<(wgpu::TextureFormat, QuadBlend), wgpu::RenderPipeline>,
}

impl QuadPipelines {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cappy quad shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER_SOURCE)),
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cappy quad globals layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cappy quad pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        Self {
            shader,
            bind_group_layout,
            pipeline_layout,
            pipelines: HashMap::new(),
        }
    }

    pub fn get(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        blend: QuadBlend,
    ) -> wgpu::RenderPipeline {
        if let Some(pipeline) = self.pipelines.get(&(format, blend)) {
            return pipeline.clone();
        }
        let pipeline = self.create(device, format, blend);
        self.pipelines.insert((format, blend), pipeline.clone());
        pipeline
    }

    fn create(
        &self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        blend: QuadBlend,
    ) -> wgpu::RenderPipeline {
        let fragment_entry = if needs_manual_srgb(format) {
            "fs_quad_encode_srgb"
        } else {
            "fs_quad"
        };
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cappy quad pipeline"),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs_quad"),
                compilation_options: Default::default(),
                buffers: &[Some(QuadInstance::layout())],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some(fragment_entry),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(blend.state()),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Globals {
    viewport: [f32; 2],
    padding: [f32; 2],
}

/// Instance data and globals for drawing quads into one target.
pub(crate) struct QuadBatch {
    instances: GrowableBuffer,
    globals: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl QuadBatch {
    pub fn new(device: &wgpu::Device, pipelines: &QuadPipelines, label: &'static str) -> Self {
        let globals = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cappy quad globals"),
            contents: bytemuck::bytes_of(&Globals::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cappy quad globals"),
            layout: &pipelines.bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        Self {
            instances: GrowableBuffer::new(device, label, wgpu::BufferUsages::VERTEX),
            globals,
            bind_group,
        }
    }

    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        viewport: (u32, u32),
        instances: &[QuadInstance],
    ) {
        let globals = Globals {
            viewport: [viewport.0 as f32, viewport.1 as f32],
            padding: [0.0; 2],
        };
        queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));
        self.instances
            .write(device, queue, bytemuck::cast_slice(instances));
    }

    /// Draws the uploaded instances in `range`.
    pub fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        pipeline: &wgpu::RenderPipeline,
        range: std::ops::Range<u32>,
    ) {
        if range.is_empty() {
            return;
        }
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instances.buffer().slice(..));
        pass.draw(0..VERTICES_PER_QUAD, range);
    }
}
