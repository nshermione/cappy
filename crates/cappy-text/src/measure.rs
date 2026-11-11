//! [`TextMeasurer`] backed by cosmic-text, matching what the renderer draws.

use std::collections::HashMap;

use cappy_core::{CaretGeometry, Size, TextAlign, TextMeasureRequest, TextMeasurer};

use crate::geometry::hit_index;
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

    fn shaped(
        &mut self,
        request: &TextMeasureRequest<'_>,
        max_width: Option<f32>,
        align: TextAlign,
        align_width: f32,
    ) -> cosmic_text::Buffer {
        let font = TextFont {
            font_size: request.font_size,
            line_height: request.line_height,
            weight: request.weight,
        };
        shape_text(
            &mut self.font_system,
            request.text,
            font,
            max_width,
            align,
            align_width,
        )
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
        let buffer = shape_text(
            &mut self.font_system,
            request.text,
            font,
            max_width,
            TextAlign::Start,
            0.0,
        );
        let (width, height) = shaped_extent(&buffer);
        // Rounding up keeps the text from wrapping again when laid out in exactly this width.
        Size::new(width.ceil(), height)
    }
}

impl TextMeasurer for CosmicTextMeasurer {
    fn hit_test(
        &mut self,
        request: &TextMeasureRequest<'_>,
        max_width: Option<f32>,
        align: TextAlign,
        align_width: f32,
        x: f32,
        y: f32,
    ) -> usize {
        let buffer = self.shaped(request, max_width, align, align_width);
        hit_index(&buffer, x, y)
    }

    fn caret_geometry(
        &mut self,
        request: &TextMeasureRequest<'_>,
        max_width: Option<f32>,
        align: TextAlign,
        align_width: f32,
        index: usize,
    ) -> CaretGeometry {
        let buffer = self.shaped(request, max_width, align, align_width);
        let (x, y, line_height) = crate::geometry::caret_origin(&buffer, index).unwrap_or((
            0.0,
            0.0,
            request.line_height,
        ));
        let line_width = buffer
            .layout_runs()
            .find(|run| (run.line_top - y).abs() < 0.5)
            .map(|run| run.line_w)
            .unwrap_or(0.0);
        CaretGeometry {
            x,
            y,
            line_width,
            line_height,
        }
    }

    fn measure(&mut self, request: &TextMeasureRequest<'_>, max_width: Option<f32>) -> Size {
        let key = MeasureKey::new(request, max_width);
        if let Some(size) = self.cache.get(&key) {
            return *size;
        }
        let size = self.measure_uncached(request, max_width);
        if self.cache.len() >= MAX_CACHED_MEASUREMENTS {
            self.cache.clear();
            self.cache.shrink_to_fit();
        }
        self.cache.insert(key, size);
        size
    }

    fn cached_measurements(&self) -> usize {
        self.cache.len()
    }
}

impl std::fmt::Debug for CosmicTextMeasurer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CosmicTextMeasurer")
            .field("cached", &self.cache.len())
            .finish_non_exhaustive()
    }
}
