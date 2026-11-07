//! Textured quads for [`cappy_core::ImagePrimitive`], one GPU texture per image.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use bytemuck::{Pod, Zeroable};
use cappy_core::{ImagePixels, ImagePrimitive, Primitive};
use wgpu::util::DeviceExt;

use crate::batch::Batch;
use crate::gpu_buffer::GrowableBuffer;
use crate::quad::needs_manual_srgb;

const SHADER_SOURCE: &str = concat!(
    include_str!("shaders/color.wgsl"),
    include_str!("shaders/sdf.wgsl"),
    include_str!("shaders/image.wgsl")
);
const VERTICES_PER_QUAD: u32 = 6;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct ImageInstance {
    clip_rect: [f32; 4],
    dest_rect: [f32; 4],
    uv_rect: [f32; 4],
    radius: f32,
    _padding: [f32; 3],
}

impl ImageInstance {
    const ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
        0 => Float32x4,
        1 => Float32x4,
        2 => Float32x4,
        3 => Float32
    ];

    fn from_primitive(image: &ImagePrimitive, scale_factor: f32) -> Self {
        let clip = image.rect.scale(scale_factor);
        let placement = image.placement();
        let dest = placement.dest.scale(scale_factor);
        Self {
            clip_rect: [clip.x, clip.y, clip.width, clip.height],
            dest_rect: [dest.x, dest.y, dest.width, dest.height],
            uv_rect: [
                placement.uv_min_x,
                placement.uv_min_y,
                placement.uv_max_x,
                placement.uv_max_y,
            ],
            radius: image.corner_radius * scale_factor,
            _padding: [0.0; 3],
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

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Globals {
    viewport: [f32; 2],
    padding: [f32; 2],
}

struct CachedImage {
    bind_group: wgpu::BindGroup,
    alive: Weak<ImagePixels>,
}

struct ImageDraw {
    bind_group: wgpu::BindGroup,
    instance: u32,
}

struct ImagePipelines {
    shader: wgpu::ShaderModule,
    layout: wgpu::PipelineLayout,
    pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
}

impl ImagePipelines {
    fn new(device: &wgpu::Device, bind_group_layout: &wgpu::BindGroupLayout) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cappy image shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(SHADER_SOURCE)),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cappy image pipeline layout"),
            bind_group_layouts: &[Some(bind_group_layout)],
            immediate_size: 0,
        });
        Self {
            shader,
            layout,
            pipelines: HashMap::new(),
        }
    }

    fn get(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
        if let Some(pipeline) = self.pipelines.get(&format) {
            return pipeline.clone();
        }
        let fragment_entry = if needs_manual_srgb(format) {
            "fs_image_encode_srgb"
        } else {
            "fs_image"
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cappy image pipeline"),
            layout: Some(&self.layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs_image"),
                compilation_options: Default::default(),
                buffers: &[Some(ImageInstance::layout())],
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
        });
        self.pipelines.insert(format, pipeline.clone());
        pipeline
    }
}

/// Textures and instances for the image primitives of one frame.
pub(crate) struct ImageLayer {
    bind_group_layout: wgpu::BindGroupLayout,
    pipelines: ImagePipelines,
    sampler: wgpu::Sampler,
    globals: wgpu::Buffer,
    instances: GrowableBuffer,
    cache: HashMap<usize, CachedImage>,
    commands: Vec<Vec<ImageDraw>>,
}

impl ImageLayer {
    pub fn new(device: &wgpu::Device) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cappy image layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let globals = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cappy image globals"),
            contents: bytemuck::bytes_of(&Globals::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("cappy image sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let pipelines = ImagePipelines::new(device, &bind_group_layout);
        Self {
            instances: GrowableBuffer::new(device, "cappy image quads", wgpu::BufferUsages::VERTEX),
            bind_group_layout,
            pipelines,
            sampler,
            globals,
            cache: HashMap::new(),
            commands: Vec::new(),
        }
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        viewport: (u32, u32),
        scale_factor: f32,
        primitives: &[Primitive],
        batches: &[Batch],
    ) {
        self.cache
            .retain(|_, cached| cached.alive.upgrade().is_some());
        let globals = Globals {
            viewport: [viewport.0 as f32, viewport.1 as f32],
            padding: [0.0; 2],
        };
        queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));

        let mut instances = Vec::new();
        self.commands.clear();
        for batch in batches {
            let mut draws = Vec::new();
            for &index in &batch.images {
                let Primitive::Image(image) = &primitives[index] else {
                    continue;
                };
                if image.placement().dest.is_empty() {
                    continue;
                }
                let bind_group = self.texture(device, queue, &image.pixels);
                let slot = instances.len() as u32;
                instances.push(ImageInstance::from_primitive(image, scale_factor));
                draws.push(ImageDraw {
                    bind_group,
                    instance: slot,
                });
            }
            self.commands.push(draws);
        }
        self.instances
            .write(device, queue, bytemuck::cast_slice(&instances));
    }

    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        pass: &mut wgpu::RenderPass<'_>,
        format: wgpu::TextureFormat,
        batch_index: usize,
    ) {
        let empty = self
            .commands
            .get(batch_index)
            .is_none_or(|draws| draws.is_empty());
        if empty {
            return;
        }
        let pipeline = self.pipelines.get(device, format);
        let buffer = self.instances.buffer().clone();
        pass.set_pipeline(&pipeline);
        pass.set_vertex_buffer(0, buffer.slice(..));
        for draw in &self.commands[batch_index] {
            pass.set_bind_group(0, &draw.bind_group, &[]);
            pass.draw(0..VERTICES_PER_QUAD, draw.instance..draw.instance + 1);
        }
    }

    fn texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pixels: &Arc<ImagePixels>,
    ) -> wgpu::BindGroup {
        let key = Arc::as_ptr(pixels) as usize;
        if let Some(cached) = self.cache.get(&key)
            && cached.alive.upgrade().is_some()
        {
            return cached.bind_group.clone();
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cappy image"),
            size: wgpu::Extent3d {
                width: pixels.width(),
                height: pixels.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let (bytes, stride) = pad_rows(pixels);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(pixels.height()),
            },
            texture.size(),
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cappy image"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.globals.as_entire_binding(),
                },
            ],
        });
        self.cache.insert(
            key,
            CachedImage {
                bind_group: bind_group.clone(),
                alive: Arc::downgrade(pixels),
            },
        );
        bind_group
    }
}

fn pad_rows(pixels: &ImagePixels) -> (Vec<u8>, u32) {
    let row = pixels.width() as usize * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
    let stride = row.div_ceil(align) * align;
    if stride == row {
        return (pixels.rgba().to_vec(), stride as u32);
    }
    let mut padded = vec![0; stride * pixels.height() as usize];
    for (index, source) in pixels.rgba().chunks_exact(row).enumerate() {
        let start = index * stride;
        padded[start..start + row].copy_from_slice(source);
    }
    (padded, stride as u32)
}
