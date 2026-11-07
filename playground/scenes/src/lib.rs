//! The playground scene shown by the winit host.

mod host;
mod showcase;

pub use cappy::Application;
#[cfg(any(feature = "gpu", feature = "cpu"))]
pub use cappy_winit::Run;
pub use host::{init_logging, window_options};
pub use showcase::showcase;
