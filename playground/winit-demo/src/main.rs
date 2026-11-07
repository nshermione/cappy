//! The playground scene in a winit window. Pass `--debug-damage` (or press F12) to see which
//! regions each frame repaints. Build with `--features cpu` and set `CAPPY_RENDERER=cpu` to draw
//! without the GPU.

use std::process::ExitCode;

use playground_scenes::{Application, Run, init_logging, showcase, window_options};

fn main() -> ExitCode {
    init_logging();
    let application = Application::new();
    match application.run(
        window_options("cappy playground (winit)"),
        showcase(application.context()),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("winit demo failed: {error}");
            ExitCode::FAILURE
        }
    }
}
