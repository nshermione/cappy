//! Retained nodes stored in a flat arena.

use std::sync::Arc;

use cappy_layout::{Point, Rect};
use cappy_style::{ClassName, InteractionState, Style, StyleRefinement, Stylesheet};

use std::time::Duration;

use crate::bitmap::{ImageClip, ImagePixels, ObjectFit};
use crate::image_load::ImageLoad;
use crate::time::Instant;

use crate::element::{
    ClickHandler, FocusHandler, HoverHandler, ImeHandler, InputHandler, KeyHandler, PointerHandler,
    TextHandler,
};
use crate::ui::edit::TextEdit;

/// One direct child of a node: a mounted element, a control-flow block, or a component boundary.
///
/// A component boundary is not a layout node. Flattening splices the nodes it owns into the parent.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ChildPart {
    Node(NodeId),
    Flow(u32),
    Component(ComponentId),
}

/// Handle to a mounted component boundary.
///
/// `generation` changes when the slot is recycled, so a handle from a disposed component does not
/// refer to whatever is mounted there later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ComponentId {
    index: u32,
    generation: u32,
}

impl ComponentId {
    pub(crate) fn new(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }

    pub(crate) fn index(self) -> usize {
        self.index as usize
    }

    pub(crate) fn generation(self) -> u32 {
        self.generation
    }

    pub(crate) fn part(self) -> ChildPart {
        ChildPart::Component(self)
    }
}

/// Handle to a mounted node.
///
/// `generation` changes when the slot is recycled, so a handle from a disposed node does not
/// refer to whatever is mounted there later.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId {
    index: u32,
    generation: u32,
}

impl NodeId {
    pub(crate) fn new(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }

    /// Node `index` at generation 0. Valid until that slot is recycled.
    pub(crate) fn from_index(index: u32) -> Self {
        Self::new(index, 0)
    }

    pub(crate) fn index(self) -> usize {
        self.index as usize
    }

    pub(crate) fn generation(self) -> u32 {
        self.generation
    }
}

pub(crate) enum NodeContent {
    Container,
    Text(Arc<str>),
    Image {
        clip: ImageClip,
        pixels: Arc<ImagePixels>,
        fit: ObjectFit,
        elapsed: Duration,
        /// `None` while the image is hidden, so that gap is not added to `elapsed`.
        clock: Option<Instant>,
        shown: usize,
        load: Option<ImageLoad>,
    },
    Input(InputField),
}

/// Editing state of one single-line input.
pub(crate) struct InputField {
    pub(crate) edit: TextEdit,
    pub(crate) placeholder: String,
    pub(crate) disabled: bool,
    pub(crate) scroll_x: f32,
    pub(crate) on_input: Option<InputHandler>,
    pub(crate) on_submit: Option<InputHandler>,
    pub(crate) writer: Option<Box<dyn Fn(String)>>,
    /// Last string written to [`Self::writer`], so an echo of that write does not move the caret.
    pub(crate) published: String,
    /// Text event equal to the last IME commit, swallowed so a platform that emits both inserts once.
    pub(crate) swallow_text: Option<String>,
}

pub(crate) struct Node {
    pub(crate) parent: Option<NodeId>,
    pub(crate) children: Vec<NodeId>,
    pub(crate) key: Option<String>,
    pub(crate) content: NodeContent,
    pub(crate) classes: Vec<ClassName>,
    pub(crate) dynamic_classes: Vec<ClassName>,
    /// Sheet of this node only. Children do not inherit it.
    pub(crate) stylesheet: Option<Arc<Stylesheet>>,
    pub(crate) inline_style: StyleRefinement,
    pub(crate) dynamic_style: StyleRefinement,
    pub(crate) style: Style,
    pub(crate) interaction: InteractionState,
    pub(crate) on_click: Option<ClickHandler>,
    pub(crate) on_hover: Option<HoverHandler>,
    pub(crate) focusable: bool,
    pub(crate) disabled: bool,
    pub(crate) on_key: Option<KeyHandler>,
    pub(crate) on_text: Option<TextHandler>,
    pub(crate) on_ime: Option<ImeHandler>,
    pub(crate) on_focus: Option<FocusHandler>,
    pub(crate) on_pointer: Option<PointerHandler>,
    /// Caret byte index drawn on a text leaf. Inputs keep theirs on [`InputField`].
    pub(crate) caret: Option<usize>,
    pub(crate) selection: Option<(usize, usize)>,
    /// Ordered children before flows are flattened into [`Node::children`](Self::children).
    pub(crate) parts: Vec<ChildPart>,
    /// `show` is hiding this node, or a parent `show` is.
    pub(crate) suppressed: bool,
    pub(crate) alive: bool,
    /// Bumped when this slot is mounted again after dispose.
    pub(crate) generation: u32,
    pub(crate) bindings: Vec<crate::reactive::BindingId>,
    /// Distance the content is shifted, in logical pixels. Positive moves content up and left.
    pub(crate) scroll: Point,
    /// Where [`Self::scroll`] is easing toward.
    pub(crate) scroll_target: Point,
    /// Absolute rectangle in logical pixels from the last layout, plus this node's translation.
    pub(crate) rect: Rect,
    /// Ink painted last frame, including shadow that falls outside [`Self::rect`].
    pub(crate) painted: Rect,
    /// Ancestor clip from the last place, in logical pixels. `None` means nothing clips this node.
    pub(crate) clip: Option<Rect>,
    /// Visual offset added after layout. Identity is zero.
    pub(crate) translation: Point,
    /// Multiplier applied while painting this node and its descendants. Identity is 1.
    pub(crate) opacity: f32,
    /// Solid fill owned by a background tween. `None` uses the resolved style.
    pub(crate) background: Option<cappy_style::Color>,
    /// Visual scale around this node's center. Identity is 1. Layout size stays put.
    pub(crate) scale: f32,
    /// Visual rotation in radians, clockwise in y-down space. Identity is 0.
    pub(crate) rotation: f32,
    pub(crate) translation_tween: Option<crate::ui::motion::Tween<Point>>,
    pub(crate) opacity_tween: Option<crate::ui::motion::Tween<f32>>,
    pub(crate) background_tween: Option<crate::ui::motion::Tween<cappy_style::Color>>,
    pub(crate) scale_tween: Option<crate::ui::motion::Tween<f32>>,
    pub(crate) rotation_tween: Option<crate::ui::motion::Tween<f32>>,
    pub(crate) shake: Option<crate::ui::motion::Wave>,
    pub(crate) style_dirty: bool,
    pub(crate) paint_dirty: bool,
}

impl Node {
    pub(crate) fn text(&self) -> Option<&str> {
        match &self.content {
            NodeContent::Text(text) => Some(text),
            NodeContent::Container | NodeContent::Image { .. } | NodeContent::Input(_) => None,
        }
    }

    pub(crate) fn is_text(&self) -> bool {
        matches!(self.content, NodeContent::Text(_))
    }

    pub(crate) fn all_classes(&self) -> Vec<ClassName> {
        self.classes
            .iter()
            .chain(&self.dynamic_classes)
            .cloned()
            .collect()
    }

    pub(crate) fn effective_inline_style(&self) -> StyleRefinement {
        let mut inline = self.inline_style.clone();
        inline.merge(&self.dynamic_style);
        inline
    }
}
