//! wgpu renderer for cappy. Keeps the UI in a persistent offscreen texture, repaints only the
//! damaged regions of each [`cappy_core::RenderFrame`], and composites the result onto any
//! target, either its own window surface or a texture owned by the host.

mod batch;
mod composite;
mod font;
mod gpu_buffer;
mod measure;
mod quad;
mod renderer;
mod scissor;
mod shaping;
mod surface;
mod text;
mod window_host;

pub use font::FontContext;
pub use measure::CosmicTextMeasurer;
pub use renderer::{RenderError, RenderTarget, Renderer, TargetLoad};
pub use surface::{PresentOutcome, SurfaceError, WindowSurface};
pub use window_host::{InputResponse, RedrawResult, WindowHost, WindowOptions};

pub use glyphon::fontdb;
pub use wgpu;
