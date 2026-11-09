//! cappy: a retained-mode GUI framework on wgpu that repaints only what changed and stays idle
//! otherwise. It owns no window; a winit host feeds input, calls [`Ui::update`], and renders the
//! resulting frame.
//!
//! Features:
//! - `basic` (default): reactive core, styles and themes, layout, text, the renderer-independent
//!   window host, `element!`/`style!` macros and derives. Enough to build any component
//!   yourself.
//! - `gpu` (default): the wgpu renderer, as [`render`].
//! - `cpu`: the tiny-skia + softbuffer renderer, as [`render_cpu`]. It loads no GPU driver, so it
//!   needs far less memory.
//! - `full`: `basic` and `gpu` plus the built-in component library.
//!
//! ```
//! use cappy::prelude::*;
//!
//! let application = Application::new();
//! let cx = application.context();
//! let count = cx.reactive(0_i32);
//! let root = element!((cx) {
//!     div(class = "app") {
//!         text(:format!("Clicked {} times", count.get()))
//!         div(class = "btn btn-primary", on_click = move |_| count.update(|n| *n += 1)) {
//!             text("Click")
//!         }
//!     }
//! });
//! let mut ui = Ui::new(cx, root, Theme::light());
//! assert!(ui.update(FrameInput::new(800, 600, 1.0)).needs_redraw());
//! ```

#[cfg(feature = "basic")]
pub use cappy_core::*;
#[cfg(feature = "basic")]
pub use cappy_dsl::{element, style};
#[cfg(feature = "basic")]
pub use cappy_macros::{ClassName, Props, Reactive, component, store, style_token, stylesheet};

/// Fonts, shaping and text measurement shared by the renderers.
#[cfg(feature = "basic")]
pub mod text {
    pub use cappy_text::*;
}

/// The renderer-independent window host: window options, renderer selection and the
/// [`host::WindowRenderer`] interface backends implement.
#[cfg(feature = "basic")]
pub mod host {
    pub use cappy_host::*;
}

/// The wgpu renderer: offscreen damage repainting, compositing and a window-surface helper.
#[cfg(feature = "gpu")]
pub mod render {
    pub use cappy_render::*;
}

/// The CPU renderer: tiny-skia damage repainting and a softbuffer window surface.
#[cfg(feature = "cpu")]
pub mod render_cpu {
    pub use cappy_render_cpu::*;
}

#[cfg(feature = "full")]
pub use cappy_components as components;

/// Everything an application usually needs, for `use cappy::prelude::*`.
#[cfg(feature = "basic")]
pub mod prelude {
    pub use cappy_core::{
        Align, Application, Color, ColorToken, Component, Container, Context, Display, Dynamic,
        Element, FlexDirection, FontSize, FontWeight, FrameInput, HasContext, Image, ImagePixels,
        IntoElement, IntoReactive, Justify, ObjectFit, Radius, Reactive, Signal, Slot, Space,
        StyleRefinement, Stylesheet, Text, Theme, ThemeCatalog, ThemeHandle, Themes, Ui,
        UpdateOutcome, div, for_each, fragment, hex, if_else, image, linear, on, percent, reactive,
        shadow, show, slot, text, text_dyn, use_local_store, use_store,
    };
    pub use cappy_dsl::{element, style};
    pub use cappy_macros::{ClassName, Props, Reactive, component, store, style_token, stylesheet};
}
