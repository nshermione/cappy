//! Tessellated [`cappy_core::VectorPrimitive`]s. Paths stay curves until this layer flattens
//! them in physical pixels, then a 1px fringe covers the aliased edge.

use std::collections::HashMap;
use std::sync::Weak;

use bytemuck::{Pod, Zeroable};
use cappy_core::vector::{
    FillRule, LineCap, LineJoin, PathVerb, VectorPaint, VectorPath, VectorPicture, VectorStyle,
};
use cappy_core::{Color, Point, Primitive, Rect, VectorPrimitive};
use lyon_path::Path;
use lyon_tessellation::geometry_builder::{
    BuffersBuilder, FillVertexConstructor, StrokeVertexConstructor, VertexBuffers,
};
use lyon_tessellation::{
    FillOptions, FillTessellator, FillVertex, LineCap as LyonCap, LineJoin as LyonJoin, Side,
    StrokeOptions, StrokeTessellator, StrokeVertex,
};

use crate::batch::Batch;
use crate::gpu_buffer::GrowableBuffer;
use crate::quad::{needs_manual_srgb, physical_clip};

const SHADER_SOURCE: &str = concat!(
    include_str!("shaders/color.wgsl"),
    include_str!("shaders/sdf.wgsl"),
    include_str!("shaders/vector.wgsl")
);
const TOLERANCE: f32 = 0.25;
const FRINGE_WIDTH: f32 = 1.0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct VectorVertex {
    position: [f32; 2],
    /// x is fill coverage or a signed distance from a stroke centerline. y is the stroke half
    /// width, or 0 when x is fill coverage.
    edge: [f32; 2],
    color: [f32; 4],
    clip_rect: [f32; 4],
    dest_rect: [f32; 4],
    scroll_clip: [f32; 4],
    radius: f32,
    _pad: [f32; 3],
}

impl VectorVertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x2,
        2 => Float32x4,
        3 => Float32x4,
        4 => Float32x4,
        5 => Float32x4,
        6 => Float32,
    ];

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
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

#[derive(Clone, Copy)]
struct LocalVertex {
    x: f32,
    y: f32,
    signed: f32,
    limit: f32,
    color: [f32; 4],
}

struct CachedMesh {
    alive: Weak<VectorPicture>,
    width: u32,
    height: u32,
    uv: [u16; 4],
    vertices: Vec<LocalVertex>,
    indices: Vec<u32>,
}

struct Stamp {
    origin_x: f32,
    origin_y: f32,
    clip_rect: [f32; 4],
    dest_rect: [f32; 4],
    scroll_clip: [f32; 4],
    radius: f32,
    opacity: f32,
}

struct VectorPipelines {
    shader: wgpu::ShaderModule,
    layout: wgpu::PipelineLayout,
    pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
}

impl VectorPipelines {
    fn new(device: &wgpu::Device, bind_group_layout: &wgpu::BindGroupLayout) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cappy vector shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(SHADER_SOURCE)),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cappy vector pipeline layout"),
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
            "fs_vector_encode_srgb"
        } else {
            "fs_vector"
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cappy vector pipeline"),
            layout: Some(&self.layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs_vector"),
                compilation_options: Default::default(),
                buffers: &[Some(VectorVertex::layout())],
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

/// Meshes for the vector primitives of one frame.
pub(crate) struct VectorLayer {
    bind_group: wgpu::BindGroup,
    pipelines: VectorPipelines,
    globals: wgpu::Buffer,
    vertices: GrowableBuffer,
    indices: GrowableBuffer,
    cache: HashMap<usize, CachedMesh>,
    ranges: Vec<std::ops::Range<u32>>,
}

impl VectorLayer {
    pub fn new(device: &wgpu::Device) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cappy vector layout"),
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
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cappy vector globals"),
            size: size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cappy vector globals bind group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        Self {
            pipelines: VectorPipelines::new(device, &bind_group_layout),
            bind_group,
            globals,
            vertices: GrowableBuffer::new(
                device,
                "cappy vector vertices",
                wgpu::BufferUsages::VERTEX,
            ),
            indices: GrowableBuffer::new(device, "cappy vector indices", wgpu::BufferUsages::INDEX),
            cache: HashMap::new(),
            ranges: Vec::new(),
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

        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        self.ranges.clear();
        for batch in batches {
            let start = indices.len() as u32;
            for &index in &batch.vectors {
                let Primitive::Vector(vector) = &primitives[index] else {
                    continue;
                };
                self.append_primitive(vector, scale_factor, &mut vertices, &mut indices);
            }
            self.ranges.push(start..indices.len() as u32);
        }
        self.vertices
            .write(device, queue, bytemuck::cast_slice(&vertices));
        self.indices
            .write(device, queue, bytemuck::cast_slice(&indices));
    }

    /// Draws this batch's vectors. Returns 1 when a draw was issued.
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        pass: &mut wgpu::RenderPass<'_>,
        format: wgpu::TextureFormat,
        batch_index: usize,
    ) -> usize {
        let Some(range) = self.ranges.get(batch_index).cloned() else {
            return 0;
        };
        if range.is_empty() {
            return 0;
        }
        let pipeline = self.pipelines.get(device, format);
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.buffer().slice(..));
        pass.set_index_buffer(self.indices.buffer().slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(range, 0, 0..1);
        1
    }

    fn append_primitive(
        &mut self,
        vector: &VectorPrimitive,
        scale_factor: f32,
        vertices: &mut Vec<VectorVertex>,
        indices: &mut Vec<u32>,
    ) {
        let Some(map) = PlacementMap::from_primitive(vector, scale_factor) else {
            return;
        };
        let mesh = self.mesh(&vector.picture, &map);
        if mesh.indices.is_empty() {
            return;
        }
        let base = vertices.len() as u32;
        let stamp = Stamp {
            origin_x: map.origin_x,
            origin_y: map.origin_y,
            clip_rect: rect_xywh(vector.rect.scale(scale_factor)),
            dest_rect: [map.origin_x, map.origin_y, map.dest_width, map.dest_height],
            scroll_clip: physical_clip(vector.clip, scale_factor),
            radius: vector.corner_radius * scale_factor,
            opacity: vector.opacity.clamp(0.0, 1.0),
        };
        for local in &mesh.vertices {
            vertices.push(stamp_vertex(local, &stamp));
        }
        indices.extend(mesh.indices.iter().map(|index| index + base));
    }

    fn mesh(&mut self, picture: &std::sync::Arc<VectorPicture>, map: &PlacementMap) -> &CachedMesh {
        let key = std::sync::Arc::as_ptr(picture) as usize;
        let width = map.dest_width.round() as u32;
        let height = map.dest_height.round() as u32;
        let uv = quantize_uv(map);
        let reusable = self.cache.get(&key).is_some_and(|cached| {
            cached.alive.upgrade().is_some()
                && cached.width == width
                && cached.height == height
                && cached.uv == uv
        });
        if !reusable {
            let (vertices, indices) = tessellate(picture, map);
            self.cache.insert(
                key,
                CachedMesh {
                    alive: std::sync::Arc::downgrade(picture),
                    width,
                    height,
                    uv,
                    vertices,
                    indices,
                },
            );
        }
        self.cache.get(&key).expect("the mesh was just inserted")
    }
}

fn stamp_vertex(local: &LocalVertex, stamp: &Stamp) -> VectorVertex {
    let mut color = local.color;
    color
        .iter_mut()
        .for_each(|channel| *channel *= stamp.opacity);
    VectorVertex {
        position: [local.x + stamp.origin_x, local.y + stamp.origin_y],
        edge: [local.signed, local.limit],
        color,
        clip_rect: stamp.clip_rect,
        dest_rect: stamp.dest_rect,
        scroll_clip: stamp.scroll_clip,
        radius: stamp.radius,
        _pad: [0.0; 3],
    }
}

fn rect_xywh(rect: Rect) -> [f32; 4] {
    [rect.x, rect.y, rect.width, rect.height]
}

fn quantize_uv(map: &PlacementMap) -> [u16; 4] {
    let quantize = |value: f32| (value.clamp(0.0, 1.0) * 10_000.0).round() as u16;
    [
        quantize(map.uv_min_x),
        quantize(map.uv_min_y),
        quantize(map.uv_max_x),
        quantize(map.uv_max_y),
    ]
}

struct PlacementMap {
    view_width: f32,
    view_height: f32,
    dest_width: f32,
    dest_height: f32,
    origin_x: f32,
    origin_y: f32,
    uv_min_x: f32,
    uv_min_y: f32,
    uv_max_x: f32,
    uv_max_y: f32,
}

impl PlacementMap {
    fn from_primitive(vector: &VectorPrimitive, scale_factor: f32) -> Option<Self> {
        let placement = vector.placement();
        let dest = placement.dest.scale(scale_factor);
        if dest.width < 0.5 || dest.height < 0.5 {
            return None;
        }
        Some(Self {
            view_width: vector.picture.view_width,
            view_height: vector.picture.view_height,
            dest_width: dest.width,
            dest_height: dest.height,
            origin_x: dest.x,
            origin_y: dest.y,
            uv_min_x: placement.uv_min_x,
            uv_min_y: placement.uv_min_y,
            uv_max_x: placement.uv_max_x,
            uv_max_y: placement.uv_max_y,
        })
    }

    fn to_physical(&self, point: Point) -> [f32; 2] {
        let u = point.x / self.view_width;
        let v = point.y / self.view_height;
        let span_u = (self.uv_max_x - self.uv_min_x).max(1.0e-6);
        let span_v = (self.uv_max_y - self.uv_min_y).max(1.0e-6);
        [
            (u - self.uv_min_x) / span_u * self.dest_width,
            (v - self.uv_min_y) / span_v * self.dest_height,
        ]
    }

    fn to_view(&self, x: f32, y: f32) -> [f32; 2] {
        let span_u = (self.uv_max_x - self.uv_min_x).max(1.0e-6);
        let span_v = (self.uv_max_y - self.uv_min_y).max(1.0e-6);
        let u = self.uv_min_x + x / self.dest_width.max(1.0e-6) * span_u;
        let v = self.uv_min_y + y / self.dest_height.max(1.0e-6) * span_v;
        [u * self.view_width, v * self.view_height]
    }

    fn axis_scale(&self) -> (f32, f32) {
        let span_u = (self.uv_max_x - self.uv_min_x).max(1.0e-6);
        let span_v = (self.uv_max_y - self.uv_min_y).max(1.0e-6);
        (
            self.dest_width / (span_u * self.view_width),
            self.dest_height / (span_v * self.view_height),
        )
    }
}

fn tessellate(picture: &VectorPicture, map: &PlacementMap) -> (Vec<LocalVertex>, Vec<u32>) {
    let mut buffers: VertexBuffers<LocalVertex, u32> = VertexBuffers::new();
    let mut fill = FillTessellator::new();
    let mut stroke = StrokeTessellator::new();
    for draw in picture.draws.iter() {
        if draw.opacity <= 0.0 {
            continue;
        }
        let built = build_paths(&draw.path, map);
        match &draw.style {
            VectorStyle::Fill { paint, rule } => {
                tessellate_fill(
                    &mut fill,
                    &built.fill,
                    paint,
                    draw.opacity,
                    *rule,
                    map,
                    &mut buffers,
                );
                tessellate_fringe(
                    &mut stroke,
                    &built.contours,
                    paint,
                    draw.opacity,
                    map,
                    &mut buffers,
                );
            }
            VectorStyle::Stroke {
                paint,
                width,
                cap,
                join,
                miter_limit,
            } => {
                let (scale_x, scale_y) = map.axis_scale();
                let width = width * (scale_x + scale_y) * 0.5;
                if width < 0.05 {
                    continue;
                }
                tessellate_stroke(
                    &mut stroke,
                    &built.fill,
                    StrokeDesc {
                        paint,
                        opacity: draw.opacity,
                        width,
                        cap: *cap,
                        join: *join,
                        miter_limit: *miter_limit,
                    },
                    map,
                    &mut buffers,
                );
            }
        }
    }
    (buffers.vertices, buffers.indices)
}

fn tessellate_fill(
    tessellator: &mut FillTessellator,
    path: &Path,
    paint: &VectorPaint,
    opacity: f32,
    rule: FillRule,
    map: &PlacementMap,
    buffers: &mut VertexBuffers<LocalVertex, u32>,
) {
    let options = FillOptions::tolerance(TOLERANCE).with_fill_rule(match rule {
        FillRule::NonZero => lyon_tessellation::FillRule::NonZero,
        FillRule::EvenOdd => lyon_tessellation::FillRule::EvenOdd,
    });
    let mark = (buffers.vertices.len(), buffers.indices.len());
    let mut builder = BuffersBuilder::new(
        buffers,
        PaintVertices {
            paint,
            opacity,
            map,
            signed: 1.0,
            limit: 0.0,
        },
    );
    if tessellator
        .tessellate_path(path, &options, &mut builder)
        .is_err()
    {
        buffers.vertices.truncate(mark.0);
        buffers.indices.truncate(mark.1);
    }
}

fn tessellate_fringe(
    tessellator: &mut StrokeTessellator,
    contours: &[(Path, bool)],
    paint: &VectorPaint,
    opacity: f32,
    map: &PlacementMap,
    buffers: &mut VertexBuffers<LocalVertex, u32>,
) {
    let options = StrokeOptions::tolerance(TOLERANCE)
        .with_line_width(FRINGE_WIDTH)
        .with_line_cap(LyonCap::Butt)
        .with_line_join(LyonJoin::Round)
        .with_miter_limit(1.0);
    for (contour, outside_positive) in contours {
        let mark = (buffers.vertices.len(), buffers.indices.len());
        let mut builder = BuffersBuilder::new(
            buffers,
            FringeVertices {
                paint,
                opacity,
                map,
                outside_positive: *outside_positive,
            },
        );
        if tessellator
            .tessellate_path(contour, &options, &mut builder)
            .is_err()
        {
            buffers.vertices.truncate(mark.0);
            buffers.indices.truncate(mark.1);
        }
    }
}

struct StrokeDesc<'a> {
    paint: &'a VectorPaint,
    opacity: f32,
    width: f32,
    cap: LineCap,
    join: LineJoin,
    miter_limit: f32,
}

fn tessellate_stroke(
    tessellator: &mut StrokeTessellator,
    path: &Path,
    stroke: StrokeDesc<'_>,
    map: &PlacementMap,
    buffers: &mut VertexBuffers<LocalVertex, u32>,
) {
    let limit = if stroke.miter_limit.is_finite() {
        stroke.miter_limit.max(1.0)
    } else {
        4.0
    };
    let options = StrokeOptions::tolerance(TOLERANCE)
        .with_line_width(stroke.width)
        .with_line_cap(match stroke.cap {
            LineCap::Butt => LyonCap::Butt,
            LineCap::Round => LyonCap::Round,
            LineCap::Square => LyonCap::Square,
        })
        .with_line_join(match stroke.join {
            LineJoin::Miter => LyonJoin::Miter,
            LineJoin::MiterClip => LyonJoin::MiterClip,
            LineJoin::Round => LyonJoin::Round,
            LineJoin::Bevel => LyonJoin::Bevel,
        })
        .with_miter_limit(limit);
    let mark = (buffers.vertices.len(), buffers.indices.len());
    let mut builder = BuffersBuilder::new(
        buffers,
        StrokeVertices {
            paint: stroke.paint,
            opacity: stroke.opacity,
            map,
        },
    );
    if tessellator
        .tessellate_path(path, &options, &mut builder)
        .is_err()
    {
        buffers.vertices.truncate(mark.0);
        buffers.indices.truncate(mark.1);
    }
}

struct PaintVertices<'a> {
    paint: &'a VectorPaint,
    opacity: f32,
    map: &'a PlacementMap,
    signed: f32,
    limit: f32,
}

impl FillVertexConstructor<LocalVertex> for PaintVertices<'_> {
    fn new_vertex(&mut self, vertex: FillVertex) -> LocalVertex {
        let position = vertex.position();
        colored(
            self.paint,
            self.opacity,
            self.map,
            position.x,
            position.y,
            self.signed,
            self.limit,
        )
    }
}

struct FringeVertices<'a> {
    paint: &'a VectorPaint,
    opacity: f32,
    map: &'a PlacementMap,
    outside_positive: bool,
}

impl StrokeVertexConstructor<LocalVertex> for FringeVertices<'_> {
    fn new_vertex(&mut self, vertex: StrokeVertex) -> LocalVertex {
        let position = vertex.position();
        let outside = match vertex.side() {
            Side::Positive => self.outside_positive,
            Side::Negative => !self.outside_positive,
        };
        let signed = if outside { 0.0 } else { 1.0 };
        colored(
            self.paint,
            self.opacity,
            self.map,
            position.x,
            position.y,
            signed,
            0.0,
        )
    }
}

struct StrokeVertices<'a> {
    paint: &'a VectorPaint,
    opacity: f32,
    map: &'a PlacementMap,
}

impl StrokeVertexConstructor<LocalVertex> for StrokeVertices<'_> {
    fn new_vertex(&mut self, vertex: StrokeVertex) -> LocalVertex {
        let position = vertex.position();
        let half = vertex.line_width() * 0.5;
        colored(
            self.paint,
            self.opacity,
            self.map,
            position.x,
            position.y,
            vertex.side().to_f32() * half,
            half,
        )
    }
}

fn colored(
    paint: &VectorPaint,
    opacity: f32,
    map: &PlacementMap,
    x: f32,
    y: f32,
    signed: f32,
    limit: f32,
) -> LocalVertex {
    let [view_x, view_y] = map.to_view(x, y);
    LocalVertex {
        x,
        y,
        signed,
        limit,
        color: paint_color(paint, view_x, view_y, opacity),
    }
}

fn paint_color(paint: &VectorPaint, x: f32, y: f32, opacity: f32) -> [f32; 4] {
    let color = match paint {
        VectorPaint::Solid(color) => *color,
        VectorPaint::Linear { start, end, stops } => sample_linear(*start, *end, stops, x, y),
    };
    Color {
        a: color.a * opacity,
        ..color
    }
    .to_linear_premultiplied()
}

fn sample_linear(start: Point, end: Point, stops: &[(f32, Color)], x: f32, y: f32) -> Color {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let length = dx * dx + dy * dy;
    let t = if length <= 1.0e-8 {
        0.0
    } else {
        ((x - start.x) * dx + (y - start.y) * dy) / length
    };
    let t = t.clamp(0.0, 1.0);
    if stops.is_empty() {
        return Color::TRANSPARENT;
    }
    if t <= stops[0].0 {
        return stops[0].1;
    }
    for pair in stops.windows(2) {
        let (offset, color) = pair[0];
        let (next_offset, next) = pair[1];
        if t <= next_offset {
            let span = (next_offset - offset).max(1.0e-6);
            return lerp_color(color, next, (t - offset) / span);
        }
    }
    stops[stops.len() - 1].1
}

fn lerp_color(from: Color, to: Color, t: f32) -> Color {
    Color {
        r: from.r + (to.r - from.r) * t,
        g: from.g + (to.g - from.g) * t,
        b: from.b + (to.b - from.b) * t,
        a: from.a + (to.a - from.a) * t,
    }
}

struct BuiltPaths {
    fill: Path,
    contours: Vec<(Path, bool)>,
}

fn build_paths(path: &VectorPath, map: &PlacementMap) -> BuiltPaths {
    let mut builder = PathBuilder::default();
    let mut points = path.points.iter().copied();
    for verb in path.verbs.iter() {
        match verb {
            PathVerb::Move => {
                let Some(point) = points.next() else { break };
                builder.begin(map.to_physical(point));
            }
            PathVerb::Line => {
                let Some(point) = points.next() else { break };
                builder.line(map.to_physical(point));
            }
            PathVerb::Quad => {
                let (Some(control), Some(end)) = (points.next(), points.next()) else {
                    break;
                };
                builder.quad(map.to_physical(control), map.to_physical(end));
            }
            PathVerb::Cubic => {
                let (Some(control_a), Some(control_b), Some(end)) =
                    (points.next(), points.next(), points.next())
                else {
                    break;
                };
                builder.cubic(
                    map.to_physical(control_a),
                    map.to_physical(control_b),
                    map.to_physical(end),
                );
            }
            PathVerb::Close => builder.close_contour(),
        }
    }
    builder.finish()
}

#[derive(Default)]
struct PathBuilder {
    fill: Option<lyon_path::path::Builder>,
    contour: Option<lyon_path::path::Builder>,
    contours: Vec<(Path, bool)>,
    area: f32,
    start: Option<[f32; 2]>,
    previous: Option<[f32; 2]>,
}

impl PathBuilder {
    fn begin(&mut self, point: [f32; 2]) {
        self.end_contour(false);
        self.fill_builder().begin(lyon_point(point));
        self.contour_builder().begin(lyon_point(point));
        self.area = 0.0;
        self.start = Some(point);
        self.previous = Some(point);
    }

    fn line(&mut self, point: [f32; 2]) {
        if self.start.is_none() {
            self.begin(point);
            return;
        }
        self.edge(point);
        self.fill_builder().line_to(lyon_point(point));
        self.contour_builder().line_to(lyon_point(point));
    }

    fn quad(&mut self, control: [f32; 2], end: [f32; 2]) {
        if self.start.is_none() {
            self.begin(end);
            return;
        }
        self.edge(end);
        self.fill_builder()
            .quadratic_bezier_to(lyon_point(control), lyon_point(end));
        self.contour_builder()
            .quadratic_bezier_to(lyon_point(control), lyon_point(end));
    }

    fn cubic(&mut self, control_a: [f32; 2], control_b: [f32; 2], end: [f32; 2]) {
        if self.start.is_none() {
            self.begin(end);
            return;
        }
        self.edge(end);
        self.fill_builder().cubic_bezier_to(
            lyon_point(control_a),
            lyon_point(control_b),
            lyon_point(end),
        );
        self.contour_builder().cubic_bezier_to(
            lyon_point(control_a),
            lyon_point(control_b),
            lyon_point(end),
        );
    }

    fn close_contour(&mut self) {
        self.end_contour(true);
    }

    fn edge(&mut self, point: [f32; 2]) {
        if let Some(previous) = self.previous {
            self.area += previous[0] * point[1] - point[0] * previous[1];
        }
        self.previous = Some(point);
    }

    fn end_contour(&mut self, close: bool) {
        if self.start.is_none() {
            return;
        }
        if close && let (Some(previous), Some(start)) = (self.previous, self.start) {
            self.area += previous[0] * start[1] - start[0] * previous[1];
        }
        self.fill_builder().end(close);
        self.contour_builder().end(close);
        let outside_positive = self.area < 0.0;
        let finished = self.contour.take().expect("contour is open");
        self.contours.push((finished.build(), outside_positive));
        self.start = None;
        self.previous = None;
        self.area = 0.0;
    }

    fn finish(mut self) -> BuiltPaths {
        self.end_contour(false);
        let fill = self
            .fill
            .take()
            .map(lyon_path::path::Builder::build)
            .unwrap_or_default();
        BuiltPaths {
            fill,
            contours: self.contours,
        }
    }

    fn fill_builder(&mut self) -> &mut lyon_path::path::Builder {
        self.fill.get_or_insert_with(Path::builder)
    }

    fn contour_builder(&mut self) -> &mut lyon_path::path::Builder {
        self.contour.get_or_insert_with(Path::builder)
    }
}

fn lyon_point(point: [f32; 2]) -> lyon_path::math::Point {
    lyon_path::math::point(point[0], point[1])
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use cappy_core::vector::{
        FillRule, PathVerb, VectorDraw, VectorPaint, VectorPath, VectorPicture, VectorStyle,
    };
    use cappy_core::{Color, ObjectFit, Point, Rect, VectorPrimitive};

    use super::*;

    #[test]
    fn a_filled_rect_tessellates_inside_its_box() {
        let picture = Arc::new(VectorPicture {
            view_width: 10.0,
            view_height: 10.0,
            draws: Arc::from([VectorDraw {
                path: VectorPath {
                    verbs: Arc::from([
                        PathVerb::Move,
                        PathVerb::Line,
                        PathVerb::Line,
                        PathVerb::Line,
                        PathVerb::Close,
                    ]),
                    points: Arc::from([
                        Point::new(0.0, 0.0),
                        Point::new(10.0, 0.0),
                        Point::new(10.0, 10.0),
                        Point::new(0.0, 10.0),
                    ]),
                },
                style: VectorStyle::Fill {
                    paint: VectorPaint::Solid(Color::hex("#ff0000")),
                    rule: FillRule::NonZero,
                },
                opacity: 1.0,
            }]),
        });
        let primitive = VectorPrimitive {
            rect: Rect::new(0.0, 0.0, 40.0, 40.0),
            picture,
            fit: ObjectFit::Fill,
            corner_radius: 0.0,
            clip: None,
            opacity: 1.0,
        };
        let map = PlacementMap::from_primitive(&primitive, 1.0).expect("placement");
        let (vertices, indices) = tessellate(&primitive.picture, &map);
        assert!(indices.len() >= 6, "fill should emit triangles");
        let min_x = vertices
            .iter()
            .map(|vertex| vertex.x)
            .fold(f32::INFINITY, f32::min);
        let max_x = vertices
            .iter()
            .map(|vertex| vertex.x)
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(min_x < 1.0 && max_x > 39.0, "rect span {min_x}..{max_x}");
        assert!(
            vertices
                .iter()
                .any(|vertex| vertex.signed > 0.5 && vertex.limit == 0.0)
        );
    }
}
