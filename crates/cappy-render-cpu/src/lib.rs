//! CPU renderer for cappy: paints [`cappy_core::RenderFrame`]s with tiny-skia into a persistent
//! pixmap, repainting only damaged regions, and shows it in a window through softbuffer.
//!
//! It loads no GPU driver, so a window costs far less memory than with the wgpu renderer, at the
//! price of CPU time per repainted pixel. Text is shaped by the same cosmic-text code as the GPU
//! renderer, so layout is identical.
//!
//! Known difference from the GPU renderer: tiny-skia blends sRGB-encoded values while wgpu
//! blends in linear light, so anti-aliased edges and translucent colors differ slightly.

mod blend;
mod overlay;
mod painter;
mod quad;
mod surface;
mod text;

pub use painter::CpuRenderer;
pub use surface::{CpuSurfaceError, CpuWindowSurface, open_window_renderer};
