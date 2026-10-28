//! The playground scene shared by every host (winit, tao), so each one shows the same UI and
//! the hosts can be compared side by side.

mod host;
mod showcase;

pub use host::{init_logging, window_options};
pub use showcase::showcase;
