//! The playground scene in a browser canvas.
//!
//! ```sh
//! cd playground/web-demo
//! trunk serve --open
//! ```
//!
//! The page uses WebGPU when the browser has it, and WebGL otherwise. Inter is embedded because
//! this build cannot read the system font directory. The face is SIL Open Font License 1.1;
//! see `assets/OFL.txt`.

use cappy::host::FontOptions;
use playground_scenes::{Application, Run, showcase, window_options};

const INTER_REGULAR: &[u8] = include_bytes!("../assets/Inter-Regular.ttf");

fn main() {
    init_log();
    let application = Application::new();
    let options = window_options("cappy playground (web)")
        .with_fonts(FontOptions::new().with_data([INTER_REGULAR.to_vec()]));
    if let Err(error) = application.run(options, showcase(application.context())) {
        log::error!("web demo failed: {error}");
    }
}

fn init_log() {
    #[cfg(target_arch = "wasm32")]
    {
        console_error_panic_hook::set_once();
        let _ = console_log::init_with_level(log::Level::Info);
    }
    #[cfg(not(target_arch = "wasm32"))]
    playground_scenes::init_logging();
}
