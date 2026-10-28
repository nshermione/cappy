//! cappy: a retained-mode GUI framework on wgpu that repaints only what changed and stays idle
//! otherwise. It owns no window; hosts (winit, tao, mobile shells) feed input, call
//! [`Ui::update`], and render the resulting frame.
//!
//! Features:
//! - `basic` (default): reactive core, styles and themes, layout, renderer, `ui!`/`style!`/
//!   `theme!` macros and derives. Enough to build any component yourself.
//! - `full`: `basic` plus the built-in component library.
//!
//! ```
//! use cappy::prelude::*;
//!
//! let count = signal(0);
//! let app = ui! {
//!     div(class = "app") {
//!         text(move || format!("Clicked {} times", count.get()))
//!         div(class = "btn btn-primary", on_click = move |_| count.update(|n| *n += 1)) {
//!             text("Click")
//!         }
//!     }
//! };
//! let mut ui = Ui::new(app, Theme::light());
//! assert!(ui.update(FrameInput::new(800, 600, 1.0)).needs_redraw());
//! ```

#[cfg(feature = "basic")]
pub use cappy_core::*;
#[cfg(feature = "basic")]
pub use cappy_dsl::{style, theme, ui};
#[cfg(feature = "basic")]
pub use cappy_macros::{ClassName, Props};

/// The wgpu renderer: offscreen damage repainting, compositing and a window-surface helper.
#[cfg(feature = "basic")]
pub mod render {
    pub use cappy_render::*;
}

#[cfg(feature = "full")]
pub use cappy_components as components;

/// Everything an application usually needs, for `use cappy::prelude::*`.
#[cfg(feature = "basic")]
pub mod prelude {
    pub use cappy_core::{
        Align, Color, ColorToken, Component, Display, Element, FlexDirection, FontSize, FontWeight,
        FrameInput, Justify, Radius, Signal, Space, StyleRefinement, Theme, Ui, UpdateOutcome, div,
        hex, percent, signal, text, text_dyn,
    };
    pub use cappy_dsl::{style, theme, ui};
    pub use cappy_macros::{ClassName, Props};
}
