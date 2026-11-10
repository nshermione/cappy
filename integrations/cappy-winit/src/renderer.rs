//! Opens the renderer backend chosen for a window, among those enabled by cargo features.

use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use cappy_host::RendererSelectionError;
use cappy_host::{RendererError, RendererKind, WindowRenderer};
use cappy_text::FontContext;
use winit::window::Window;

#[cfg(not(any(feature = "gpu", feature = "cpu")))]
compile_error!("cappy-winit needs a renderer: enable the `gpu` or `cpu` feature");

#[cfg(all(target_arch = "wasm32", not(feature = "gpu")))]
compile_error!("wasm needs the gpu feature; the cpu renderer has no browser surface");

/// Backends compiled into this build, in order of preference.
pub const AVAILABLE_RENDERERS: &[RendererKind] = &[
    #[cfg(feature = "gpu")]
    RendererKind::Gpu,
    #[cfg(feature = "cpu")]
    RendererKind::Cpu,
];

/// Opens the GPU surface. The browser cannot block the thread, so wasm calls this directly
/// and finishes the window when the future resolves.
#[cfg(feature = "gpu")]
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
    match kind {
        #[cfg(feature = "gpu")]
        RendererKind::Gpu => {
            let size = window.inner_size();
            pollster::block_on(open_gpu(
                Arc::clone(window),
                (size.width, size.height),
                fonts,
            ))
        }
        #[cfg(feature = "cpu")]
        RendererKind::Cpu => cappy_render_cpu::open_window_renderer(Arc::clone(window), fonts),
        #[allow(unreachable_patterns)]
        unavailable => Err(RendererError::new(
            unavailable,
            RendererSelectionError::Unavailable(unavailable),
        )),
    }
}
