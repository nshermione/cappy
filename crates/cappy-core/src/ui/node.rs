//! Retained nodes stored in a flat arena.

use std::sync::Arc;

use cappy_layout::{Point, Rect};
use cappy_style::{ClassName, InteractionState, Style, StyleRefinement, Stylesheet};

use crate::bitmap::{ImagePixels, ObjectFit};

use crate::element::{ClickHandler, HoverHandler};

/// One direct child of a node: a mounted element, or a control-flow block.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ChildPart {
    Node(NodeId),
    Flow(u32),
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
        pixels: Arc<ImagePixels>,
        fit: ObjectFit,
    },
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
            NodeContent::Container | NodeContent::Image { .. } => None,
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
