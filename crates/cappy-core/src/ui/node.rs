//! Retained nodes stored in a flat arena.

use std::sync::Arc;

use cappy_layout::Rect;
use cappy_style::{ClassName, InteractionState, Style, StyleRefinement};

use crate::element::{ClickHandler, HoverHandler};

/// Handle to a mounted node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub(crate) u32);

impl NodeId {
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

pub(crate) enum NodeContent {
    Container,
    Text(Arc<str>),
}

pub(crate) struct Node {
    pub(crate) parent: Option<NodeId>,
    pub(crate) children: Vec<NodeId>,
    pub(crate) key: Option<String>,
    pub(crate) content: NodeContent,
    pub(crate) classes: Vec<ClassName>,
    pub(crate) dynamic_classes: Vec<ClassName>,
    pub(crate) inline_style: StyleRefinement,
    pub(crate) dynamic_style: StyleRefinement,
    pub(crate) style: Style,
    pub(crate) interaction: InteractionState,
    pub(crate) on_click: Option<ClickHandler>,
    pub(crate) on_hover: Option<HoverHandler>,
    /// Absolute rectangle in logical pixels from the last layout.
    pub(crate) rect: Rect,
    pub(crate) style_dirty: bool,
    pub(crate) paint_dirty: bool,
}

impl Node {
    pub(crate) fn text(&self) -> Option<&str> {
        match &self.content {
            NodeContent::Text(text) => Some(text),
            NodeContent::Container => None,
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
