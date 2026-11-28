//! The shell. Each component is its own file. The open tab is state on [`App`].

mod app;
mod header;
mod tab_button;

pub use app::App;
