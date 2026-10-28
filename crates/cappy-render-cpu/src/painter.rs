//! Paints [`RenderFrame`]s into a persistent pixmap, repainting only damaged regions.

use cappy_core::{Color, Damage, Primitive, RenderFrame};
use cappy_host::{PixelRect, physical_regions};
use cappy_text::FontContext;
use tiny_skia::{Pixmap, PixmapMut};

use crate::blend::{over, premultiply};
use crate::quad::paint_quad;
use crate::text::TextPainter;

/// Anti-aliasing can bleed slightly past a primitive's bounds.
const CULL_MARGIN: f32 = 1.0;
const BYTES_PER_PIXEL: usize = 4;

/// Keeps last frame's pixels (premultiplied, sRGB-encoded RGBA) so a frame only costs its damage.
pub struct CpuRenderer {
    canvas: Option<Pixmap>,
    /// Reused backing store for the region being painted.
    scratch: Vec<u8>,
    text: TextPainter,
    /// Physical rectangles repainted by the latest paint that had damage.
    last_repainted: Vec<PixelRect>,
}

impl CpuRenderer {
    pub fn new(fonts: &FontContext) -> Self {
        Self {
            canvas: None,
            scratch: Vec::new(),
            text: TextPainter::new(fonts),
            last_repainted: Vec::new(),
        }
    }

    /// Repaints the damaged regions of the canvas. Returns how many regions were repainted;
    /// zero means the frame had no damage and nothing was drawn.
    pub fn paint(&mut self, frame: &RenderFrame) -> usize {
        let size = (frame.physical_width.max(1), frame.physical_height.max(1));
        let damage = if self.ensure_canvas(size) {
            &Damage::Full
        } else {
            &frame.damage
        };
        let regions = physical_regions(
            &damage.rects(frame.logical_size()),
            frame.scale_factor,
            size,
        );
        if regions.is_empty() {
            return 0;
        }
        self.text.begin_paint();
        for &region in &regions {
            self.paint_region(frame, region);
        }
        self.text.finish_paint();
        let repainted = regions.len();
        self.last_repainted = regions;
        repainted
    }

    /// The painted pixels, premultiplied and sRGB-encoded; `None` before the first paint.
    pub fn canvas(&self) -> Option<&Pixmap> {
        self.canvas.as_ref()
    }

    pub fn last_repainted(&self) -> &[PixelRect] {
        &self.last_repainted
    }

    /// The canvas over an opaque `background`, as straight RGBA8 rows.
    pub fn composite_rgba(&self, background: Color) -> Vec<u8> {
        let background = premultiply(background.to_rgba8());
        self.canvas
            .as_ref()
            .map(|canvas| {
                canvas
                    .data()
                    .chunks_exact(BYTES_PER_PIXEL)
                    .flat_map(|pixel| over([pixel[0], pixel[1], pixel[2], pixel[3]], background))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns `true` when the canvas is new and must be repainted completely.
    fn ensure_canvas(&mut self, (width, height): (u32, u32)) -> bool {
        if self
            .canvas
            .as_ref()
            .is_some_and(|canvas| (canvas.width(), canvas.height()) == (width, height))
        {
            return false;
        }
        self.canvas = Some(Pixmap::new(width, height).expect("canvas size is at least 1x1"));
        true
    }

    /// Paints every primitive touching `region` into a cleared scratch pixmap of the region's
    /// size, then copies it over the canvas. The scratch bounds clip the drawing.
    fn paint_region(&mut self, frame: &RenderFrame, region: PixelRect) {
        let Self {
            canvas,
            scratch,
            text,
            ..
        } = self;
        let canvas = canvas.as_mut().expect("paint creates the canvas");
        scratch.clear();
        scratch.resize(
            region.width as usize * region.height as usize * BYTES_PER_PIXEL,
            0,
        );
        let mut target = PixmapMut::from_bytes(scratch, region.width, region.height)
            .expect("physical regions are never empty");
        let bounds = region.to_rect();
        let origin = (region.x, region.y);
        for primitive in frame.primitives.iter() {
            let physical_bounds = primitive
                .bounds()
                .scale(frame.scale_factor)
                .inflate(CULL_MARGIN);
            if !physical_bounds.intersects(&bounds) {
                continue;
            }
            match primitive {
                Primitive::Quad(quad) => paint_quad(
                    &mut target,
                    quad,
                    frame.scale_factor,
                    (origin.0 as f32, origin.1 as f32),
                ),
                Primitive::Text(label) => {
                    text.paint(&mut target, label, frame.scale_factor, origin)
                }
            }
        }
        copy_region(canvas, scratch, region);
    }
}

/// Replaces `region` of `canvas` with the tightly packed rows in `pixels`.
fn copy_region(canvas: &mut Pixmap, pixels: &[u8], region: PixelRect) {
    let canvas_row = canvas.width() as usize * BYTES_PER_PIXEL;
    let region_row = region.width as usize * BYTES_PER_PIXEL;
    let left = region.x as usize * BYTES_PER_PIXEL;
    let data = canvas.data_mut();
    for (row, source) in pixels.chunks_exact(region_row).enumerate() {
        let start = (region.y as usize + row) * canvas_row + left;
        data[start..start + region_row].copy_from_slice(source);
    }
}

impl std::fmt::Debug for CpuRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CpuRenderer")
            .field(
                "canvas",
                &self
                    .canvas
                    .as_ref()
                    .map(|canvas| (canvas.width(), canvas.height())),
            )
            .field("last_repainted", &self.last_repainted)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use cappy_core::{QuadPrimitive, Rect};

    use super::*;

    const WIDTH: u32 = 64;
    const HEIGHT: u32 = 32;

    fn square(x: f32, color: Color) -> Primitive {
        Primitive::Quad(QuadPrimitive {
            rect: Rect::new(x, 0.0, 32.0, 32.0),
            background: color,
            border_color: Color::TRANSPARENT,
            border_width: 0.0,
            corner_radius: 0.0,
        })
    }

    fn frame(primitives: Vec<Primitive>, damage: Damage) -> RenderFrame {
        RenderFrame {
            physical_width: WIDTH,
            physical_height: HEIGHT,
            scale_factor: 1.0,
            primitives: Arc::new(primitives),
            damage,
        }
    }

    fn pixel(renderer: &CpuRenderer, x: u32, y: u32) -> [u8; 4] {
        let color = renderer
            .canvas()
            .and_then(|canvas| canvas.pixel(x, y))
            .expect("pixel inside the canvas");
        [color.red(), color.green(), color.blue(), color.alpha()]
    }

    fn renderer() -> CpuRenderer {
        CpuRenderer::new(&FontContext::from_database(
            "en-US".to_owned(),
            cappy_text::fontdb::Database::new(),
        ))
    }

    #[test]
    fn first_paint_repaints_everything() {
        let mut renderer = renderer();
        let repainted = renderer.paint(&frame(vec![square(0.0, Color::WHITE)], Damage::None));
        assert_eq!(repainted, 1);
        assert_eq!(pixel(&renderer, 4, 4), [255, 255, 255, 255]);
        assert_eq!(pixel(&renderer, 40, 4), [0, 0, 0, 0]);
    }

    #[test]
    fn partial_damage_leaves_other_pixels_alone() {
        let mut renderer = renderer();
        renderer.paint(&frame(
            vec![square(0.0, Color::WHITE), square(32.0, Color::WHITE)],
            Damage::Full,
        ));
        let right_half = Damage::Regions(vec![Rect::new(32.0, 0.0, 32.0, 32.0)]);
        let black = Color::rgb(0, 0, 0);
        renderer.paint(&frame(
            vec![square(0.0, black), square(32.0, black)],
            right_half,
        ));
        assert_eq!(pixel(&renderer, 4, 4), [255, 255, 255, 255]);
        assert_eq!(pixel(&renderer, 40, 4), [0, 0, 0, 255]);
        assert_eq!(
            renderer.last_repainted(),
            [PixelRect {
                x: 32,
                y: 0,
                width: 32,
                height: 32
            }]
        );
    }

    #[test]
    fn damaged_regions_are_cleared_before_repainting() {
        let mut renderer = renderer();
        renderer.paint(&frame(vec![square(0.0, Color::WHITE)], Damage::Full));
        renderer.paint(&frame(Vec::new(), Damage::Full));
        assert_eq!(pixel(&renderer, 4, 4), [0, 0, 0, 0]);
    }

    #[test]
    fn frames_without_damage_paint_nothing() {
        let mut renderer = renderer();
        renderer.paint(&frame(vec![square(0.0, Color::WHITE)], Damage::Full));
        assert_eq!(renderer.paint(&frame(Vec::new(), Damage::None)), 0);
        assert_eq!(pixel(&renderer, 4, 4), [255, 255, 255, 255]);
    }

    #[test]
    fn composite_puts_the_canvas_over_the_background() {
        let mut renderer = renderer();
        renderer.paint(&frame(vec![square(0.0, Color::WHITE)], Damage::Full));
        let image = renderer.composite_rgba(Color::rgb(10, 20, 30));
        let right = ((4 * WIDTH + 40) * 4) as usize;
        assert_eq!(&image[..4], [255, 255, 255, 255]);
        assert_eq!(&image[right..right + 4], [10, 20, 30, 255]);
    }
}
