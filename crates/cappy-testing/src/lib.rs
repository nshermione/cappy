//! Test support for cappy.
//!
//! - [`TestUi`] drives a UI headlessly: click, hover and inspect nodes by key, check damage.
//! - With the `gpu-tests` feature, [`gpu`] renders frames on a real (or software) GPU for pixel
//!   probes and PNG snapshots.
//! - With the `cpu-tests` feature, [`cpu`] does the same with the CPU renderer, on any machine.

mod harness;
#[cfg(any(feature = "gpu-tests", feature = "cpu-tests"))]
mod image;

#[cfg(feature = "cpu-tests")]
pub mod cpu;
#[cfg(feature = "gpu-tests")]
pub mod gpu;

pub use harness::TestUi;
#[cfg(any(feature = "gpu-tests", feature = "cpu-tests"))]
pub use image::{Image, ImageError, assert_snapshot};
