//! Text drawing with glyphon: one text renderer per batch, shaped buffers cached across frames.

use std::collections::HashMap;
use std::sync::Arc;

use cappy_core::{Primitive, TextPrimitive};
use glyphon::{
    Buffer, Cache, FontSystem, PrepareError, RenderError, Resolution, SwashCache, TextArea,
    TextAtlas, TextBounds, TextRenderer, Viewport,
};

use crate::FontContext;
use crate::batch::Batch;
use crate::shaping::{TextFont, shape_text};

/// Shaped buffers unused for this many paints are dropped.
const BUFFER_TIME_TO_LIVE: u64 = 600;
/// Extra wrap width so text measured to an exact pixel width does not wrap when drawn.
const WRAP_SLACK: f32 = 0.5;

#[derive(Clone, PartialEq, Eq, Hash)]
struct BufferKey {
    text: Arc<str>,
    font_size_bits: u32,
    line_height_bits: u32,
    weight: u16,
    width_bits: u32,
}

impl BufferKey {
    fn new(text: &TextPrimitive) -> Self {
        Self {
            text: text.text.clone(),
            font_size_bits: text.font_size.to_bits(),
            line_height_bits: text.line_height.to_bits(),
            weight: text.weight,
            width_bits: text.rect.width.to_bits(),
        }
    }
}

struct CachedBuffer {
    buffer: Buffer,
    last_used: u64,
}

pub(crate) struct TextLayer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    renderers: Vec<TextRenderer>,
    buffers: HashMap<BufferKey, CachedBuffer>,
    paint_count: u64,
}

impl TextLayer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        fonts: &FontContext,
        format: wgpu::TextureFormat,
    ) -> Self {
        let cache = Cache::new(device);
        Self {
            font_system: fonts.font_system(),
            swash_cache: SwashCache::new(),
            viewport: Viewport::new(device, &cache),
            atlas: TextAtlas::new(device, queue, &cache, format),
            renderers: Vec::new(),
            buffers: HashMap::new(),
            paint_count: 0,
        }
    }

    /// Shapes and uploads the text of every batch. Call before [`TextLayer::render`].
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        resolution: (u32, u32),
        scale_factor: f32,
        primitives: &[Primitive],
        batches: &[Batch],
    ) -> Result<(), PrepareError> {
        self.paint_count += 1;
        self.viewport.update(
            queue,
            Resolution {
                width: resolution.0,
                height: resolution.1,
            },
        );
        for batch in batches {
            for text in batch_texts(primitives, batch) {
                self.ensure_buffer(text);
            }
        }
        while self.renderers.len() < batches.len() {
            let renderer = TextRenderer::new(
                &mut self.atlas,
                device,
                wgpu::MultisampleState::default(),
                None,
            );
            self.renderers.push(renderer);
        }
        for (batch, renderer) in batches.iter().zip(&mut self.renderers) {
            let areas = batch_texts(primitives, batch).map(|text| {
                text_area(
                    &self.buffers[&BufferKey::new(text)].buffer,
                    text,
                    scale_factor,
                )
            });
            renderer.prepare(
                device,
                queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                areas,
                &mut self.swash_cache,
            )?;
        }
        Ok(())
    }

    pub fn render(
        &self,
        batch_index: usize,
        pass: &mut wgpu::RenderPass<'_>,
    ) -> Result<(), RenderError> {
        self.renderers[batch_index].render(&self.atlas, &self.viewport, pass)
    }

    /// Releases glyphs and buffers that are no longer needed.
    pub fn finish_paint(&mut self) {
        self.atlas.trim();
        let paint_count = self.paint_count;
        self.buffers
            .retain(|_, cached| paint_count - cached.last_used < BUFFER_TIME_TO_LIVE);
    }

    fn ensure_buffer(&mut self, text: &TextPrimitive) {
        let paint_count = self.paint_count;
        let font_system = &mut self.font_system;
        self.buffers
            .entry(BufferKey::new(text))
            .and_modify(|cached| cached.last_used = paint_count)
            .or_insert_with(|| CachedBuffer {
                buffer: shape_text(
                    font_system,
                    &text.text,
                    TextFont {
                        font_size: text.font_size,
                        line_height: text.line_height,
                        weight: text.weight,
                    },
                    Some(text.rect.width + WRAP_SLACK),
                ),
                last_used: paint_count,
            });
    }
}

fn batch_texts<'a>(
    primitives: &'a [Primitive],
    batch: &'a Batch,
) -> impl Iterator<Item = &'a TextPrimitive> {
    batch
        .texts
        .iter()
        .filter_map(|&index| match &primitives[index] {
            Primitive::Text(text) => Some(text),
            Primitive::Quad(_) => None,
        })
}

fn text_area<'a>(buffer: &'a Buffer, text: &TextPrimitive, scale_factor: f32) -> TextArea<'a> {
    let clip = text.rect.scale(scale_factor).round_out();
    let [red, green, blue, alpha] = text.color.to_rgba8();
    TextArea {
        buffer,
        left: text.rect.x * scale_factor,
        top: text.rect.y * scale_factor,
        scale: scale_factor,
        bounds: TextBounds {
            left: clip.x as i32,
            top: clip.y as i32,
            right: clip.right() as i32,
            bottom: clip.bottom() as i32,
        },
        default_color: glyphon::Color::rgba(red, green, blue, alpha),
        custom_glyphs: &[],
    }
}
