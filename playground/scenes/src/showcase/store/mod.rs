//! Singletons for one context. `cx.use_store(Name)` is the shared value; `Name::new` is a separate
//! instance.

mod app;
mod counter;

pub use app::{App, Tab};
pub use counter::Counter;
