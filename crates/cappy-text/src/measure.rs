//! [`TextMeasurer`] backed by cosmic-text, matching what the renderer draws.

use std::collections::HashMap;

use cappy_core::{Size, TextMeasureRequest, TextMeasurer};
use cosmic_text::FontSystem;

use crate::FontContext;
use crate::shaping::{TextFont, shape_text, shaped_extent};

/// Measurements are cached; past this many entries the cache starts over.
const MAX_CACHED_MEASUREMENTS: usize = 4096;

#[derive(Clone, PartialEq, Eq, Hash)]
struct MeasureKey {
    text: String,
    font_size_bits: u32,
    line_height_bits: u32,
    weight: u16,
    max_width_bits: Option<u32>,
}

impl MeasureKey {
    fn new(request: &TextMeasureRequest<'_>, max_width: Option<f32>) -> Self {
        Self {
            text: request.text.to_owned(),
            font_size_bits: request.font_size.to_bits(),
            line_height_bits: request.line_height.to_bits(),
            weight: request.weight,
            max_width_bits: max_width.map(f32::to_bits),
        }
    }
}

pub struct CosmicTextMeasurer {
    font_system: FontSystem,
    cache: HashMap<MeasureKey, Size>,
}

impl CosmicTextMeasurer {
    pub fn new(fonts: &FontContext) -> Self {
        Self {
            font_system: fonts.font_system(),
            cache: HashMap::new(),
        }
    }

    fn measure_uncached(
        &mut self,
        request: &TextMeasureRequest<'_>,
        max_width: Option<f32>,
    ) -> Size {
        let font = TextFont {
            font_size: request.font_size,
            line_height: request.line_height,
            weight: request.weight,
        };
        let buffer = shape_text(&mut self.font_system, request.text, font, max_width);
        let (width, height) = shaped_extent(&buffer);
        // Rounding up keeps the text from wrapping again when laid out in exactly this width.
        Size::new(width.ceil(), height)
    }
}

impl TextMeasurer for CosmicTextMeasurer {
    fn measure(&mut self, request: &TextMeasureRequest<'_>, max_width: Option<f32>) -> Size {
        let key = MeasureKey::new(request, max_width);
        if let Some(size) = self.cache.get(&key) {
            return *size;
        }
        let size = self.measure_uncached(request, max_width);
        if self.cache.len() >= MAX_CACHED_MEASUREMENTS {
            self.cache.clear();
        }
        self.cache.insert(key, size);
        size
    }
}

impl std::fmt::Debug for CosmicTextMeasurer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CosmicTextMeasurer")
            .field("cached", &self.cache.len())
            .finish_non_exhaustive()
    }
}
