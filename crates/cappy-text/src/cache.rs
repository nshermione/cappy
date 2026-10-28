//! Shaped text buffers kept across paints, so unchanged text is not shaped again every frame.

use std::collections::HashMap;
use std::sync::Arc;

use cappy_core::TextPrimitive;
use cosmic_text::{Buffer, FontSystem};

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

/// Buffers shaped in logical pixels, wrapped to each primitive's width. Call
/// [`ShapedTextCache::begin_paint`] and [`ShapedTextCache::finish_paint`] around every paint so
/// buffers no longer drawn eventually expire.
#[derive(Default)]
pub struct ShapedTextCache {
    buffers: HashMap<BufferKey, CachedBuffer>,
    paint_count: u64,
}

impl ShapedTextCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn begin_paint(&mut self) {
        self.paint_count += 1;
    }

    /// Shapes `text` unless an identical buffer is cached, and marks it as used by this paint.
    pub fn ensure(&mut self, font_system: &mut FontSystem, text: &TextPrimitive) {
        let paint_count = self.paint_count;
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

    /// The buffer for `text`, if [`ShapedTextCache::ensure`] shaped it.
    pub fn get(&self, text: &TextPrimitive) -> Option<&Buffer> {
        self.buffers
            .get(&BufferKey::new(text))
            .map(|cached| &cached.buffer)
    }

    /// Drops buffers that no paint used for a while.
    pub fn finish_paint(&mut self) {
        let paint_count = self.paint_count;
        self.buffers
            .retain(|_, cached| paint_count - cached.last_used < BUFFER_TIME_TO_LIVE);
    }

    pub fn len(&self) -> usize {
        self.buffers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffers.is_empty()
    }
}

impl std::fmt::Debug for ShapedTextCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShapedTextCache")
            .field("buffers", &self.buffers.len())
            .field("paint_count", &self.paint_count)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use cappy_core::{Color, Rect};

    use super::*;
    use crate::FontContext;

    fn label(text: &str) -> TextPrimitive {
        TextPrimitive {
            rect: Rect::new(0.0, 0.0, 200.0, 20.0),
            text: Arc::from(text),
            color: Color::BLACK,
            font_size: 14.0,
            line_height: 20.0,
            weight: 400,
        }
    }

    fn font_system() -> FontSystem {
        static FONTS: OnceLock<FontContext> = OnceLock::new();
        FONTS.get_or_init(FontContext::from_system).font_system()
    }

    #[test]
    fn identical_text_is_shaped_once() {
        let mut fonts = font_system();
        let mut cache = ShapedTextCache::new();
        cache.begin_paint();
        cache.ensure(&mut fonts, &label("hello"));
        cache.ensure(&mut fonts, &label("hello"));
        assert_eq!(cache.len(), 1);
        assert!(cache.get(&label("hello")).is_some());
        assert!(cache.get(&label("other")).is_none());
    }

    #[test]
    fn unused_buffers_expire() {
        let mut fonts = font_system();
        let mut cache = ShapedTextCache::new();
        cache.begin_paint();
        cache.ensure(&mut fonts, &label("stale"));
        for _ in 0..BUFFER_TIME_TO_LIVE {
            cache.begin_paint();
            cache.ensure(&mut fonts, &label("fresh"));
            cache.finish_paint();
        }
        assert!(cache.get(&label("stale")).is_none());
        assert!(cache.get(&label("fresh")).is_some());
    }
}
