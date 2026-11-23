//! Opens the wgpu renderer for a window.

use std::sync::Arc;

use cappy_host::{RendererError, RendererKind, WindowRenderer};
use cappy_text::FontContext;
use winit::window::Window;

/// Backends compiled into this build, in order of preference.
pub const AVAILABLE_RENDERERS: &[RendererKind] = &[RendererKind::Gpu];

/// Opens the GPU surface. The browser cannot block the thread, so wasm calls this directly
/// and finishes the window when the future resolves.
pub(crate) async fn open_gpu(
    window: Arc<Window>,
    physical_size: (u32, u32),
    fonts: &FontContext,
) -> Result<Box<dyn WindowRenderer>, RendererError> {
    cappy_render::open_window_renderer(window, physical_size, fonts).await
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn open_renderer(
    kind: RendererKind,
    window: &Arc<Window>,
    fonts: &FontContext,
) -> Result<Box<dyn WindowRenderer>, RendererError> {
    let RendererKind::Gpu = kind;
    let size = window.inner_size();
    pollster::block_on(open_gpu(
        Arc::clone(window),
        (size.width, size.height),
        fonts,
    ))
}
