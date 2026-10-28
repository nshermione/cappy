//! Layout tree backed by taffy, keyed by the caller's node ids.

use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;

use cappy_style::{Align, Display, Extent, FlexDirection, Justify, LayoutStyle};
use taffy::prelude::{
    AvailableSpace, Dimension as TaffyDimension, LengthPercentage, LengthPercentageAuto, TaffyAuto,
    TaffyTree,
};

use crate::{Rect, Size, TextMeasureRequest, TextMeasurer};

/// Text content of a leaf node, measured during layout.
#[derive(Clone, Debug, PartialEq)]
pub struct TextLeaf {
    pub text: String,
    pub font_size: f32,
    pub line_height: f32,
    pub weight: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LayoutError {
    /// The key was never inserted, or was removed.
    UnknownNode(String),
    /// Taffy rejected an operation.
    Engine(String),
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownNode(key) => write!(f, "layout node {key} does not exist"),
            Self::Engine(message) => write!(f, "layout engine error: {message}"),
        }
    }
}

impl std::error::Error for LayoutError {}

fn engine_error(error: taffy::TaffyError) -> LayoutError {
    LayoutError::Engine(format!("{error:?}"))
}

/// Mirrors the caller's tree in taffy and computes rectangles relative to each parent.
pub struct LayoutTree<K> {
    taffy: TaffyTree<TextLeaf>,
    nodes: HashMap<K, taffy::NodeId>,
}

impl<K> Default for LayoutTree<K> {
    fn default() -> Self {
        Self {
            taffy: TaffyTree::new(),
            nodes: HashMap::new(),
        }
    }
}

impl<K: Copy + Eq + Hash + fmt::Debug> LayoutTree<K> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, key: K, style: &LayoutStyle) -> Result<(), LayoutError> {
        let node = self
            .taffy
            .new_leaf(to_taffy_style(style))
            .map_err(engine_error)?;
        if let Some(previous) = self.nodes.insert(key, node) {
            self.taffy.remove(previous).map_err(engine_error)?;
        }
        Ok(())
    }

    pub fn set_style(&mut self, key: K, style: &LayoutStyle) -> Result<(), LayoutError> {
        let node = self.node(key)?;
        self.taffy
            .set_style(node, to_taffy_style(style))
            .map_err(engine_error)
    }

    /// Sets or clears the text measured for a leaf, and invalidates its cached size.
    pub fn set_text(&mut self, key: K, text: Option<TextLeaf>) -> Result<(), LayoutError> {
        let node = self.node(key)?;
        self.taffy
            .set_node_context(node, text)
            .map_err(engine_error)?;
        self.taffy.mark_dirty(node).map_err(engine_error)
    }

    pub fn set_children(&mut self, key: K, children: &[K]) -> Result<(), LayoutError> {
        let node = self.node(key)?;
        let child_nodes = children
            .iter()
            .map(|child| self.node(*child))
            .collect::<Result<Vec<_>, _>>()?;
        self.taffy
            .set_children(node, &child_nodes)
            .map_err(engine_error)
    }

    /// Lays out the subtree under `root` within `available` logical pixels.
    pub fn compute(
        &mut self,
        root: K,
        available: Size,
        measurer: &mut dyn TextMeasurer,
    ) -> Result<(), LayoutError> {
        let root_node = self.node(root)?;
        let space = taffy::Size {
            width: AvailableSpace::Definite(available.width),
            height: AvailableSpace::Definite(available.height),
        };
        self.taffy
            .compute_layout_with_measure(root_node, space, |inputs, _node, leaf, style| {
                taffy::compute_leaf_layout(
                    inputs,
                    style,
                    |_, _| 0.0,
                    |known, available_space| {
                        measure_leaf(leaf.as_deref(), known, available_space, measurer)
                    },
                )
            })
            .map_err(engine_error)
    }

    /// Rectangle of `key` relative to its parent, from the last [`LayoutTree::compute`].
    pub fn local_rect(&self, key: K) -> Result<Rect, LayoutError> {
        let layout = self.taffy.layout(self.node(key)?).map_err(engine_error)?;
        Ok(Rect::new(
            layout.location.x,
            layout.location.y,
            layout.size.width,
            layout.size.height,
        ))
    }

    fn node(&self, key: K) -> Result<taffy::NodeId, LayoutError> {
        self.nodes
            .get(&key)
            .copied()
            .ok_or_else(|| LayoutError::UnknownNode(format!("{key:?}")))
    }
}

fn measure_leaf(
    leaf: Option<&TextLeaf>,
    known: taffy::Size<Option<f32>>,
    available: taffy::Size<AvailableSpace>,
    measurer: &mut dyn TextMeasurer,
) -> taffy::Size<f32> {
    let Some(leaf) = leaf else {
        return taffy::Size {
            width: known.width.unwrap_or(0.0),
            height: known.height.unwrap_or(0.0),
        };
    };
    if let (Some(width), Some(height)) = (known.width, known.height) {
        return taffy::Size { width, height };
    }
    let max_width = known.width.or(match available.width {
        AvailableSpace::Definite(width) => Some(width),
        AvailableSpace::MinContent => Some(0.0),
        AvailableSpace::MaxContent => None,
    });
    let request = TextMeasureRequest {
        text: &leaf.text,
        font_size: leaf.font_size,
        line_height: leaf.line_height,
        weight: leaf.weight,
    };
    let measured = measurer.measure(&request, max_width);
    taffy::Size {
        width: known.width.unwrap_or(measured.width),
        height: known.height.unwrap_or(measured.height),
    }
}

fn to_taffy_style(style: &LayoutStyle) -> taffy::Style {
    let edges = |edges: &cappy_style::Edges| taffy::Rect {
        left: LengthPercentage::length(edges.left),
        right: LengthPercentage::length(edges.right),
        top: LengthPercentage::length(edges.top),
        bottom: LengthPercentage::length(edges.bottom),
    };
    let margin = |edges: &cappy_style::Edges| taffy::Rect {
        left: LengthPercentageAuto::length(edges.left),
        right: LengthPercentageAuto::length(edges.right),
        top: LengthPercentageAuto::length(edges.top),
        bottom: LengthPercentageAuto::length(edges.bottom),
    };
    let gap = LengthPercentage::length(style.gap);
    let mut taffy_style = taffy::Style {
        display: match style.display {
            Display::Flex => taffy::Display::Flex,
            Display::Grid => taffy::Display::Grid,
            Display::None => taffy::Display::None,
        },
        flex_direction: match style.direction {
            FlexDirection::Row => taffy::FlexDirection::Row,
            FlexDirection::Column => taffy::FlexDirection::Column,
        },
        flex_wrap: if style.wrap {
            taffy::FlexWrap::Wrap
        } else {
            taffy::FlexWrap::NoWrap
        },
        justify_content: match style.justify {
            Justify::Start => taffy::JustifyContent::START,
            Justify::Center => taffy::JustifyContent::CENTER,
            Justify::End => taffy::JustifyContent::END,
            Justify::SpaceBetween => taffy::JustifyContent::SPACE_BETWEEN,
            Justify::SpaceAround => taffy::JustifyContent::SPACE_AROUND,
            Justify::SpaceEvenly => taffy::JustifyContent::SPACE_EVENLY,
        },
        align_items: match style.align {
            Align::Start => taffy::AlignItems::START,
            Align::Center => taffy::AlignItems::CENTER,
            Align::End => taffy::AlignItems::END,
            Align::Stretch => taffy::AlignItems::STRETCH,
        },
        gap: taffy::Size {
            width: gap,
            height: gap,
        },
        padding: edges(&style.padding),
        margin: margin(&style.margin),
        size: taffy::Size {
            width: dimension(style.width),
            height: dimension(style.height),
        },
        min_size: taffy::Size {
            width: length_or_auto(style.min_width),
            height: length_or_auto(style.min_height),
        },
        max_size: taffy::Size {
            width: length_or_auto(style.max_width),
            height: length_or_auto(style.max_height),
        },
        flex_grow: style.flex_grow,
        flex_shrink: style.flex_shrink,
        ..Default::default()
    };
    if style.display == Display::Grid && style.grid_columns > 0 {
        taffy_style.grid_template_columns =
            taffy::style_helpers::evenly_sized_tracks(style.grid_columns);
    }
    taffy_style
}

fn dimension(extent: Extent) -> TaffyDimension {
    match extent {
        Extent::Auto => TaffyDimension::AUTO,
        Extent::Px(value) => TaffyDimension::length(value),
        Extent::Percent(value) => TaffyDimension::percent(value / 100.0),
    }
}

fn length_or_auto(extent: Extent) -> LengthPercentageAuto {
    match extent {
        Extent::Auto => LengthPercentageAuto::AUTO,
        Extent::Px(value) => LengthPercentageAuto::length(value),
        Extent::Percent(value) => LengthPercentageAuto::percent(value / 100.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ApproximateTextMeasurer;
    use cappy_style::Edges;

    fn column(gap: f32, padding: f32) -> LayoutStyle {
        LayoutStyle {
            direction: FlexDirection::Column,
            gap,
            padding: Edges::all(padding),
            flex_shrink: 1.0,
            ..LayoutStyle::default()
        }
    }

    fn fixed(width: f32, height: f32) -> LayoutStyle {
        LayoutStyle {
            width: Extent::Px(width),
            height: Extent::Px(height),
            ..LayoutStyle::default()
        }
    }

    fn text_leaf(text: &str) -> TextLeaf {
        TextLeaf {
            text: text.to_owned(),
            font_size: 10.0,
            line_height: 14.0,
            weight: 400,
        }
    }

    #[test]
    fn percent_sized_root_fills_the_available_space() {
        let mut tree = LayoutTree::new();
        let root = LayoutStyle {
            width: Extent::Percent(100.0),
            height: Extent::Percent(100.0),
            ..LayoutStyle::default()
        };
        tree.insert(0, &root).unwrap();
        tree.compute(
            0,
            Size::new(320.0, 240.0),
            &mut ApproximateTextMeasurer::default(),
        )
        .unwrap();

        assert_eq!(
            tree.local_rect(0).unwrap(),
            Rect::new(0.0, 0.0, 320.0, 240.0)
        );
    }

    #[test]
    fn stacks_children_in_a_padded_column() {
        let mut tree = LayoutTree::new();
        tree.insert(0, &column(4.0, 10.0)).unwrap();
        tree.insert(1, &fixed(50.0, 20.0)).unwrap();
        tree.insert(2, &fixed(30.0, 10.0)).unwrap();
        tree.set_children(0, &[1, 2]).unwrap();
        tree.compute(
            0,
            Size::new(200.0, 100.0),
            &mut ApproximateTextMeasurer::default(),
        )
        .unwrap();

        assert_eq!(tree.local_rect(0).unwrap(), Rect::new(0.0, 0.0, 70.0, 54.0));
        assert_eq!(
            tree.local_rect(1).unwrap(),
            Rect::new(10.0, 10.0, 50.0, 20.0)
        );
        assert_eq!(
            tree.local_rect(2).unwrap(),
            Rect::new(10.0, 34.0, 30.0, 10.0)
        );
    }

    #[test]
    fn text_leaves_are_measured() {
        let mut tree = LayoutTree::new();
        let row = LayoutStyle {
            align: Align::Start,
            ..LayoutStyle::default()
        };
        tree.insert(0, &row).unwrap();
        tree.insert(1, &LayoutStyle::default()).unwrap();
        tree.set_text(1, Some(text_leaf("hello"))).unwrap();
        tree.set_children(0, &[1]).unwrap();
        tree.compute(
            0,
            Size::new(200.0, 100.0),
            &mut ApproximateTextMeasurer::default(),
        )
        .unwrap();

        assert_eq!(tree.local_rect(1).unwrap().size(), Size::new(25.0, 14.0));
    }

    #[test]
    fn changing_text_invalidates_the_cached_size() {
        let mut tree = LayoutTree::new();
        let row = LayoutStyle {
            align: Align::Start,
            ..LayoutStyle::default()
        };
        tree.insert(0, &row).unwrap();
        tree.insert(1, &LayoutStyle::default()).unwrap();
        tree.set_text(1, Some(text_leaf("hi"))).unwrap();
        tree.set_children(0, &[1]).unwrap();
        let mut measurer = ApproximateTextMeasurer::default();
        tree.compute(0, Size::new(200.0, 100.0), &mut measurer)
            .unwrap();
        tree.set_text(1, Some(text_leaf("hello world"))).unwrap();
        tree.compute(0, Size::new(200.0, 100.0), &mut measurer)
            .unwrap();

        assert_eq!(tree.local_rect(1).unwrap().width, 55.0);
    }

    #[test]
    fn grid_splits_columns_evenly() {
        let mut tree = LayoutTree::new();
        let grid = LayoutStyle {
            display: Display::Grid,
            grid_columns: 2,
            width: Extent::Px(100.0),
            ..LayoutStyle::default()
        };
        tree.insert(0, &grid).unwrap();
        tree.insert(
            1,
            &LayoutStyle {
                height: Extent::Px(10.0),
                ..LayoutStyle::default()
            },
        )
        .unwrap();
        tree.insert(
            2,
            &LayoutStyle {
                height: Extent::Px(10.0),
                ..LayoutStyle::default()
            },
        )
        .unwrap();
        tree.set_children(0, &[1, 2]).unwrap();
        tree.compute(
            0,
            Size::new(100.0, 100.0),
            &mut ApproximateTextMeasurer::default(),
        )
        .unwrap();

        assert_eq!(
            tree.local_rect(2).unwrap(),
            Rect::new(50.0, 0.0, 50.0, 10.0)
        );
    }

    #[test]
    fn unknown_keys_are_reported() {
        let mut tree: LayoutTree<u32> = LayoutTree::new();
        assert_eq!(
            tree.set_style(7, &LayoutStyle::default()),
            Err(LayoutError::UnknownNode("7".into()))
        );
    }
}
