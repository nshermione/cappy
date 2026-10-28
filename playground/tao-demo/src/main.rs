//! The playground scene in a tao window. Pass `--debug-damage` (or press F12) to see which
//! regions each frame repaints.

use std::process::ExitCode;

use playground_scenes::{init_logging, showcase, window_options};

fn main() -> ExitCode {
    init_logging();
    match cappy_tao::run(window_options("cappy playground (tao)"), showcase()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("tao demo failed: {error}");
            ExitCode::FAILURE
        }
    }
}
