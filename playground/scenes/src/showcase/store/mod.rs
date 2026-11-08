//! Singletons for one context. `use_store(Name)` is the shared value. `use_local_store(Name)` is a
//! separate instance with default fields. `Name::new` is a separate
//! instance.

mod app;
mod counter;

pub use app::{App, Tab};
pub use counter::Counter;
