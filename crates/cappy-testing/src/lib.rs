//! Test support for cappy.
//!
//! - [`TestUi`] drives a UI headlessly: click, hover and inspect nodes by key, check damage.
//! - With the `gpu-tests` feature, [`gpu`] renders frames on a real (or software) GPU for pixel
//!   probes and PNG snapshots.

mod harness;

#[cfg(feature = "gpu-tests")]
pub mod gpu;

pub use harness::TestUi;
