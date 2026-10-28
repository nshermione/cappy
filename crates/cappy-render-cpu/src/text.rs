//! Text drawing on the CPU: buffers shaped by `cappy-text`, glyphs rasterized by swash and
//! blended into the pixmap, positioned exactly like glyphon positions them on the GPU.

use cappy_core::{Rect, TextPrimitive};
use cappy_text::cosmic_text::{FontSystem, SwashCache, SwashContent, SwashImage};
use cappy_text::{FontContext, ShapedTextCache};
use tiny_skia::PixmapMut;

use crate::blend::{blend_into, premultiply, scale};

const BYTES_PER_PIXEL: usize = 4;

/// Pixel bounds glyphs may touch, in target coordinates; right and bottom are exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ClipBounds {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl ClipBounds {
    fn is_empty(self) -> bool {
        self.left >= self.right || self.top >= self.bottom
    }
}

pub(crate) struct TextPainter {
    font_system: FontSystem,
    swash_cache: SwashCache,
    buffers: ShapedTextCache,
}

impl TextPainter {
    pub fn new(fonts: &FontContext) -> Self {
        Self {
            font_system: fonts.font_system(),
            swash_cache: SwashCache::new(),
            buffers: ShapedTextCache::new(),
        }
    }

    pub fn begin_paint(&mut self) {
        self.buffers.begin_paint();
    }

    pub fn finish_paint(&mut self) {
        self.buffers.finish_paint();
    }

    /// Paints `text`, scaled to physical pixels, into `target`, whose top-left pixel sits at
    /// `origin`. Glyphs are clipped to the text's rectangle.
    pub fn paint(
        &mut self,
        target: &mut PixmapMut<'_>,
        text: &TextPrimitive,
        scale_factor: f32,
        origin: (u32, u32),
    ) {
        let clip = clip_bounds(text.rect, scale_factor, origin, target);
        if clip.is_empty() {
            return;
        }
        self.buffers.ensure(&mut self.font_system, text);
        let buffer = self
            .buffers
            .get(text)
            .expect("the buffer was shaped just above");
        let color = premultiply(text.color.to_rgba8());
        let left = text.rect.x * scale_factor;
        let top = text.rect.y * scale_factor;
        let target_width = target.width() as usize;
        let pixels = target.data_mut();
        for run in buffer.layout_runs() {
            let line_y = (run.line_y * scale_factor).round() as i32;
            for glyph in run.glyphs {
                let physical = glyph.physical((left, top), scale_factor);
                let glyph_color = glyph.color_opt.map_or(color, |custom| {
                    premultiply([custom.r(), custom.g(), custom.b(), custom.a()])
                });
                let Some(image) = self
                    .swash_cache
                    .get_image(&mut self.font_system, physical.cache_key)
                else {
                    continue;
                };
                let position = (
                    physical.x + image.placement.left - origin.0 as i32,
                    line_y + physical.y - image.placement.top - origin.1 as i32,
                );
                let target = GlyphTarget {
                    pixels: &mut *pixels,
                    width: target_width,
                    clip,
                };
                draw_glyph(target, image, position, glyph_color);
            }
        }
    }
}

struct GlyphTarget<'a> {
    pixels: &'a mut [u8],
    width: usize,
    clip: ClipBounds,
}

/// The text rectangle in physical pixels, moved into target coordinates and clipped to it.
fn clip_bounds(
    rect: Rect,
    scale_factor: f32,
    origin: (u32, u32),
    target: &PixmapMut<'_>,
) -> ClipBounds {
    let physical = rect.scale(scale_factor).round_out();
    ClipBounds {
        left: (physical.x as i32 - origin.0 as i32).max(0),
        top: (physical.y as i32 - origin.1 as i32).max(0),
        right: (physical.right() as i32 - origin.0 as i32).min(target.width() as i32),
        bottom: (physical.bottom() as i32 - origin.1 as i32).min(target.height() as i32),
    }
}

/// Blends one rasterized glyph with its top-left corner at `position`.
fn draw_glyph(target: GlyphTarget<'_>, image: &SwashImage, position: (i32, i32), color: [u8; 4]) {
    let (x, y) = position;
    let glyph_width = image.placement.width as i32;
    let glyph_height = image.placement.height as i32;
    let visible = ClipBounds {
        left: x.max(target.clip.left),
        top: y.max(target.clip.top),
        right: (x + glyph_width).min(target.clip.right),
        bottom: (y + glyph_height).min(target.clip.bottom),
    };
    if visible.is_empty() {
        return;
    }
    for pixel_y in visible.top..visible.bottom {
        for pixel_x in visible.left..visible.right {
            let texel = ((pixel_y - y) * glyph_width + (pixel_x - x)) as usize;
            let source = glyph_texel(image, texel, color);
            let offset = (pixel_y as usize * target.width + pixel_x as usize) * BYTES_PER_PIXEL;
            blend_into(&mut target.pixels[offset..offset + BYTES_PER_PIXEL], source);
        }
    }
}

/// The premultiplied color a glyph texel contributes, tinted by the text `color`.
fn glyph_texel(image: &SwashImage, texel: usize, color: [u8; 4]) -> [u8; 4] {
    match image.content {
        SwashContent::Mask => scale(color, image.data[texel]),
        SwashContent::Color => {
            let start = texel * BYTES_PER_PIXEL;
            let rgba = [
                image.data[start],
                image.data[start + 1],
                image.data[start + 2],
                image.data[start + 3],
            ];
            scale(premultiply(rgba), color[3])
        }
        SwashContent::SubpixelMask => {
            let start = texel * BYTES_PER_PIXEL;
            let coverage = image.data[start..start + 3]
                .iter()
                .copied()
                .max()
                .unwrap_or(0);
            scale(color, coverage)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use cappy_core::Color;
    use tiny_skia::Pixmap;

    use super::*;

    fn label(rect: Rect) -> TextPrimitive {
        TextPrimitive {
            rect,
            text: Arc::from("Hello"),
            color: Color::BLACK,
            font_size: 16.0,
            line_height: 20.0,
            weight: 400,
        }
    }

    fn inked_pixels(pixmap: &Pixmap) -> usize {
        pixmap
            .pixels()
            .iter()
            .filter(|pixel| pixel.alpha() > 0)
            .count()
    }

    #[test]
    fn text_leaves_ink_inside_its_rect_only() {
        let fonts = FontContext::from_system();
        let mut painter = TextPainter::new(&fonts);
        let mut pixmap = Pixmap::new(120, 40).expect("non-empty pixmap");
        let rect = Rect::new(10.0, 10.0, 80.0, 20.0);
        painter.begin_paint();
        painter.paint(&mut pixmap.as_mut(), &label(rect), 1.0, (0, 0));
        painter.finish_paint();
        assert!(inked_pixels(&pixmap) > 20, "expected glyph pixels");
        for (index, pixel) in pixmap.pixels().iter().enumerate() {
            let (x, y) = (index as u32 % 120, index as u32 / 120);
            if pixel.alpha() > 0 {
                assert!(
                    (10..90).contains(&x) && (10..30).contains(&y),
                    "ink outside the text rect at ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn text_outside_the_target_draws_nothing() {
        let fonts = FontContext::from_system();
        let mut painter = TextPainter::new(&fonts);
        let mut pixmap = Pixmap::new(40, 40).expect("non-empty pixmap");
        painter.begin_paint();
        let far_away = label(Rect::new(10.0, 10.0, 80.0, 20.0));
        painter.paint(&mut pixmap.as_mut(), &far_away, 1.0, (500, 500));
        assert_eq!(inked_pixels(&pixmap), 0);
    }
}
