//! Owns a wgpu device and window surface for hosts without their own renderer (winit, tao).

use std::fmt;

use cappy_core::{Color, RenderFrame};

use crate::{FontContext, RenderError, RenderTarget, Renderer, TargetLoad};

#[derive(Debug)]
pub enum SurfaceError {
    CreateSurface(wgpu::CreateSurfaceError),
    RequestAdapter(wgpu::RequestAdapterError),
    RequestDevice(wgpu::RequestDeviceError),
    /// The adapter cannot present to this surface in any format.
    NoSurfaceFormat,
    /// The surface reported a validation error while acquiring a frame.
    Validation,
    Render(RenderError),
}

impl fmt::Display for SurfaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreateSurface(error) => write!(f, "failed to create the window surface: {error}"),
            Self::RequestAdapter(error) => write!(f, "no suitable GPU adapter: {error}"),
            Self::RequestDevice(error) => write!(f, "failed to open the GPU device: {error}"),
            Self::NoSurfaceFormat => f.write_str("the GPU adapter cannot present to this window"),
            Self::Validation => f.write_str("validation error while acquiring a surface frame"),
            Self::Render(error) => write!(f, "rendering failed: {error}"),
        }
    }
}

impl std::error::Error for SurfaceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CreateSurface(error) => Some(error),
            Self::RequestAdapter(error) => Some(error),
            Self::RequestDevice(error) => Some(error),
            Self::Render(error) => Some(error),
            Self::NoSurfaceFormat | Self::Validation => None,
        }
    }
}

impl From<RenderError> for SurfaceError {
    fn from(error: RenderError) -> Self {
        Self::Render(error)
    }
}

/// Result of [`WindowSurface::present`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentOutcome {
    /// The frame is on screen. `repainted` regions were redrawn; zero means a pure re-composite.
    Presented { repainted: usize },
    /// No frame could be acquired this time; request another redraw.
    Retry,
    /// The window is hidden; wait for it to become visible before redrawing.
    Occluded,
}

pub struct WindowSurface {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
}

impl WindowSurface {
    /// Opens a GPU device able to present to `window`, sized `physical_size`.
    pub async fn new(
        window: impl Into<wgpu::SurfaceTarget<'static>>,
        physical_size: (u32, u32),
        fonts: &FontContext,
    ) -> Result<Self, SurfaceError> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let surface = instance
            .create_surface(window)
            .map_err(SurfaceError::CreateSurface)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::from_env().unwrap_or_default(),
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(SurfaceError::RequestAdapter)?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("cappy device"),
                ..Default::default()
            })
            .await
            .map_err(SurfaceError::RequestDevice)?;
        let config = surface_config(&surface, &adapter, physical_size)?;
        surface.configure(&device, &config);
        log::info!(
            "cappy surface on {} ({:?}), format {:?}",
            adapter.get_info().name,
            adapter.get_info().backend,
            config.format
        );
        let renderer = Renderer::new(&device, &queue, fonts);
        Ok(Self {
            device,
            queue,
            surface,
            config,
            renderer,
        })
    }

    pub fn resize(&mut self, physical_size: (u32, u32)) {
        let (width, height) = physical_size;
        if width == 0 || height == 0 || (width, height) == (self.config.width, self.config.height) {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn renderer_mut(&mut self) -> &mut Renderer {
        &mut self.renderer
    }

    /// Paints `frame`'s damage and presents. When no surface frame is available the damage is
    /// still painted offscreen, so nothing is lost before the next attempt.
    pub fn present(
        &mut self,
        frame: &RenderFrame,
        background: Color,
    ) -> Result<PresentOutcome, SurfaceError> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => {
                self.draw_and_present(texture, frame, background)
            }
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                let outcome = self.draw_and_present(texture, frame, background);
                self.surface.configure(&self.device, &self.config);
                outcome
            }
            wgpu::CurrentSurfaceTexture::Timeout => self.paint_only(frame, PresentOutcome::Retry),
            wgpu::CurrentSurfaceTexture::Occluded => {
                self.paint_only(frame, PresentOutcome::Occluded)
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                self.paint_only(frame, PresentOutcome::Retry)
            }
            wgpu::CurrentSurfaceTexture::Validation => Err(SurfaceError::Validation),
        }
    }

    fn draw_and_present(
        &mut self,
        texture: wgpu::SurfaceTexture,
        frame: &RenderFrame,
        background: Color,
    ) -> Result<PresentOutcome, SurfaceError> {
        let view = texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.create_encoder();
        let target = RenderTarget {
            view: &view,
            format: self.config.format,
        };
        let repainted = self.renderer.render(
            &self.device,
            &self.queue,
            &mut encoder,
            frame,
            target,
            TargetLoad::Clear(background),
        )?;
        self.queue.submit([encoder.finish()]);
        self.queue.present(texture);
        Ok(PresentOutcome::Presented { repainted })
    }

    fn paint_only(
        &mut self,
        frame: &RenderFrame,
        outcome: PresentOutcome,
    ) -> Result<PresentOutcome, SurfaceError> {
        let mut encoder = self.create_encoder();
        self.renderer
            .paint(&self.device, &self.queue, &mut encoder, frame)?;
        self.queue.submit([encoder.finish()]);
        Ok(outcome)
    }

    fn create_encoder(&self) -> wgpu::CommandEncoder {
        self.device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cappy frame"),
            })
    }
}

fn surface_config(
    surface: &wgpu::Surface<'_>,
    adapter: &wgpu::Adapter,
    (width, height): (u32, u32),
) -> Result<wgpu::SurfaceConfiguration, SurfaceError> {
    let mut config = surface
        .get_default_config(adapter, width.max(1), height.max(1))
        .ok_or(SurfaceError::NoSurfaceFormat)?;
    let capabilities = surface.get_capabilities(adapter);
    if let Some(srgb) = capabilities
        .formats
        .iter()
        .copied()
        .find(|format| format.is_srgb())
    {
        config.format = srgb;
    }
    config.present_mode = wgpu::PresentMode::AutoVsync;
    Ok(config)
}
