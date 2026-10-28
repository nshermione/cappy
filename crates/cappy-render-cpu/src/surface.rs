//! Shows the CPU canvas in a window through softbuffer.

use std::fmt;
use std::num::NonZeroU32;

use cappy_core::{Color, RenderFrame};
use cappy_host::{PixelRect, PresentOutcome, RendererError, RendererKind, WindowRenderer};
use cappy_text::FontContext;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use softbuffer::SoftBufferError;

use crate::CpuRenderer;
use crate::blend::{over, premultiply, to_xrgb};
use crate::overlay::draw_damage_overlay;

const BYTES_PER_PIXEL: usize = 4;

/// A softbuffer failure, tagged with the step that failed. softbuffer's error can hold a raw
/// window handle, which is neither `Send` nor `Sync`, so only its message is kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CpuSurfaceError {
    CreateContext(String),
    CreateSurface(String),
    Resize {
        width: u32,
        height: u32,
        message: String,
    },
    AcquireBuffer(String),
    Present(String),
}

impl fmt::Display for CpuSurfaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreateContext(message) => {
                write!(f, "failed to connect softbuffer to the display: {message}")
            }
            Self::CreateSurface(message) => {
                write!(f, "failed to create the softbuffer surface: {message}")
            }
            Self::Resize {
                width,
                height,
                message,
            } => write!(
                f,
                "failed to resize the window buffer to {width}x{height}: {message}"
            ),
            Self::AcquireBuffer(message) => {
                write!(f, "failed to acquire the window buffer: {message}")
            }
            Self::Present(message) => write!(f, "failed to present the window buffer: {message}"),
        }
    }
}

impl std::error::Error for CpuSurfaceError {}

/// Opens a softbuffer surface for `window` as a [`WindowRenderer`].
pub fn open_window_renderer<W>(
    window: W,
    fonts: &FontContext,
) -> Result<Box<dyn WindowRenderer>, RendererError>
where
    W: HasDisplayHandle + HasWindowHandle + Clone + 'static,
{
    let surface = CpuWindowSurface::new(window, fonts)
        .map_err(|error| RendererError::new(RendererKind::Cpu, error))?;
    Ok(Box::new(surface))
}

pub struct CpuWindowSurface<W: HasDisplayHandle + HasWindowHandle> {
    // Declared before `_context` so the surface is dropped first.
    surface: softbuffer::Surface<W, W>,
    _context: softbuffer::Context<W>,
    buffer_size: Option<(u32, u32)>,
    renderer: CpuRenderer,
    debug_damage: bool,
    /// Regions the overlay covered in the last presented buffer; they must be presented again
    /// to erase the outlines.
    shown_overlay: Vec<PixelRect>,
}

impl<W: HasDisplayHandle + HasWindowHandle + Clone> CpuWindowSurface<W> {
    pub fn new(window: W, fonts: &FontContext) -> Result<Self, CpuSurfaceError> {
        let context = softbuffer::Context::new(window.clone())
            .map_err(|error| CpuSurfaceError::CreateContext(error.to_string()))?;
        let surface = softbuffer::Surface::new(&context, window)
            .map_err(|error| CpuSurfaceError::CreateSurface(error.to_string()))?;
        log::info!("cappy CPU surface (softbuffer + tiny-skia)");
        Ok(Self {
            surface,
            _context: context,
            buffer_size: None,
            renderer: CpuRenderer::new(fonts),
            debug_damage: false,
            shown_overlay: Vec::new(),
        })
    }

    pub fn renderer(&self) -> &CpuRenderer {
        &self.renderer
    }

    /// Paints `frame`'s damage and presents the changed part of the window.
    pub fn present(
        &mut self,
        frame: &RenderFrame,
        background: Color,
    ) -> Result<PresentOutcome, CpuSurfaceError> {
        let (width, height) = (frame.physical_width, frame.physical_height);
        let (Some(buffer_width), Some(buffer_height)) =
            (NonZeroU32::new(width), NonZeroU32::new(height))
        else {
            return Ok(PresentOutcome::Occluded);
        };
        let resized = self.buffer_size != Some((width, height));
        if resized {
            self.surface
                .resize(buffer_width, buffer_height)
                .map_err(|error| resize_error(width, height, &error))?;
            self.buffer_size = Some((width, height));
        }

        let repainted = self.renderer.paint(frame);
        let canvas = self.renderer.canvas().expect("paint creates the canvas");
        let mut buffer = self
            .surface
            .buffer_mut()
            .map_err(|error| CpuSurfaceError::AcquireBuffer(error.to_string()))?;
        write_canvas(&mut buffer, canvas.data(), background);
        let overlay = if self.debug_damage {
            self.renderer.last_repainted().to_vec()
        } else {
            Vec::new()
        };
        draw_damage_overlay(&mut buffer, width, &overlay);

        let full_present = resized || repainted == 0 || buffer.age() == 0;
        let result = if full_present {
            buffer.present()
        } else {
            let damage: Vec<softbuffer::Rect> = self
                .renderer
                .last_repainted()
                .iter()
                .chain(&self.shown_overlay)
                .filter_map(|&rect| to_softbuffer_rect(rect))
                .collect();
            buffer.present_with_damage(&damage)
        };
        result.map_err(|error| CpuSurfaceError::Present(error.to_string()))?;
        self.shown_overlay = overlay;
        Ok(PresentOutcome::Presented { repainted })
    }
}

impl<W: HasDisplayHandle + HasWindowHandle + Clone> WindowRenderer for CpuWindowSurface<W> {
    fn kind(&self) -> RendererKind {
        RendererKind::Cpu
    }

    // The buffer follows each frame's size in `present`, where a failed resize can be reported.
    fn resize(&mut self, _physical_size: (u32, u32)) {}

    fn present(
        &mut self,
        frame: &RenderFrame,
        background: Color,
    ) -> Result<PresentOutcome, RendererError> {
        CpuWindowSurface::present(self, frame, background)
            .map_err(|error| RendererError::new(RendererKind::Cpu, error))
    }

    fn set_debug_damage(&mut self, enabled: bool) {
        self.debug_damage = enabled;
    }

    fn debug_damage(&self) -> bool {
        self.debug_damage
    }
}

/// Composites the premultiplied canvas over `background` into `0RGB` pixels.
fn write_canvas(buffer: &mut [u32], canvas: &[u8], background: Color) {
    let background = premultiply(background.to_rgba8());
    for (target, pixel) in buffer.iter_mut().zip(canvas.chunks_exact(BYTES_PER_PIXEL)) {
        *target = to_xrgb(over([pixel[0], pixel[1], pixel[2], pixel[3]], background));
    }
}

fn to_softbuffer_rect(rect: PixelRect) -> Option<softbuffer::Rect> {
    Some(softbuffer::Rect {
        x: rect.x,
        y: rect.y,
        width: NonZeroU32::new(rect.width)?,
        height: NonZeroU32::new(rect.height)?,
    })
}

fn resize_error(width: u32, height: u32, error: &SoftBufferError) -> CpuSurfaceError {
    CpuSurfaceError::Resize {
        width,
        height,
        message: error.to_string(),
    }
}
