//! Text drawing with glyphon: one text renderer per batch, shaped buffers cached across frames.

use cappy_core::{Primitive, TextPrimitive};
use cappy_text::ShapedTextCache;
use glyphon::{
    Buffer, Cache, FontSystem, PrepareError, RenderError, Resolution, SwashCache, TextArea,
    TextAtlas, TextBounds, TextRenderer, Viewport,
};

use crate::FontContext;
use crate::batch::Batch;

pub(crate) struct TextLayer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    renderers: Vec<TextRenderer>,
    buffers: ShapedTextCache,
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
            buffers: ShapedTextCache::new(),
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
        self.buffers.begin_paint();
        self.viewport.update(
            queue,
            Resolution {
                width: resolution.0,
                height: resolution.1,
            },
        );
        for batch in batches {
            for text in batch_texts(primitives, batch) {
                self.buffers.ensure(&mut self.font_system, text);
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
            let buffers = &self.buffers;
            let areas = batch_texts(primitives, batch).map(|text| {
                let buffer = buffers
                    .get(text)
                    .expect("every batch text was shaped above");
                text_area(buffer, text, scale_factor)
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
        self.buffers.finish_paint();
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
