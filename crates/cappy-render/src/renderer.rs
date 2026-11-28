//! Paints [`RenderFrame`]s into a persistent offscreen texture, repainting only damaged regions,
//! and composites that texture onto whatever target the host provides.

use std::fmt;
use std::ops::Range;

use cappy_core::{Color, Damage, Primitive, Rect, RenderFrame, coalesce_regions};
use cappy_host::{PaintStats, PixelRect, coverage_percent, physical_regions};

use crate::FontContext;
use crate::batch::{Batch, plan_batches};
use crate::composite::{Compositor, OFFSCREEN_FORMAT, Offscreen};
use crate::image::ImageLayer;
use crate::quad::{QuadBatch, QuadBlend, QuadInstance, QuadPipelines, needs_manual_srgb};
use crate::text::TextLayer;
use crate::vector::VectorLayer;

/// Each scissor redraws every batch, so a long list of regions becomes their bounding box.
const MAX_GPU_REGIONS: usize = 8;
const DAMAGE_OVERLAY_FILL: Color = Color::rgba(255, 40, 80, 40);
const DAMAGE_OVERLAY_BORDER: Color = Color::rgba(255, 40, 80, 220);
const DAMAGE_OVERLAY_BORDER_WIDTH: f32 = 2.0;

/// How the composite pass treats the target's existing contents.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TargetLoad {
    /// Clear to a color first; for hosts where the UI owns the whole surface.
    Clear(Color),
    /// Draw over what is there; for hosts that rendered their own content underneath.
    Load,
}

/// The texture the UI is composited onto.
#[derive(Clone, Copy, Debug)]
pub struct RenderTarget<'a> {
    pub view: &'a wgpu::TextureView,
    pub format: wgpu::TextureFormat,
}

#[derive(Debug)]
pub enum RenderError {
    PrepareText(glyphon::PrepareError),
    DrawText(glyphon::RenderError),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PrepareText(error) => write!(f, "failed to prepare text for drawing: {error}"),
            Self::DrawText(error) => write!(f, "failed to draw text: {error}"),
        }
    }
}

impl std::error::Error for RenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::PrepareText(error) => Some(error),
            Self::DrawText(error) => Some(error),
        }
    }
}

impl From<glyphon::PrepareError> for RenderError {
    fn from(error: glyphon::PrepareError) -> Self {
        Self::PrepareText(error)
    }
}

impl From<glyphon::RenderError> for RenderError {
    fn from(error: glyphon::RenderError) -> Self {
        Self::DrawText(error)
    }
}

struct PaintRegions<'a> {
    frame_size: (u32, u32),
    scissors: &'a [PixelRect],
    quad_ranges: &'a [Range<u32>],
    selection_count: u32,
    caret_count: u32,
}

pub struct Renderer {
    quad_pipelines: QuadPipelines,
    images: ImageLayer,
    vectors: VectorLayer,
    compositor: Compositor,
    scene_quads: QuadBatch,
    selection_quads: QuadBatch,
    caret_quads: QuadBatch,
    overlay_quads: QuadBatch,
    text: TextLayer,
    offscreen: Option<Offscreen>,
    /// Frame size of the latest paint. The offscreen texture may be larger than this.
    viewport_size: (u32, u32),
    debug_damage: bool,
    /// Count paint-pass draw calls. Off unless a debug-stats binding is subscribed.
    collect_stats: bool,
    /// Physical rectangles repainted by the latest paint that had damage.
    last_repainted: Vec<Rect>,
    /// Counts from the latest paint that was asked to collect them.
    paint_stats: PaintStats,
}

impl Renderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, fonts: &FontContext) -> Self {
        let quad_pipelines = QuadPipelines::new(device);
        Self {
            scene_quads: QuadBatch::new(device, &quad_pipelines, "cappy scene quads"),
            selection_quads: QuadBatch::new(device, &quad_pipelines, "cappy text selection"),
            caret_quads: QuadBatch::new(device, &quad_pipelines, "cappy text carets"),
            overlay_quads: QuadBatch::new(device, &quad_pipelines, "cappy overlay quads"),
            images: ImageLayer::new(device),
            vectors: VectorLayer::new(device),
            quad_pipelines,
            compositor: Compositor::new(device),
            text: TextLayer::new(device, queue, fonts, OFFSCREEN_FORMAT),
            offscreen: None,
            viewport_size: (1, 1),
            debug_damage: false,
            collect_stats: false,
            last_repainted: Vec::new(),
            paint_stats: PaintStats::default(),
        }
    }

    /// Outlines the regions repainted by the latest paint when compositing.
    pub fn set_debug_damage(&mut self, enabled: bool) {
        self.debug_damage = enabled;
    }

    pub fn debug_damage(&self) -> bool {
        self.debug_damage
    }

    pub fn set_collect_stats(&mut self, enabled: bool) {
        self.collect_stats = enabled;
    }

    pub fn paint_stats(&self) -> PaintStats {
        self.paint_stats
    }

    /// Paints `frame` and composites it onto `target`.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &RenderFrame,
        target: RenderTarget<'_>,
        load: TargetLoad,
    ) -> Result<usize, RenderError> {
        let repainted = self.paint(device, queue, encoder, frame)?;
        self.composite(device, queue, encoder, target, load);
        Ok(repainted)
    }

    /// Repaints the damaged regions of the offscreen texture. Returns how many regions were
    /// repainted; zero means the frame had no damage and nothing was drawn.
    pub fn paint(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &RenderFrame,
    ) -> Result<usize, RenderError> {
        let size = (frame.physical_width.max(1), frame.physical_height.max(1));
        self.viewport_size = size;
        let recreated = self
            .compositor
            .ensure_offscreen(device, &mut self.offscreen, size);
        let damage = if recreated {
            &Damage::Full
        } else {
            &frame.damage
        };
        let logical_damage = coalesce_regions(damage.rects(frame.logical_size()), MAX_GPU_REGIONS);
        let scissors = physical_regions(&logical_damage, frame.scale_factor, size);
        if scissors.is_empty() {
            return Ok(0);
        }

        let batches = plan_batches(&frame.primitives, &logical_damage);
        let (instances, quad_ranges) =
            quad_instances(&frame.primitives, &batches, &scissors, frame.scale_factor);
        self.scene_quads.upload(device, queue, size, &instances);
        self.images.prepare(
            device,
            queue,
            size,
            frame.scale_factor,
            &frame.primitives,
            &batches,
        );
        self.vectors.prepare(
            device,
            queue,
            size,
            frame.scale_factor,
            &frame.primitives,
            &batches,
        );
        self.text.prepare(
            device,
            queue,
            size,
            frame.scale_factor,
            &frame.primitives,
            &batches,
        )?;
        let (selection, carets) = self.text.decorations(&frame.primitives);
        let selection_count = upload_quads(
            &mut self.selection_quads,
            device,
            queue,
            size,
            &selection,
            frame.scale_factor,
        );
        let caret_count = upload_quads(
            &mut self.caret_quads,
            device,
            queue,
            size,
            &carets,
            frame.scale_factor,
        );
        let draw_calls = self.draw_regions(
            device,
            encoder,
            PaintRegions {
                frame_size: size,
                scissors: &scissors,
                quad_ranges: &quad_ranges,
                selection_count,
                caret_count,
            },
        )?;
        self.text.finish_paint();

        if self.collect_stats {
            let covered = scissors
                .iter()
                .map(|scissor| u64::from(scissor.width) * u64::from(scissor.height))
                .sum();
            let viewport = u64::from(size.0) * u64::from(size.1);
            self.paint_stats = PaintStats {
                batches: batches.len(),
                draw_calls,
                regions: scissors.len(),
                coverage_percent: coverage_percent(covered, viewport),
            };
        }
        self.last_repainted = scissors.iter().map(|scissor| scissor.to_rect()).collect();
        Ok(scissors.len())
    }

    /// Copies the offscreen texture onto `target`, plus the damage outlines when enabled.
    pub fn composite(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: RenderTarget<'_>,
        load: TargetLoad,
    ) {
        let instances = self.overlay_instances();
        let overlay_pipeline = if instances.is_empty() {
            None
        } else {
            let count = u32::try_from(instances.len()).unwrap_or(u32::MAX);
            self.overlay_quads
                .upload(device, queue, self.viewport_size, &instances);
            Some((
                self.quad_pipelines
                    .get(device, target.format, QuadBlend::Over),
                count,
            ))
        };
        let composite_pipeline = self.compositor.pipeline(device, target.format);
        let mut pass = begin_pass(
            encoder,
            "cappy composite",
            target.view,
            load_op(load, target.format),
        );
        if let Some(offscreen) = &self.offscreen {
            Compositor::draw(&mut pass, &composite_pipeline, offscreen);
        }
        if let Some((pipeline, count)) = overlay_pipeline {
            self.overlay_quads.draw(&mut pass, &pipeline, 0..count);
        }
    }

    fn draw_regions(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        regions: PaintRegions<'_>,
    ) -> Result<usize, RenderError> {
        let PaintRegions {
            frame_size,
            scissors,
            quad_ranges,
            selection_count,
            caret_count,
        } = regions;
        let eraser = self
            .quad_pipelines
            .get(device, OFFSCREEN_FORMAT, QuadBlend::Replace);
        let painter = self
            .quad_pipelines
            .get(device, OFFSCREEN_FORMAT, QuadBlend::Over);
        let offscreen = self
            .offscreen
            .as_ref()
            .expect("paint creates the offscreen texture");
        let mut pass = begin_pass(encoder, "cappy paint", &offscreen.view, wgpu::LoadOp::Load);
        // Quads and text map pixels through this frame. The texture can be larger, so the pass
        // viewport has to be the frame or the content lands past the rectangle the composite reads.
        pass.set_viewport(0.0, 0.0, frame_size.0 as f32, frame_size.1 as f32, 0.0, 1.0);
        let collect = self.collect_stats;
        let mut draws = 0;
        for (index, scissor) in (0u32..).zip(scissors) {
            pass.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height);
            self.scene_quads.draw(&mut pass, &eraser, index..index + 1);
            draws += usize::from(collect);
            for (batch_index, range) in quad_ranges.iter().enumerate() {
                if !range.is_empty() {
                    self.scene_quads.draw(&mut pass, &painter, range.clone());
                    draws += usize::from(collect);
                }
                let images = self
                    .images
                    .draw(device, &mut pass, OFFSCREEN_FORMAT, batch_index);
                let vectors = self
                    .vectors
                    .draw(device, &mut pass, OFFSCREEN_FORMAT, batch_index);
                if collect {
                    draws += images + vectors;
                }
                if selection_count > 0 {
                    self.selection_quads
                        .draw(&mut pass, &painter, 0..selection_count);
                    draws += usize::from(collect);
                }
                self.text.render(batch_index, &mut pass)?;
                if collect && self.text.has_text(batch_index) {
                    draws += 1;
                }
                if caret_count > 0 {
                    self.caret_quads.draw(&mut pass, &painter, 0..caret_count);
                    draws += usize::from(collect);
                }
            }
        }
        Ok(draws)
    }

    fn overlay_instances(&self) -> Vec<QuadInstance> {
        if !self.debug_damage {
            return Vec::new();
        }
        self.last_repainted
            .iter()
            .map(|rect| {
                QuadInstance::new(
                    *rect,
                    DAMAGE_OVERLAY_FILL,
                    DAMAGE_OVERLAY_BORDER,
                    DAMAGE_OVERLAY_BORDER_WIDTH,
                    0.0,
                )
            })
            .collect()
    }
}

/// Instance list: one eraser per scissor first, then each batch's quads.
/// Returns the instances and, per batch, the range holding its quads.
fn upload_quads(
    batch: &mut QuadBatch,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: (u32, u32),
    quads: &[cappy_core::QuadPrimitive],
    scale_factor: f32,
) -> u32 {
    let instances: Vec<QuadInstance> = quads
        .iter()
        .filter_map(|quad| {
            QuadInstance::from_primitive(&Primitive::Quad(quad.clone()), scale_factor)
        })
        .collect();
    let count = instances.len() as u32;
    batch.upload(device, queue, size, &instances);
    count
}

fn quad_instances(
    primitives: &[Primitive],
    batches: &[Batch],
    scissors: &[PixelRect],
    scale_factor: f32,
) -> (Vec<QuadInstance>, Vec<Range<u32>>) {
    let mut instances: Vec<QuadInstance> = scissors
        .iter()
        .map(|scissor| QuadInstance::eraser(scissor.to_rect()))
        .collect();
    let mut ranges = Vec::with_capacity(batches.len());
    for batch in batches {
        let start = instances.len() as u32;
        instances.extend(
            batch.quads.iter().filter_map(|&index| {
                QuadInstance::from_primitive(&primitives[index], scale_factor)
            }),
        );
        ranges.push(start..instances.len() as u32);
    }
    (instances, ranges)
}

fn load_op(load: TargetLoad, format: wgpu::TextureFormat) -> wgpu::LoadOp<wgpu::Color> {
    match load {
        TargetLoad::Load => wgpu::LoadOp::Load,
        TargetLoad::Clear(color) => {
            let [red, green, blue, alpha] = if needs_manual_srgb(format) {
                [
                    color.r * color.a,
                    color.g * color.a,
                    color.b * color.a,
                    color.a,
                ]
            } else {
                color.to_linear_premultiplied()
            };
            wgpu::LoadOp::Clear(wgpu::Color {
                r: red.into(),
                g: green.into(),
                b: blue.into(),
                a: alpha.into(),
            })
        }
    }
}

fn begin_pass<'encoder>(
    encoder: &'encoder mut wgpu::CommandEncoder,
    label: &'static str,
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPass<'encoder> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}
