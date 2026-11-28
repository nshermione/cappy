//! cappy: a retained-mode GUI framework on wgpu that repaints only what changed and stays idle
//! otherwise. It owns no window; a winit host feeds input, calls [`Ui::update`], and renders the
//! resulting frame.
//!
//! Features:
//! - `basic` (default): reactive core, styles and themes, layout, text, the renderer-independent
//!   window host, `element!`/`style!` macros and derives. Enough to build any component
//!   yourself.
//! - `gpu` (default): the wgpu renderer, as [`render`].
//! - `full`: `basic` and `gpu` plus the built-in component library.
//!
//! ```
//! use cappy::prelude::*;
//!
//! let _application = Application::new();
//! let count = Context::current().reactive(0_i32);
//! let root = element! {
//!     div(class = "app") {
//!         text(:format!("Clicked {} times", count.get()))
//!         div(class = "btn btn-primary", on_click = move |_| count.update(|n| *n += 1)) {
//!             text("Click")
//!         }
//!     }
//! };
//! let mut ui = Ui::new(root, Theme::light());
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

#[cfg(feature = "full")]
pub use cappy_components as components;

/// Everything an application usually needs, for `use cappy::prelude::*`.
#[cfg(feature = "basic")]
pub mod prelude {
    pub use cappy_core::{
        Align, Application, Color, ColorToken, Component, Context, CursorIcon, Display, Dynamic,
        Element, FlexDirection, FocusScope, FontSize, FontWeight, FrameInput, ImageClip,
        ImageDecodeError, ImagePixels, ImageSource, IntoElement, IntoReactive, Justify, ObjectFit,
        Overflow, PointerEvents, Position, Radius, Reactive, Signal, Slot, Space, StyleRefinement,
        Stylesheet, SurfaceId, SurfaceOptions, TextAlign, Theme, ThemeCatalog, ThemeHandle, Themes,
        Ui, UpdateOutcome, VideoSource, YuvPlanes, asset_path, canvas, div, for_each, fragment,
        hex, if_else, image, input,
        linear, list_window, on, on_before_mount, on_before_unmount, on_mounted, on_unmounted,
        video,
        on_updated, percent, reactive, shadow, show, slot, text, text_dyn, use_local_store,
        use_store,
    };
    pub use cappy_core::{Anim, AnimHandle, Canvas, CanvasFrame, CanvasTick, Ease, Motion, Repeat};
    pub use cappy_dsl::{element, style};
    pub use cappy_macros::{ClassName, Props, Reactive, component, store, style_token, stylesheet};
}
