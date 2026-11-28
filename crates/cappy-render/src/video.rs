//! YUV quads for [`cappy_core::VideoPrimitive`]. One set of planes per video id.
//! A new generation with the same size uploads into those textures.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use cappy_core::{Primitive, VideoPrimitive};
use wgpu::util::DeviceExt;

use crate::batch::Batch;
use crate::gpu_buffer::GrowableBuffer;
use crate::quad::needs_manual_srgb;

const SHADER_SOURCE: &str = concat!(
    include_str!("shaders/color.wgsl"),
    include_str!("shaders/sdf.wgsl"),
    include_str!("shaders/video.wgsl")
);
const VERTICES_PER_QUAD: u32 = 6;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct VideoInstance {
    clip_rect: [f32; 4],
    dest_rect: [f32; 4],
    uv_rect: [f32; 4],
    radius: f32,
    opacity: f32,
    _pad: [f32; 2],
    scroll_clip: [f32; 4],
}

impl VideoInstance {
    const ATTRIBUTES: [wgpu::VertexAttribute; 5] = [
        wgpu::VertexAttribute {
            offset: 0,
            shader_location: 0,
            format: wgpu::VertexFormat::Float32x4,
        },
        wgpu::VertexAttribute {
            offset: 16,
            shader_location: 1,
            format: wgpu::VertexFormat::Float32x4,
        },
        wgpu::VertexAttribute {
            offset: 32,
            shader_location: 2,
            format: wgpu::VertexFormat::Float32x4,
        },
        wgpu::VertexAttribute {
            offset: 48,
            shader_location: 3,
            format: wgpu::VertexFormat::Float32x4,
        },
        wgpu::VertexAttribute {
            offset: 64,
            shader_location: 4,
            format: wgpu::VertexFormat::Float32x4,
        },
    ];

    fn from_primitive(video: &VideoPrimitive, scale_factor: f32) -> Self {
        let clip = video.rect.scale(scale_factor);
        let placement = video.placement();
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
            radius: video.corner_radius * scale_factor,
            opacity: video.opacity,
            _pad: [0.0; 2],
            scroll_clip: crate::quad::physical_clip(video.clip, scale_factor),
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

struct PlaneSet {
    y: wgpu::Texture,
    u: wgpu::Texture,
    v: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
    generation: u64,
}

struct VideoDraw {
    bind_group: wgpu::BindGroup,
    instance: u32,
}

struct VideoPipelines {
    shader: wgpu::ShaderModule,
    layout: wgpu::PipelineLayout,
    pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
}

impl VideoPipelines {
    fn new(device: &wgpu::Device, bind_group_layout: &wgpu::BindGroupLayout) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cappy video shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(SHADER_SOURCE)),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cappy video pipeline layout"),
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
            "fs_video_encode_srgb"
        } else {
            "fs_video"
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cappy video pipeline"),
            layout: Some(&self.layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs_video"),
                compilation_options: Default::default(),
                buffers: &[Some(VideoInstance::layout())],
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

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

pub(crate) struct VideoLayer {
    bind_group_layout: wgpu::BindGroupLayout,
    pipelines: VideoPipelines,
    sampler: wgpu::Sampler,
    globals: wgpu::Buffer,
    instances: GrowableBuffer,
    cache: HashMap<u64, PlaneSet>,
    commands: Vec<Vec<VideoDraw>>,
    textures_created: u32,
}

impl VideoLayer {
    pub fn new(device: &wgpu::Device) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cappy video layout"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                texture_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
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
            label: Some("cappy video globals"),
            contents: bytemuck::bytes_of(&Globals::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("cappy video sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let pipelines = VideoPipelines::new(device, &bind_group_layout);
        Self {
            instances: GrowableBuffer::new(device, "cappy video quads", wgpu::BufferUsages::VERTEX),
            bind_group_layout,
            pipelines,
            sampler,
            globals,
            cache: HashMap::new(),
            commands: Vec::new(),
            textures_created: 0,
        }
    }

    pub fn textures_created(&self) -> u32 {
        self.textures_created
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
        let globals = Globals {
            viewport: [viewport.0 as f32, viewport.1 as f32],
            padding: [0.0; 2],
        };
        queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));
        let mut instances = Vec::new();
        self.commands.clear();
        for batch in batches {
            let mut draws = Vec::new();
            for &index in &batch.videos {
                let Primitive::Video(video) = &primitives[index] else {
                    continue;
                };
                if video.placement().dest.is_empty() {
                    continue;
                }
                let bind_group = self.planes(device, queue, video);
                let slot = instances.len() as u32;
                instances.push(VideoInstance::from_primitive(video, scale_factor));
                draws.push(VideoDraw {
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
    ) -> usize {
        let empty = self
            .commands
            .get(batch_index)
            .is_none_or(|draws| draws.is_empty());
        if empty {
            return 0;
        }
        let pipeline = self.pipelines.get(device, format);
        let buffer = self.instances.buffer().clone();
        pass.set_pipeline(&pipeline);
        pass.set_vertex_buffer(0, buffer.slice(..));
        let draws = &self.commands[batch_index];
        for draw in draws {
            pass.set_bind_group(0, &draw.bind_group, &[]);
            pass.draw(0..VERTICES_PER_QUAD, draw.instance..draw.instance + 1);
        }
        draws.len()
    }

    fn planes(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        video: &VideoPrimitive,
    ) -> wgpu::BindGroup {
        if let Some(cached) = self.cache.get_mut(&video.id)
            && cached.width == video.width
            && cached.height == video.height
        {
            if cached.generation != video.generation {
                write_plane(queue, &cached.y, &video.y, video.width, video.height);
                let chroma_w = video.width.div_ceil(2);
                let chroma_h = video.height.div_ceil(2);
                write_plane(queue, &cached.u, &video.u, chroma_w, chroma_h);
                write_plane(queue, &cached.v, &video.v, chroma_w, chroma_h);
                cached.generation = video.generation;
            }
            return cached.bind_group.clone();
        }
        self.textures_created += 1;
        let y = plane_texture(device, "cappy video y", video.width, video.height);
        let chroma_w = video.width.div_ceil(2);
        let chroma_h = video.height.div_ceil(2);
        let u = plane_texture(device, "cappy video u", chroma_w, chroma_h);
        let v = plane_texture(device, "cappy video v", chroma_w, chroma_h);
        write_plane(queue, &y, &video.y, video.width, video.height);
        write_plane(queue, &u, &video.u, chroma_w, chroma_h);
        write_plane(queue, &v, &video.v, chroma_w, chroma_h);
        let y_view = y.create_view(&wgpu::TextureViewDescriptor::default());
        let u_view = u.create_view(&wgpu::TextureViewDescriptor::default());
        let v_view = v.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cappy video"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&y_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&u_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&v_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: self.globals.as_entire_binding(),
                },
            ],
        });
        let bind = bind_group.clone();
        self.cache.insert(
            video.id,
            PlaneSet {
                y,
                u,
                v,
                bind_group,
                width: video.width,
                height: video.height,
                generation: video.generation,
            },
        );
        bind
    }
}

fn plane_texture(device: &wgpu::Device, label: &str, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn write_plane(queue: &wgpu::Queue, texture: &wgpu::Texture, bytes: &[u8], width: u32, height: u32) {
    let (padded, stride) = pad_rows(bytes, width, height);
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &padded,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(stride),
            rows_per_image: Some(height),
        },
        texture.size(),
    );
}

fn pad_rows(bytes: &[u8], width: u32, height: u32) -> (Vec<u8>, u32) {
    let row = width as usize;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
    let stride = row.div_ceil(align) * align;
    if stride == row {
        return (bytes.to_vec(), stride as u32);
    }
    let mut padded = vec![0; stride * height as usize];
    for (index, source) in bytes.chunks(row).take(height as usize).enumerate() {
        let start = index * stride;
        let len = source.len().min(row);
        padded[start..start + len].copy_from_slice(&source[..len]);
    }
    (padded, stride as u32)
}
