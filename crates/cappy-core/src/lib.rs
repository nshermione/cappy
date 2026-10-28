//! Core of the cappy GUI framework: reactive signals, a retained node tree, input handling and
//! damage tracking. Window-manager and GPU agnostic: hosts feed [`InputEvent`]s, call
//! [`Ui::update`], and hand [`Ui::prepare_frame`] output to a renderer.

mod damage;
mod element;
mod event;
mod paint;
mod reactive;
mod ui;

pub use damage::{Damage, DamageTracker};
pub use element::{Component, Element, div, text, text_dyn};
pub use event::{ButtonState, ClickEvent, CursorIcon, InputEvent, PointerButton};
pub use paint::{Primitive, QuadPrimitive, RenderFrame, TextPrimitive};
pub use reactive::{Signal, signal};
pub use ui::{FrameInput, NodeId, Ui, UpdateOutcome};

pub use cappy_layout::{
    ApproximateTextMeasurer, Point, Rect, Size, TextMeasureRequest, TextMeasurer,
};
pub use cappy_style::*;
