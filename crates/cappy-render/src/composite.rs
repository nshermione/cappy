//! The persistent offscreen texture and the pass that copies it onto the host's target.

use std::borrow::Cow;
use std::collections::HashMap;

use crate::quad::needs_manual_srgb;

const SHADER_SOURCE: &str = concat!(
    include_str!("shaders/color.wgsl"),
    include_str!("shaders/composite.wgsl")
);
const FULLSCREEN_TRIANGLE_VERTICES: u32 = 3;

/// Format of the offscreen texture: premultiplied, sRGB-encoded storage of linear colors.
pub(crate) const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Keeps last frame's pixels so only damaged regions need repainting.
pub(crate) struct Offscreen {
    pub view: wgpu::TextureView,
    pub size: (u32, u32),
    pub bind_group: wgpu::BindGroup,
}

impl Offscreen {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, size: (u32, u32)) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cappy offscreen"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OFFSCREEN_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cappy offscreen source"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });
        Self {
            view,
            size,
            bind_group,
        }
    }
}

pub(crate) struct Compositor {
    shader: wgpu::ShaderModule,
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
}

impl Compositor {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cappy composite shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER_SOURCE)),
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cappy composite layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cappy composite pipeline layout"),
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

    /// Returns an offscreen texture of `size`, recreating it if the size changed.
    /// The flag is `true` when the texture is new and must be repainted completely.
    pub fn ensure_offscreen(
        &self,
        device: &wgpu::Device,
        current: &mut Option<Offscreen>,
        size: (u32, u32),
    ) -> bool {
        if current
            .as_ref()
            .is_some_and(|offscreen| offscreen.size == size)
        {
            return false;
        }
        *current = Some(Offscreen::new(device, &self.bind_group_layout, size));
        true
    }

    pub fn pipeline(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        if let Some(pipeline) = self.pipelines.get(&format) {
            return pipeline.clone();
        }
        let pipeline = self.create(device, format);
        self.pipelines.insert(format, pipeline.clone());
        pipeline
    }

    pub fn draw(
        pass: &mut wgpu::RenderPass<'_>,
        pipeline: &wgpu::RenderPipeline,
        offscreen: &Offscreen,
    ) {
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &offscreen.bind_group, &[]);
        pass.draw(0..FULLSCREEN_TRIANGLE_VERTICES, 0..1);
    }

    fn create(&self, device: &wgpu::Device, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
        let fragment_entry = if needs_manual_srgb(format) {
            "fs_composite_encode_srgb"
        } else {
            "fs_composite"
        };
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cappy composite pipeline"),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs_fullscreen"),
                compilation_options: Default::default(),
                buffers: &[],
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
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    }
}
