//! Settings shared by the playground hosts: logging and window options.

use cappy::render::WindowOptions;

const DEBUG_DAMAGE_FLAG: &str = "--debug-damage";
const DEBUG_DAMAGE_ENV: &str = "CAPPY_DEBUG_DAMAGE";
/// Per-frame repaint statistics are logged by `cappy_render` at debug level.
const DEFAULT_LOG_FILTER: &str = "info,cappy_render=debug";

/// Starts logging; `RUST_LOG` overrides the default filter, which shows per-frame repaints.
pub fn init_logging() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(DEFAULT_LOG_FILTER))
        .init();
}

/// Window options for a playground host, honouring the debug damage flag.
pub fn window_options(title: &str) -> WindowOptions {
    WindowOptions::new(title).with_debug_damage(debug_damage_requested())
}

/// Whether the damage overlay was requested with `--debug-damage` or `CAPPY_DEBUG_DAMAGE=1`.
fn debug_damage_requested() -> bool {
    std::env::args().any(|argument| argument == DEBUG_DAMAGE_FLAG)
        || std::env::var(DEBUG_DAMAGE_ENV).is_ok_and(|value| value == "1")
}
