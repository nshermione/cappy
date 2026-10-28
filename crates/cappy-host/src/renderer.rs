//! The interface a window renderer backend implements, plus its result and error types.

use std::error::Error;
use std::fmt;
use std::str::FromStr;

use cappy_core::{Color, RenderFrame};

/// Which backend draws the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RendererKind {
    /// wgpu: smooth animation and cheap effects, but the GPU driver costs memory.
    Gpu,
    /// tiny-skia and softbuffer: far less memory, painting costs CPU time.
    Cpu,
}

impl RendererKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Gpu => "gpu",
            Self::Cpu => "cpu",
        }
    }
}

impl fmt::Display for RendererKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A renderer name that is neither `gpu` nor `cpu`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownRendererError {
    pub name: String,
}

impl fmt::Display for UnknownRendererError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown renderer {:?}; expected \"gpu\" or \"cpu\"",
            self.name
        )
    }
}

impl Error for UnknownRendererError {}

impl FromStr for RendererKind {
    type Err = UnknownRendererError;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        match name.trim().to_ascii_lowercase().as_str() {
            "gpu" => Ok(Self::Gpu),
            "cpu" => Ok(Self::Cpu),
            _ => Err(UnknownRendererError {
                name: name.to_owned(),
            }),
        }
    }
}

/// Result of [`WindowRenderer::present`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentOutcome {
    /// The frame is on screen. `repainted` regions were redrawn; zero means a pure re-composite.
    Presented { repainted: usize },
    /// No frame could be acquired this time; request another redraw.
    Retry,
    /// The window is hidden; wait for it to become visible before redrawing.
    Occluded,
}

/// A backend failure, tagged with the backend that produced it.
#[derive(Debug)]
pub struct RendererError {
    kind: RendererKind,
    source: Box<dyn Error + Send + Sync + 'static>,
}

impl RendererError {
    pub fn new(
        kind: RendererKind,
        source: impl Into<Box<dyn Error + Send + Sync + 'static>>,
    ) -> Self {
        Self {
            kind,
            source: source.into(),
        }
    }

    pub fn kind(&self) -> RendererKind {
        self.kind
    }
}

impl fmt::Display for RendererError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} renderer: {}", self.kind, self.source)
    }
}

impl Error for RendererError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}

/// Draws [`RenderFrame`]s into a window. Implementations keep the previous frame's pixels so
/// only the frame's damage has to be repainted.
pub trait WindowRenderer {
    fn kind(&self) -> RendererKind;

    /// Adapts to a new window size in physical pixels; zero-sized windows are ignored.
    fn resize(&mut self, physical_size: (u32, u32));

    /// Paints `frame`'s damage and shows the result over `background`.
    fn present(
        &mut self,
        frame: &RenderFrame,
        background: Color,
    ) -> Result<PresentOutcome, RendererError>;

    /// Outlines the regions repainted by the latest paint.
    fn set_debug_damage(&mut self, enabled: bool);

    fn debug_damage(&self) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_renderer_names_case_insensitively() {
        assert_eq!(" GPU ".parse(), Ok(RendererKind::Gpu));
        assert_eq!("cpu".parse(), Ok(RendererKind::Cpu));
        assert_eq!(
            "metal".parse::<RendererKind>(),
            Err(UnknownRendererError {
                name: "metal".to_owned()
            })
        );
    }
}
