//! wgpu renderer for cappy. Keeps the UI in a persistent offscreen texture, repaints only the
//! damaged regions of each [`cappy_core::RenderFrame`], and composites the result onto any
//! target, either its own window surface or a texture owned by the host.

mod batch;
mod composite;
mod gpu_buffer;
mod quad;
mod renderer;
mod surface;
mod text;

pub use renderer::{RenderError, RenderTarget, Renderer, TargetLoad};
pub use surface::{SurfaceError, WindowSurface, open_window_renderer};

pub use cappy_host::{PresentOutcome, WindowRenderer};
pub use cappy_text::{CosmicTextMeasurer, FontContext, fontdb};
pub use wgpu;
