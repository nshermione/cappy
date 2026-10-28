//! Paints [`RenderFrame`]s into a persistent offscreen texture, repainting only damaged regions,
//! and composites that texture onto whatever target the host provides.

use std::fmt;
use std::ops::Range;

use cappy_core::{Color, Damage, Primitive, Rect, RenderFrame};

use crate::FontContext;
use crate::batch::{Batch, plan_batches};
use crate::composite::{Compositor, OFFSCREEN_FORMAT, Offscreen};
use crate::quad::{QuadBatch, QuadBlend, QuadInstance, QuadPipelines, needs_manual_srgb};
use crate::scissor::{ScissorRect, physical_scissors};
use crate::text::TextLayer;

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

pub struct Renderer {
    quad_pipelines: QuadPipelines,
    compositor: Compositor,
    scene_quads: QuadBatch,
    overlay_quads: QuadBatch,
    text: TextLayer,
    offscreen: Option<Offscreen>,
    debug_damage: bool,
    /// Physical rectangles repainted by the latest paint that had damage.
    last_repainted: Vec<Rect>,
}

impl Renderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, fonts: &FontContext) -> Self {
        let quad_pipelines = QuadPipelines::new(device);
        Self {
            scene_quads: QuadBatch::new(device, &quad_pipelines, "cappy scene quads"),
            overlay_quads: QuadBatch::new(device, &quad_pipelines, "cappy overlay quads"),
            quad_pipelines,
            compositor: Compositor::new(device),
            text: TextLayer::new(device, queue, fonts, OFFSCREEN_FORMAT),
            offscreen: None,
            debug_damage: false,
            last_repainted: Vec::new(),
        }
    }

    /// Outlines the regions repainted by the latest paint when compositing.
    pub fn set_debug_damage(&mut self, enabled: bool) {
        self.debug_damage = enabled;
    }

    pub fn debug_damage(&self) -> bool {
        self.debug_damage
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
        let recreated = self
            .compositor
            .ensure_offscreen(device, &mut self.offscreen, size);
        let damage = if recreated {
            &Damage::Full
        } else {
            &frame.damage
        };
        let logical_damage = damage.rects(frame.logical_size());
        let scissors = physical_scissors(&logical_damage, frame.scale_factor, size);
        if scissors.is_empty() {
            return Ok(0);
        }

        let batches = plan_batches(&frame.primitives, &logical_damage);
        let (instances, quad_ranges) =
            quad_instances(&frame.primitives, &batches, &scissors, frame.scale_factor);
        self.scene_quads.upload(device, queue, size, &instances);
        self.text.prepare(
            device,
            queue,
            size,
            frame.scale_factor,
            &frame.primitives,
            &batches,
        )?;
        self.draw_regions(device, encoder, &scissors, &quad_ranges)?;
        self.text.finish_paint();

        self.last_repainted = scissors.iter().map(|scissor| scissor.to_rect()).collect();
        Ok(scissors.len())
    }

    /// Copies the offscreen texture onto `target`, plus the damage overlay when enabled.
    pub fn composite(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: RenderTarget<'_>,
        load: TargetLoad,
    ) {
        let composite_pipeline = self.compositor.pipeline(device, target.format);
        let overlay = self.prepare_overlay(device, queue, target.format);
        let mut pass = begin_pass(
            encoder,
            "cappy composite",
            target.view,
            load_op(load, target.format),
        );
        if let Some(offscreen) = &self.offscreen {
            Compositor::draw(&mut pass, &composite_pipeline, offscreen);
        }
        if let Some((pipeline, count)) = overlay {
            self.overlay_quads.draw(&mut pass, &pipeline, 0..count);
        }
    }

    fn draw_regions(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        scissors: &[ScissorRect],
        quad_ranges: &[Range<u32>],
    ) -> Result<(), RenderError> {
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
        for (index, scissor) in (0u32..).zip(scissors) {
            pass.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height);
            self.scene_quads.draw(&mut pass, &eraser, index..index + 1);
            for (batch_index, range) in quad_ranges.iter().enumerate() {
                self.scene_quads.draw(&mut pass, &painter, range.clone());
                self.text.render(batch_index, &mut pass)?;
            }
        }
        Ok(())
    }

    fn prepare_overlay(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Option<(wgpu::RenderPipeline, u32)> {
        let offscreen = self.offscreen.as_ref()?;
        if !self.debug_damage || self.last_repainted.is_empty() {
            return None;
        }
        let instances: Vec<QuadInstance> = self
            .last_repainted
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
            .collect();
        self.overlay_quads
            .upload(device, queue, offscreen.size, &instances);
        let pipeline = self.quad_pipelines.get(device, format, QuadBlend::Over);
        Some((pipeline, instances.len() as u32))
    }
}

/// Instance list: one eraser per scissor first, then each batch's quads.
/// Returns the instances and, per batch, the range holding its quads.
fn quad_instances(
    primitives: &[Primitive],
    batches: &[Batch],
    scissors: &[ScissorRect],
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
