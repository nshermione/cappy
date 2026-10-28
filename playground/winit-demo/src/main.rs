//! The playground scene in a winit window. Pass `--debug-damage` (or press F12) to see which
//! regions each frame repaints.

use std::process::ExitCode;

use playground_scenes::{init_logging, showcase, window_options};

fn main() -> ExitCode {
    init_logging();
    match cappy_winit::run(window_options("cappy playground (winit)"), showcase()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("winit demo failed: {error}");
            ExitCode::FAILURE
        }
    }
}
