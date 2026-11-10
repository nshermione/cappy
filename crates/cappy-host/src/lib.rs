//! Renderer-independent desktop hosting for cappy. [`WindowHost`] drives a [`cappy_core::Ui`]
//! and hands each frame to a [`WindowRenderer`] backend (wgpu in `cappy-render`, CPU in
//! `cappy-render-cpu`); [`select_renderer`] decides which backend a window uses.

mod regions;
mod renderer;
mod resize_pace;
mod selection;
mod session;
mod window_host;

pub use cappy_text::FontOptions;
pub use regions::{PixelRect, physical_regions};
pub use renderer::{
    PresentOutcome, RendererError, RendererKind, UnknownRendererError, WindowRenderer,
};
pub use selection::{RENDERER_ENV, RendererSelectionError, select_renderer};
pub use session::WindowSession;
pub use window_host::{
    InputResponse, RedrawResult, WindowHost, WindowOptions, memory_mark, set_log_memory,
};
