//! Core of the cappy GUI framework: reactive signals, a retained node tree, input handling and
//! damage tracking. Window-manager and GPU agnostic: hosts feed [`InputEvent`]s, call
//! [`Ui::update`], and hand [`Ui::prepare_frame`] output to a renderer.

mod binding;
mod damage;
mod element;
mod event;
mod listener;
mod paint;
mod prop;
mod reactive;
mod ui;

pub use binding::{Dynamic, IntoClasses, IntoStyle, IntoText};
pub use damage::{Damage, DamageTracker};
pub use element::{
    Component, Element, Slot, div, for_each, fragment, if_else, show, slot, text, text_dyn,
};
pub use event::{ButtonState, ClickEvent, CursorIcon, InputEvent, PointerButton};
pub use listener::Listener;
pub use paint::{Primitive, QuadPrimitive, RenderFrame, TextPrimitive};
pub use prop::{ByHandle, ByValue, PropInput};
pub use reactive::{IntoReactive, Reactive, RetirePlaceholder, Signal, reactive, signal};
pub use ui::{FrameInput, NodeId, Ui, UpdateOutcome};

pub use cappy_layout::{
    ApproximateTextMeasurer, Point, Rect, Size, TextMeasureRequest, TextMeasurer,
};
pub use cappy_style::*;
