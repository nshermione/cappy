//! Core of the cappy GUI framework: reactive signals, a retained node tree, input handling and
//! damage tracking. Window-manager and GPU agnostic: hosts feed [`InputEvent`]s, call
//! [`Ui::update`], and hand [`Ui::prepare_frame`] output to a renderer.

mod binding;
mod bitmap;
mod context;
mod damage;
mod element;
mod event;
mod listener;
mod paint;
mod prop;
mod reactive;
mod theme_handle;
mod ui;

pub use context::{Application, Context, FieldInit, Singleton, SingletonOf};

pub use binding::{Dynamic, IntoClasses, IntoStyle, IntoText};
pub use bitmap::{ImagePixels, ImagePixelsError, ObjectFit};
pub use damage::{Damage, DamageTracker};
pub use element::{
    Component, Element, HasContext, Slot, div, for_each, fragment, if_else, image, show, slot,
    text, text_dyn, with_context,
};
pub use event::{ButtonState, ClickEvent, CursorIcon, InputEvent, PointerButton};
pub use listener::Listener;
pub use paint::{
    ImagePlacement, ImagePrimitive, Primitive, QuadPrimitive, RenderFrame, ShadowPrimitive,
    TextPrimitive, image_placement, paint_bounds, rounded_box_distance,
};
pub use prop::{ByHandle, ByValue, PropInput};
pub use reactive::{IntoReactive, Reactive, RetirePlaceholder, Signal};
pub use theme_handle::{ThemeCatalog, ThemeHandle, Themes};
pub use ui::{FrameInput, NodeId, ResourceCensus, Ui, UpdateOutcome};

pub use cappy_layout::{
    ApproximateTextMeasurer, Point, Rect, Size, TextMeasureRequest, TextMeasurer,
};
pub use cappy_style::*;
