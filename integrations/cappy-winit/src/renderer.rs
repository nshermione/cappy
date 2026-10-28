//! Opens the renderer backend chosen for a window, among those enabled by cargo features.

use std::sync::Arc;

use cappy_host::{RendererError, RendererKind, RendererSelectionError, WindowRenderer};
use cappy_text::FontContext;
use winit::window::Window;

#[cfg(not(any(feature = "gpu", feature = "cpu")))]
compile_error!("cappy-winit needs a renderer: enable the `gpu` or `cpu` feature");

/// Backends compiled into this build, in order of preference.
pub const AVAILABLE_RENDERERS: &[RendererKind] = &[
    #[cfg(feature = "gpu")]
    RendererKind::Gpu,
    #[cfg(feature = "cpu")]
    RendererKind::Cpu,
];

pub(crate) fn open_renderer(
    kind: RendererKind,
    window: &Arc<Window>,
    fonts: &FontContext,
) -> Result<Box<dyn WindowRenderer>, RendererError> {
    match kind {
        #[cfg(feature = "gpu")]
        RendererKind::Gpu => {
            let size = window.inner_size();
            pollster::block_on(cappy_render::open_window_renderer(
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
