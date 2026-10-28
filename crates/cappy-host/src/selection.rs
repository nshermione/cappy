//! Picks the renderer backend from the environment, the window options and what was compiled.

use std::fmt;

use crate::{RendererKind, UnknownRendererError};

/// Overrides the renderer chosen in code, e.g. `CAPPY_RENDERER=cpu`.
pub const RENDERER_ENV: &str = "CAPPY_RENDERER";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RendererSelectionError {
    /// [`RENDERER_ENV`] holds something other than a renderer name.
    InvalidEnv(UnknownRendererError),
    /// The requested backend was not compiled in; enable its cargo feature.
    Unavailable(RendererKind),
    /// The host was built without any renderer backend.
    NoneCompiled,
}

impl fmt::Display for RendererSelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEnv(error) => write!(f, "invalid {RENDERER_ENV}: {error}"),
            Self::Unavailable(kind) => write!(
                f,
                "the {kind} renderer is not compiled in; enable the `{kind}` cargo feature"
            ),
            Self::NoneCompiled => f.write_str("no renderer backend is compiled in"),
        }
    }
}

impl std::error::Error for RendererSelectionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidEnv(error) => Some(error),
            Self::Unavailable(_) | Self::NoneCompiled => None,
        }
    }
}

/// Chooses the backend: [`RENDERER_ENV`] first, then `requested`, then the first entry of
/// `available` (listed in order of preference). Never falls back silently: asking for a backend
/// that is not in `available` is an error.
pub fn select_renderer(
    requested: Option<RendererKind>,
    available: &[RendererKind],
) -> Result<RendererKind, RendererSelectionError> {
    let env_value = std::env::var(RENDERER_ENV).ok();
    resolve_renderer(env_value.as_deref(), requested, available)
}

fn resolve_renderer(
    env_value: Option<&str>,
    requested: Option<RendererKind>,
    available: &[RendererKind],
) -> Result<RendererKind, RendererSelectionError> {
    let from_env = env_value
        .filter(|value| !value.trim().is_empty())
        .map(str::parse)
        .transpose()
        .map_err(RendererSelectionError::InvalidEnv)?;
    match from_env.or(requested) {
        Some(kind) if available.contains(&kind) => Ok(kind),
        Some(kind) => Err(RendererSelectionError::Unavailable(kind)),
        None => available
            .first()
            .copied()
            .ok_or(RendererSelectionError::NoneCompiled),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTH: &[RendererKind] = &[RendererKind::Gpu, RendererKind::Cpu];

    #[test]
    fn defaults_to_the_first_available_backend() {
        assert_eq!(resolve_renderer(None, None, BOTH), Ok(RendererKind::Gpu));
        assert_eq!(
            resolve_renderer(None, None, &[RendererKind::Cpu]),
            Ok(RendererKind::Cpu)
        );
    }

    #[test]
    fn options_override_the_default() {
        assert_eq!(
            resolve_renderer(None, Some(RendererKind::Cpu), BOTH),
            Ok(RendererKind::Cpu)
        );
    }

    #[test]
    fn environment_overrides_the_options() {
        assert_eq!(
            resolve_renderer(Some("gpu"), Some(RendererKind::Cpu), BOTH),
            Ok(RendererKind::Gpu)
        );
    }

    #[test]
    fn blank_environment_is_ignored() {
        assert_eq!(
            resolve_renderer(Some("  "), Some(RendererKind::Cpu), BOTH),
            Ok(RendererKind::Cpu)
        );
    }

    #[test]
    fn invalid_environment_is_an_error() {
        assert!(matches!(
            resolve_renderer(Some("vulkan"), None, BOTH),
            Err(RendererSelectionError::InvalidEnv(_))
        ));
    }

    #[test]
    fn missing_backends_are_reported_instead_of_replaced() {
        assert_eq!(
            resolve_renderer(Some("cpu"), None, &[RendererKind::Gpu]),
            Err(RendererSelectionError::Unavailable(RendererKind::Cpu))
        );
        assert_eq!(
            resolve_renderer(None, None, &[]),
            Err(RendererSelectionError::NoneCompiled)
        );
    }
}
