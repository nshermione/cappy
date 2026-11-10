//! Settings shared by the playground hosts: logging and window options.

use cappy::host::WindowOptions;

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
const DEBUG_DAMAGE_FLAG: &str = "--debug-damage";
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
const DEBUG_DAMAGE_ENV: &str = "CAPPY_DEBUG_DAMAGE";
/// Startup and errors only. Per-frame lines and memory samples stay off until
/// [`WindowOptions::with_log_frames`] or [`WindowOptions::with_log_memory`] is set.
const DEFAULT_LOG_FILTER: &str = "info";

/// Starts logging. `RUST_LOG` overrides this filter. Frame repaint lines are a window option,
/// not part of the default filter.
pub fn init_logging() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(DEFAULT_LOG_FILTER))
        .init();
}

/// Window options for a playground host, honouring the debug damage flag. The renderer is
/// chosen by the host's cargo features and `CAPPY_RENDERER`.
pub fn window_options(title: &str) -> WindowOptions {
    WindowOptions::new(title).with_debug_damage(debug_damage_requested())
}

/// Whether the damage overlay was requested with `--debug-damage` or `CAPPY_DEBUG_DAMAGE=1`.
/// The browser has no process arguments, so a web build never starts with the overlay.
fn debug_damage_requested() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::args().any(|argument| argument == DEBUG_DAMAGE_FLAG)
            || std::env::var(DEBUG_DAMAGE_ENV).is_ok_and(|value| value == "1")
    }
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
}
