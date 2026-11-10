//! Monotonic clock for animation, scroll easing and frame deadlines.
//!
//! Callers use [`Instant`] from here. The platform module is the only place that
//! names `std` or `web_time`. [`Duration`] is [`std::time::Duration`] on every
//! target, because both clocks add that type. [`std::time::Instant::now`] panics
//! on `wasm32-unknown-unknown`, so that target reads the browser performance clock.

pub use std::time::Duration;

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
mod platform {
    pub use std::time::Instant;
}

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
mod platform {
    pub use web_time::Instant;
}

pub use platform::Instant;
