//! Headless CPU rendering for pixel tests: paint a [`RenderFrame`] with the CPU renderer and
//! read the composited pixels. Needs no GPU, so it runs on any machine.

use cappy_core::{Color, RenderFrame};
use cappy_render_cpu::CpuRenderer;
use cappy_text::FontContext;

pub use crate::image::{Image, ImageError, assert_snapshot};

/// A CPU renderer keeping its canvas across frames, like a window would.
pub struct CpuHeadlessRenderer {
    renderer: CpuRenderer,
}

impl CpuHeadlessRenderer {
    pub fn new(fonts: &FontContext) -> Self {
        Self {
            renderer: CpuRenderer::new(fonts),
        }
    }

    pub fn renderer(&self) -> &CpuRenderer {
        &self.renderer
    }

    /// Paints `frame` (only its damage) and returns the canvas over `background`.
    pub fn render(&mut self, frame: &RenderFrame, background: Color) -> Image {
        self.renderer.paint(frame);
        Image {
            width: frame.physical_width.max(1),
            height: frame.physical_height.max(1),
            pixels: self.renderer.composite_rgba(background),
        }
    }
}
