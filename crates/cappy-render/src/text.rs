//! Text drawing with glyphon: one text renderer per batch, shaped buffers cached across frames.

use cappy_core::{Background, Color, Primitive, QuadPrimitive, Rect, Spin, TextPrimitive};
use cappy_text::ShapedTextCache;
use cappy_text::{caret_origin, selection_spans};
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

    /// Selection quads, then caret quads, in logical pixels. Call after [`TextLayer::prepare`].
    pub fn decorations(
        &self,
        primitives: &[Primitive],
    ) -> (Vec<QuadPrimitive>, Vec<QuadPrimitive>) {
        let mut selection = Vec::new();
        let mut carets = Vec::new();
        for primitive in primitives {
            let Primitive::Text(text) = primitive else {
                continue;
            };
            let Some(buffer) = self.buffers.get(text) else {
                continue;
            };
            if let Some((start, end)) = text.selection {
                let mut color = text.color;
                color.a *= 0.35;
                for (x, y, width, height) in selection_spans(buffer, start, end) {
                    if let Some(quad) = decoration_quad(text, x, y, width, height, color) {
                        selection.push(quad);
                    }
                }
            }
            if let Some(index) = text.caret
                && let Some((x, y, height)) = caret_origin(buffer, index)
                && let Some(quad) = decoration_quad(text, x, y, 1.0, height, text.color)
            {
                carets.push(quad);
            }
        }
        (selection, carets)
    }

    /// Releases glyphs and buffers that are no longer needed.
    pub fn finish_paint(&mut self) {
        self.atlas.trim();
        self.buffers.finish_paint();
    }
}

fn decoration_quad(
    text: &TextPrimitive,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: Color,
) -> Option<QuadPrimitive> {
    let rect = Rect::new(
        text.rect.x + x - text.scroll_x,
        text.rect.y + y - text.scroll_y,
        width,
        height,
    );
    let rect = rect.intersection(&text.rect)?;
    let rect = match text.clip {
        Some(clip) => rect.intersection(&clip)?,
        None => rect,
    };
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    Some(QuadPrimitive {
        rect,
        background: Background::Solid(color),
        border_color: Color::TRANSPARENT,
        border_width: cappy_core::Edges::ZERO,
        corner_radius: cappy_core::Corners::ZERO,
        clip: text.clip,
        spin: Spin::NONE,
    })
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
            Primitive::Quad(_) | Primitive::Shadow(_) | Primitive::Image(_) => None,
        })
}

fn text_area<'a>(buffer: &'a Buffer, text: &TextPrimitive, scale_factor: f32) -> TextArea<'a> {
    let visible = match text.clip.and_then(|clip| text.rect.intersection(&clip)) {
        Some(visible) => visible,
        None if text.clip.is_some() => Rect::ZERO,
        None => text.rect,
    };
    let clip = visible.scale(scale_factor).round_out();
    let [red, green, blue, alpha] = text.color.to_rgba8();
    TextArea {
        buffer,
        left: (text.rect.x - text.scroll_x) * scale_factor,
        top: (text.rect.y - text.scroll_y) * scale_factor,
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
