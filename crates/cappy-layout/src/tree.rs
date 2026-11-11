//! Layout tree backed by taffy, keyed by the caller's node ids.

use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;

use cappy_style::{Align, Display, Edges, Extent, FlexDirection, Justify, LayoutStyle, Overflow};
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

/// How much space one axis of a layout root is given.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LayoutConstraint {
    /// A known number of logical pixels. Percentages resolve against this.
    Definite(f32),
    /// The content's max size. An automatic width or height shrinks to the children.
    MaxContent,
}

impl LayoutConstraint {
    fn available(self) -> AvailableSpace {
        match self {
            Self::Definite(value) => AvailableSpace::Definite(value),
            Self::MaxContent => AvailableSpace::MaxContent,
        }
    }
}

fn engine_error(error: taffy::TaffyError) -> LayoutError {
    LayoutError::Engine(format!("{error:?}"))
}

/// Mirrors the caller's tree in taffy and computes rectangles relative to each parent.
pub struct LayoutTree<K> {
    taffy: TaffyTree<TextLeaf>,
    nodes: HashMap<K, taffy::NodeId>,
    /// Taffy keeps a slot for every node ever inserted. `remove` frees the value but not the slot.
    retained: usize,
}

impl<K> Default for LayoutTree<K> {
    fn default() -> Self {
        Self {
            taffy: TaffyTree::new(),
            nodes: HashMap::new(),
            retained: 0,
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
        self.retained = self.retained.max(self.nodes.len());
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

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn node_capacity(&self) -> usize {
        self.nodes.capacity()
    }

    /// Taffy slots still reserved. This stays at the high-water mark until [`LayoutTree::compact`].
    pub fn retained_slots(&self) -> usize {
        self.retained
    }

    /// Rebuilds taffy from the live nodes once enough removed nodes have left empty slots.
    pub fn compact(&mut self) {
        const FREE_SLOTS_BEFORE_COMPACT: usize = 32;
        let live = self.nodes.len();
        if self.retained <= live || self.retained - live < FREE_SLOTS_BEFORE_COMPACT {
            return;
        }
        let reverse: HashMap<taffy::NodeId, K> =
            self.nodes.iter().map(|(key, id)| (*id, *key)).collect();
        let saved: Vec<(K, taffy::Style, Option<TextLeaf>, Vec<K>)> = self
            .nodes
            .iter()
            .map(|(key, id)| {
                let style = self.taffy.style(*id).expect("live layout node").clone();
                let text = self.taffy.get_node_context(*id).cloned();
                let children = self
                    .taffy
                    .children(*id)
                    .expect("live layout node")
                    .into_iter()
                    .map(|child| reverse[&child])
                    .collect();
                (*key, style, text, children)
            })
            .collect();
        self.taffy = TaffyTree::new();
        self.nodes.clear();
        for (key, style, text, _) in &saved {
            let node = self
                .taffy
                .new_leaf(style.clone())
                .expect("a fresh layout tree accepts a node");
            if let Some(text) = text {
                self.taffy
                    .set_node_context(node, Some(text.clone()))
                    .expect("a fresh layout node accepts its text");
            }
            self.nodes.insert(*key, node);
        }
        for (key, _, _, children) in &saved {
            let child_ids: Vec<taffy::NodeId> =
                children.iter().map(|child| self.nodes[child]).collect();
            self.taffy
                .set_children(self.nodes[key], &child_ids)
                .expect("rebuilt children are nodes in this tree");
        }
        self.nodes.shrink_to_fit();
        self.retained = self.nodes.len();
    }

    /// Drops spare capacity left behind after nodes were removed.
    pub fn shrink_to_fit(&mut self) {
        self.nodes.shrink_to_fit();
    }

    /// Drops `key` after it has been unlinked from its parent. Children should already be gone.
    pub fn remove(&mut self, key: K) -> Result<(), LayoutError> {
        let node = self
            .nodes
            .remove(&key)
            .ok_or_else(|| LayoutError::UnknownNode(format!("{key:?}")))?;
        self.taffy.remove(node).map_err(engine_error)?;
        Ok(())
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
        self.compute_axes(
            root,
            LayoutConstraint::Definite(available.width),
            LayoutConstraint::Definite(available.height),
            measurer,
        )
    }

    /// Lays out `root` as its own tree. [`LayoutConstraint::MaxContent`] shrinks that axis to the
    /// content. [`LayoutConstraint::Definite`] is the size a percentage resolves against.
    pub fn compute_axes(
        &mut self,
        root: K,
        width: LayoutConstraint,
        height: LayoutConstraint,
        measurer: &mut dyn TextMeasurer,
    ) -> Result<(), LayoutError> {
        let root_node = self.node(root)?;
        let space = taffy::Size {
            width: width.available(),
            height: height.available(),
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

    /// Scroll extent of `key` from the last [`LayoutTree::compute`].
    ///
    /// `content.right` and `content.bottom` are the reachable content edges measured from the
    /// padding-box origin. [`ScrollableOverflow::max_offset`] is how far the content can move.
    pub fn scrollable_overflow(&self, key: K) -> Result<ScrollableOverflow, LayoutError> {
        let layout = self.taffy.layout(self.node(key)?).map_err(engine_error)?;
        let content = layout.scrollable_overflow_rect;
        Ok(ScrollableOverflow {
            size: Size::new(layout.size.width, layout.size.height),
            border: Edges {
                top: layout.border.top,
                right: layout.border.right,
                bottom: layout.border.bottom,
                left: layout.border.left,
            },
            content: Edges {
                top: content.top,
                right: content.right,
                bottom: content.bottom,
                left: content.left,
            },
        })
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
    let edges = |edges: &Edges| taffy::Rect {
        left: LengthPercentage::length(edges.left),
        right: LengthPercentage::length(edges.right),
        top: LengthPercentage::length(edges.top),
        bottom: LengthPercentage::length(edges.bottom),
    };
    let margin = |edges: &Edges| taffy::Rect {
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
        overflow: taffy::Point {
            x: taffy_overflow(style.overflow_x),
            y: taffy_overflow(style.overflow_y),
        },
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

fn taffy_overflow(overflow: Overflow) -> taffy::Overflow {
    match overflow {
        Overflow::Visible => taffy::Overflow::Visible,
        // `Auto` reserves no scrollbar gutter. The runtime draws an overlay thumb.
        Overflow::Hidden | Overflow::Auto => taffy::Overflow::Hidden,
    }
}

/// Content reach of one node after layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollableOverflow {
    pub size: Size,
    pub border: Edges,
    /// Edges of the scrollable overflow rectangle. `right` and `bottom` are the reachable extent.
    pub content: Edges,
}

impl ScrollableOverflow {
    /// How far content can move before the far edge meets the padding box.
    pub fn max_offset(self) -> crate::Point {
        let port_width = (self.size.width - self.border.horizontal()).max(0.0);
        let port_height = (self.size.height - self.border.vertical()).max(0.0);
        crate::Point::new(
            (self.content.right - port_width).max(0.0),
            (self.content.bottom - port_height).max(0.0),
        )
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
    fn a_fixed_column_clips_a_taller_child_and_reports_the_leftover() {
        let mut tree = LayoutTree::new();
        let mut parent = fixed(100.0, 40.0);
        parent.direction = FlexDirection::Column;
        parent.overflow_y = Overflow::Auto;
        let mut child = fixed(80.0, 90.0);
        child.flex_shrink = 0.0;
        tree.insert(0, &parent).unwrap();
        tree.insert(1, &child).unwrap();
        tree.set_children(0, &[1]).unwrap();
        tree.compute(
            0,
            Size::new(200.0, 200.0),
            &mut ApproximateTextMeasurer::default(),
        )
        .unwrap();

        assert_eq!(tree.local_rect(0).unwrap().height, 40.0);
        assert_eq!(tree.local_rect(1).unwrap().height, 90.0);
        assert_eq!(tree.scrollable_overflow(0).unwrap().max_offset().y, 50.0);
    }

    #[test]
    fn a_scroll_column_shrinks_inside_a_parent_that_can_shrink() {
        let mut tree = LayoutTree::new();
        let mut root = column(0.0, 0.0);
        root.width = Extent::Px(200.0);
        root.height = Extent::Px(100.0);
        let mut card = column(0.0, 0.0);
        card.flex_grow = 1.0;
        card.min_height = Extent::Px(0.0);
        let mut list = column(0.0, 0.0);
        list.flex_grow = 1.0;
        list.overflow_y = Overflow::Auto;
        let mut child = fixed(80.0, 180.0);
        child.flex_shrink = 0.0;

        tree.insert(0, &root).unwrap();
        tree.insert(1, &card).unwrap();
        tree.insert(2, &list).unwrap();
        tree.insert(3, &child).unwrap();
        tree.set_children(0, &[1]).unwrap();
        tree.set_children(1, &[2]).unwrap();
        tree.set_children(2, &[3]).unwrap();
        tree.compute(
            0,
            Size::new(200.0, 200.0),
            &mut ApproximateTextMeasurer::default(),
        )
        .unwrap();

        assert_eq!(tree.local_rect(0).unwrap().height, 100.0);
        assert!(tree.local_rect(2).unwrap().height < 180.0);
        assert!(tree.scrollable_overflow(2).unwrap().max_offset().y > 0.0);
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
    fn remove_drops_the_node() {
        let mut tree = LayoutTree::new();
        tree.insert(0, &fixed(10.0, 10.0)).unwrap();
        tree.insert(1, &fixed(4.0, 4.0)).unwrap();
        tree.set_children(0, &[1]).unwrap();
        tree.set_children(0, &[]).unwrap();
        tree.remove(1).unwrap();
        assert!(matches!(
            tree.local_rect(1),
            Err(LayoutError::UnknownNode(_))
        ));
        tree.remove(1).unwrap_err();
    }

    #[test]
    fn compact_releases_slots_after_a_large_drop() {
        let mut tree = LayoutTree::new();
        tree.insert(0, &fixed(10.0, 10.0)).unwrap();
        let children: Vec<u32> = (1..80).collect();
        for id in &children {
            tree.insert(*id, &fixed(4.0, 4.0)).unwrap();
        }
        tree.set_children(0, &children).unwrap();
        assert!(tree.retained_slots() >= 80);
        tree.set_children(0, &[]).unwrap();
        for id in &children {
            tree.remove(*id).unwrap();
        }
        assert_eq!(tree.node_count(), 1);
        assert!(tree.retained_slots() >= 80);
        tree.compact();
        assert_eq!(tree.retained_slots(), 1);
        tree.compute(
            0,
            Size::new(100.0, 100.0),
            &mut ApproximateTextMeasurer::default(),
        )
        .unwrap();
        assert!(tree.local_rect(0).unwrap().width > 0.0);
    }

    #[test]
    fn max_content_shrinks_and_a_percentage_uses_the_definite_axis() {
        let mut tree = LayoutTree::new();
        let mut parent = column(0.0, 0.0);
        parent.flex_shrink = 0.0;
        let mut child = fixed(30.0, 20.0);
        child.flex_shrink = 0.0;
        tree.insert(0, &parent).unwrap();
        tree.insert(1, &child).unwrap();
        tree.set_children(0, &[1]).unwrap();
        tree.compute_axes(
            0,
            LayoutConstraint::MaxContent,
            LayoutConstraint::MaxContent,
            &mut ApproximateTextMeasurer::default(),
        )
        .unwrap();
        let shrunk = tree.local_rect(0).unwrap();
        assert!((shrunk.width - 30.0).abs() < 0.5, "width {}", shrunk.width);
        assert!(
            (shrunk.height - 20.0).abs() < 0.5,
            "height {}",
            shrunk.height
        );

        let mut wide = column(0.0, 0.0);
        wide.width = Extent::Percent(50.0);
        wide.height = Extent::Px(10.0);
        tree.set_style(0, &wide).unwrap();
        tree.compute_axes(
            0,
            LayoutConstraint::Definite(800.0),
            LayoutConstraint::Definite(600.0),
            &mut ApproximateTextMeasurer::default(),
        )
        .unwrap();
        let percent = tree.local_rect(0).unwrap();
        assert!(
            (percent.width - 400.0).abs() < 0.5,
            "percent width {}",
            percent.width
        );
        assert!((percent.height - 10.0).abs() < 0.5);
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
