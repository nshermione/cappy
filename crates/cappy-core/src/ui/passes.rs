//! The style, layout and paint passes run by [`Ui::update`].

use std::sync::Arc;

use cappy_layout::{LayoutConstraint, Point, Rect, Size, TextLeaf};
use cappy_style::{
    Background, BoxShadow, Color, Display, Extent, LayoutStyle, TextStyle, VisualStyle,
};

use super::scroll::{self, axis_clip, horizontal_thumb, nest_clip, scrollport, vertical_thumb};
use super::{NodeId, Ui};
use crate::paint::{
    ImagePrimitive, Primitive, QuadPrimitive, ShadowPrimitive, Spin, TextPrimitive, paint_bounds,
    rect_center, scale_about, shadow_shape, spun_aabb,
};

const LAYOUT_OUT_OF_SYNC: &str = "layout tree is out of sync with the node tree";

impl Ui {
    pub(super) fn mark_style_dirty(&mut self, node: NodeId) {
        self.nodes[node.index()].style_dirty = true;
        self.style_dirty = true;
    }

    pub(super) fn mark_paint_dirty(&mut self, node: NodeId) {
        self.nodes[node.index()].paint_dirty = true;
        self.paint_dirty = true;
    }

    pub(super) fn set_text_content(&mut self, node: NodeId, text: String) {
        let target = &mut self.nodes[node.index()];
        if target.text() == Some(text.as_str()) {
            return;
        }
        target.content = super::NodeContent::Text(Arc::from(text));
        self.sync_text_leaf(node);
        self.mark_paint_dirty(node);
    }

    /// Pushes a text node's content and font metrics to the layout tree.
    pub(super) fn sync_text_leaf(&mut self, node: NodeId) {
        let target = &self.nodes[node.index()];
        let Some(text) = target.text() else {
            return;
        };
        let leaf = text_leaf(text, &target.style.text);
        self.layout
            .set_text(node, Some(leaf))
            .expect(LAYOUT_OUT_OF_SYNC);
        self.layout_dirty = true;
    }

    /// Gives an input one line of height from its placeholder, not from the value, so a long
    /// value scrolls inside the box instead of growing it.
    pub(super) fn sync_input_leaf(&mut self, node: NodeId) {
        let Some(sample) = self.input_sample(node) else {
            return;
        };
        let style = &self.nodes[node.index()].style.text;
        let leaf = text_leaf(&sample, style);
        self.layout
            .set_text(node, Some(leaf))
            .expect(LAYOUT_OUT_OF_SYNC);
        self.layout_dirty = true;
    }

    fn input_sample(&self, node: NodeId) -> Option<String> {
        let super::NodeContent::Input(field) = &self.nodes[node.index()].content else {
            return None;
        };
        if field.placeholder.is_empty() {
            Some(" ".to_owned())
        } else {
            Some(field.placeholder.clone())
        }
    }

    /// The root fills the viewport unless its style gives it an explicit size.
    pub(super) fn layout_style_of(&self, node: NodeId) -> LayoutStyle {
        let mut style = self.nodes[node.index()].style.layout.clone();
        if self.nodes[node.index()].suppressed {
            style.display = Display::None;
        }
        if node == self.root {
            for extent in [&mut style.width, &mut style.height] {
                if *extent == Extent::Auto {
                    *extent = Extent::Percent(100.0);
                }
            }
        }
        style
    }

    /// Re-resolves dirty styles top-down and records what each change invalidates.
    pub(super) fn resolve_styles(&mut self) {
        if !std::mem::take(&mut self.style_dirty) {
            return;
        }
        let mut stack: Vec<(NodeId, Option<TextStyle>, bool)> = vec![(self.root, None, false)];
        while let Some((id, parent_text, parent_text_changed)) = stack.pop() {
            let mut text_changed = false;
            if self.nodes[id.index()].style_dirty || parent_text_changed {
                text_changed = self.restyle(id, parent_text.as_ref());
            }
            let node = &self.nodes[id.index()];
            for child in node.children.iter().rev() {
                stack.push((*child, Some(node.style.text.clone()), text_changed));
            }
        }
    }

    /// Resolves one node's style. Returns whether its inherited text style changed.
    fn restyle(&mut self, id: NodeId, parent_text: Option<&TextStyle>) -> bool {
        let was_none = self.nodes[id.index()].style.layout.display == Display::None;
        let node = &self.nodes[id.index()];
        let resolved = self.theme.get().resolve(
            parent_text,
            &node.all_classes(),
            &node.effective_inline_style(),
            node.interaction,
            node.stylesheet.as_deref(),
        );
        let change = resolved.diff(&node.style);
        let is_text = node.is_text();
        let is_input = matches!(node.content, super::NodeContent::Input(_));
        let node = &mut self.nodes[id.index()];
        node.style = resolved;
        node.style_dirty = false;
        if change.layout {
            let layout_style = self.layout_style_of(id);
            self.layout
                .set_style(id, &layout_style)
                .expect(LAYOUT_OUT_OF_SYNC);
            self.layout_dirty = true;
            if is_text {
                self.sync_text_leaf(id);
            }
            if is_input {
                self.sync_input_leaf(id);
            }
        }
        if change.paint {
            self.mark_paint_dirty(id);
        }
        let now_none = self.nodes[id.index()].style.layout.display == Display::None;
        if was_none != now_none {
            self.scope_display_changed(id, now_none);
        }
        change.inherited
    }

    /// Recomputes layout when needed and damages every node whose rectangle moved.
    ///
    /// A scroll change sets [`Ui::positions_dirty`] and skips taffy: child positions are the
    /// stored layout rects shifted by the current offset.
    pub(super) fn run_layout(&mut self) {
        if std::mem::take(&mut self.layout_dirty) {
            let viewport = self.logical_viewport();
            self.layout
                .compute(self.root, viewport, self.measurer.as_mut())
                .expect(LAYOUT_OUT_OF_SYNC);
            for id in self.overlay_layers() {
                let (width, height) = self.overlay_constraint(id, viewport);
                self.layout
                    .compute_axes(id, width, height, self.measurer.as_mut())
                    .expect(LAYOUT_OUT_OF_SYNC);
            }
            self.clamp_scroll_targets();
            self.positions_dirty = true;
        }
        if !std::mem::take(&mut self.positions_dirty) {
            return;
        }
        self.place_nodes();
        if self.pointer.is_some() {
            self.refresh_hover();
        }
    }

    /// Moves each scrolling offset toward its target. A change only repositions; taffy stays.
    pub(super) fn step_scroll(&mut self, dt: f32) {
        let mut moved = false;
        for index in 0..self.nodes.len() {
            let node = &self.nodes[index];
            if !node.alive {
                continue;
            }
            let next = scroll::ease_point(node.scroll, node.scroll_target, dt);
            if next == node.scroll {
                continue;
            }
            let id = NodeId::new(index as u32, node.generation);
            self.nodes[index].scroll = next;
            self.damage_scrollport(id);
            moved = true;
        }
        if moved {
            self.positions_dirty = true;
            self.display_list_stale = true;
        }
    }

    fn clamp_scroll_targets(&mut self) {
        for index in 0..self.nodes.len() {
            if !self.nodes[index].alive {
                continue;
            }
            let id = NodeId::new(index as u32, self.nodes[index].generation);
            let max = self.scroll_max(id);
            let scrolls_x = self.nodes[index].style.layout.overflow_x.scrolls();
            let scrolls_y = self.nodes[index].style.layout.overflow_y.scrolls();
            let node = &mut self.nodes[index];
            let mut target = node.scroll_target;
            let mut scroll = node.scroll;
            if scrolls_x {
                target.x = target.x.clamp(0.0, max.x);
            } else {
                target.x = 0.0;
                scroll.x = 0.0;
            }
            if scrolls_y {
                target.y = target.y.clamp(0.0, max.y);
            } else {
                target.y = 0.0;
                scroll.y = 0.0;
            }
            if scroll != node.scroll || target != node.scroll_target {
                node.scroll = scroll;
                node.scroll_target = target;
            }
        }
    }

    /// Puts `offset` on one axis and skips easing, so a scrollbar drag tracks the pointer.
    pub(super) fn jump_scroll(&mut self, id: NodeId, vertical: bool, offset: f32) {
        if !self.is_current(id) {
            return;
        }
        let max = self.scroll_max(id);
        let node = &mut self.nodes[id.index()];
        let mut next = node.scroll;
        if vertical {
            next.y = offset.clamp(0.0, max.y.max(0.0));
        } else {
            next.x = offset.clamp(0.0, max.x.max(0.0));
        }
        if next == node.scroll && next == node.scroll_target {
            return;
        }
        node.scroll = next;
        node.scroll_target = next;
        self.damage_scrollport(id);
        self.positions_dirty = true;
        self.display_list_stale = true;
    }

    pub(super) fn scroll_max(&self, id: NodeId) -> Point {
        let overflow = self
            .layout
            .scrollable_overflow(id)
            .expect(LAYOUT_OUT_OF_SYNC);
        let mut max = overflow.max_offset();
        let layout = &self.nodes[id.index()].style.layout;
        if !layout.overflow_x.scrolls() {
            max.x = 0.0;
        }
        if !layout.overflow_y.scrolls() {
            max.y = 0.0;
        }
        max
    }

    /// Copies this node's rect and scroll into the geometry board when it has a key.
    fn publish_geometry(&self, id: NodeId, rect: Rect) {
        let Some(key) = self.nodes[id.index()].key.as_deref() else {
            return;
        };
        let scroll = self.nodes[id.index()].scroll;
        self.board.publish(key, id, rect, scroll);
    }

    /// Stores the viewport top-left of an overlay. A change is placed on the next layout pass.
    pub(super) fn apply_place(&mut self, id: NodeId, point: Point) {
        if !self.is_current(id) || self.nodes[id.index()].anchor == point {
            return;
        }
        self.nodes[id.index()].anchor = point;
        if self.nodes[id.index()].overlay {
            self.positions_dirty = true;
            self.display_list_stale = true;
        }
    }

    /// Automatic axes shrink to the content. A length or percentage gets the viewport, so the
    /// percentage has a size to resolve against.
    fn overlay_constraint(
        &self,
        id: NodeId,
        viewport: Size,
    ) -> (LayoutConstraint, LayoutConstraint) {
        let layout = &self.nodes[id.index()].style.layout;
        (
            constraint_for(layout.width, viewport.width),
            constraint_for(layout.height, viewport.height),
        )
    }

    /// Overlay nodes in paint order. The list is rebuilt after the tree changes.
    pub(super) fn overlay_layers(&self) -> Vec<NodeId> {
        if self.overlay_order_dirty.get() {
            let order = self.collect_overlay_layers();
            *self.overlay_order.borrow_mut() = order;
            self.overlay_order_dirty.set(false);
        }
        self.overlay_order.borrow().clone()
    }

    fn collect_overlay_layers(&self) -> Vec<NodeId> {
        let mut order = Vec::new();
        self.collect_layers_under(self.root, &mut order);
        order
    }

    /// In-flow descendants first, then each overlay child and the overlays nested inside it.
    fn collect_layers_under(&self, id: NodeId, order: &mut Vec<NodeId>) {
        let children = self.nodes[id.index()].children.clone();
        for child in &children {
            if !self.nodes[child.index()].overlay {
                self.collect_layers_under(*child, order);
            }
        }
        for child in children {
            if self.nodes[child.index()].overlay {
                order.push(child);
                self.collect_layers_under(child, order);
            }
        }
    }

    fn place_nodes(&mut self) {
        let mut stack = vec![Placed {
            id: self.root,
            origin: self.layer_origin(self.root),
            clip: None,
        }];
        while let Some(Placed { id, origin, clip }) = stack.pop() {
            let layout_rect = self
                .layout
                .local_rect(id)
                .expect(LAYOUT_OUT_OF_SYNC)
                .translate(origin);
            let translation = self.nodes[id.index()].translation;
            let rect = layout_rect.translate(translation);
            let (previous, shadow, moved) = {
                let node = &mut self.nodes[id.index()];
                let moved = node.rect != rect;
                let previous = node.painted;
                let shadow = node.style.visual.shadow;
                node.clip = clip;
                if moved {
                    node.rect = rect;
                }
                (previous, shadow, moved)
            };
            if moved {
                self.damage_clipped(previous, clip);
                self.damage_clipped(paint_bounds(rect, shadow), clip);
                self.display_list_stale = true;
            }
            self.publish_geometry(id, rect);
            let node = &self.nodes[id.index()];
            let child_origin = Point::new(rect.x - node.scroll.x, rect.y - node.scroll.y);
            let child_clip = if node.style.layout.clips() {
                nest_clip(
                    clip,
                    axis_clip(
                        scrollport(rect, node.style.visual.border_width),
                        node.style.layout.overflow_x.clips(),
                        node.style.layout.overflow_y.clips(),
                    ),
                )
            } else {
                clip
            };
            let children = node.children.clone();
            for child in children.into_iter().rev() {
                if self.nodes[child.index()].overlay {
                    stack.push(Placed {
                        id: child,
                        origin: self.nodes[child.index()].anchor,
                        clip: None,
                    });
                } else {
                    stack.push(Placed {
                        id: child,
                        origin: child_origin,
                        clip: child_clip,
                    });
                }
            }
        }
    }

    /// An overlay's top-left is its anchor. Every other node starts at the origin passed in.
    fn layer_origin(&self, id: NodeId) -> Point {
        let node = &self.nodes[id.index()];
        if node.overlay {
            node.anchor
        } else {
            Point::default()
        }
    }

    fn damage_scrollport(&mut self, id: NodeId) {
        let node = &self.nodes[id.index()];
        self.damage
            .add(scrollport(node.rect, node.style.visual.border_width));
    }

    pub(super) fn damage_clipped(&mut self, rect: Rect, clip: Option<Rect>) {
        let Some(rect) = clip_rect(rect, clip) else {
            return;
        };
        self.damage.add(rect);
    }

    /// Damages every node whose appearance changed.
    pub(super) fn collect_paint_damage(&mut self) {
        if !std::mem::take(&mut self.paint_dirty) {
            return;
        }
        for index in 0..self.nodes.len() {
            if !std::mem::take(&mut self.nodes[index].paint_dirty) {
                continue;
            }
            let clip = self.nodes[index].clip;
            let painted = self.nodes[index].painted;
            let bounds = paint_bounds(
                self.nodes[index].rect,
                self.nodes[index].style.visual.shadow,
            );
            self.damage_clipped(painted, clip);
            self.damage_clipped(bounds, clip);
            self.display_list_stale = true;
        }
    }

    pub(super) fn damage_visual_tree(&mut self, id: NodeId) {
        let mut stack = vec![id];
        while let Some(current) = stack.pop() {
            if !self.is_current(current) {
                continue;
            }
            let ink = self.visual_ink(current);
            let (painted, clip, children) = {
                let node = &mut self.nodes[current.index()];
                node.paint_dirty = true;
                (node.painted, node.clip, node.children.clone())
            };
            self.damage_clipped(painted, clip);
            self.damage_clipped(ink, clip);
            stack.extend(children);
        }
        self.paint_dirty = true;
        self.display_list_stale = true;
    }

    fn visual_ink(&self, id: NodeId) -> Rect {
        let node = &self.nodes[id.index()];
        let space = self.paint_space_of(id);
        let shadow = scale_shadow(node.style.visual.shadow, space.scale);
        let (rect, spin) = map_paint_rect(node.rect, space);
        spun_aabb(paint_bounds(rect, shadow), spin)
    }

    fn paint_space_of(&self, id: NodeId) -> PaintSpace {
        let mut chain = vec![id];
        let mut current = self.nodes[id.index()].parent;
        while let Some(parent) = current {
            chain.push(parent);
            current = self.nodes[parent.index()].parent;
        }
        let mut space = PaintSpace::IDENTITY;
        for node_id in chain.iter().rev() {
            let node = &self.nodes[node_id.index()];
            space = space.inherit(node.rect, node.scale, node.rotation);
        }
        space
    }

    pub(super) fn rebuild_display_list(&mut self) {
        if !std::mem::take(&mut self.display_list_stale) {
            return;
        }
        let mut primitives = Vec::new();
        let mut painted = vec![Rect::ZERO; self.nodes.len()];
        let mut stack = vec![Painted::Enter {
            id: self.root,
            clip: None,
            opacity: 1.0,
            space: PaintSpace::IDENTITY,
        }];
        while let Some(step) = stack.pop() {
            match step {
                Painted::Thumb { id, clip, opacity } => {
                    self.push_thumbs(&mut primitives, id, clip, opacity);
                    continue;
                }
                Painted::Layers {
                    ids,
                    opacity,
                    space,
                } => {
                    stack.extend(ids.into_iter().rev().map(|id| Painted::Enter {
                        id,
                        clip: None,
                        opacity,
                        space,
                    }));
                    continue;
                }
                Painted::Enter {
                    id,
                    clip,
                    opacity,
                    space,
                } => {
                    let node = &self.nodes[id.index()];
                    if !node.alive || node.suppressed || node.style.layout.display == Display::None
                    {
                        continue;
                    }
                    let opacity = opacity * node.opacity;
                    let space = space.inherit(node.rect, node.scale, node.rotation);
                    let mut visual = node.style.visual.clone();
                    if let Some(color) = node.background {
                        visual.background = Background::Solid(color);
                    }
                    visual.border_width *= space.scale;
                    visual.corner_radius *= space.scale;
                    visual.shadow = scale_shadow(visual.shadow, space.scale);
                    let (rect, spin) = map_paint_rect(node.rect, space);
                    painted[id.index()] = spun_aabb(paint_bounds(rect, visual.shadow), spin);
                    push_box(&mut primitives, rect, &visual, clip, opacity, spin);
                    match &node.content {
                        super::NodeContent::Text(text) => {
                            let content = text_content_rect(node, space);
                            let text_style = &node.style.text;
                            primitives.push(Primitive::Text(TextPrimitive {
                                rect: content,
                                text: text.clone(),
                                color: fade_color(text_style.color, opacity),
                                font_size: text_style.font_size * space.scale,
                                line_height: text_style.line_height_px() * space.scale,
                                weight: text_style.weight.0,
                                clip,
                                caret: node.caret,
                                selection: node.selection,
                                wrap: true,
                                scroll_x: 0.0,
                            }));
                        }
                        super::NodeContent::Input(field) => {
                            let content = text_content_rect(node, space);
                            let text_style = &node.style.text;
                            let focused = self.focused == Some(id);
                            let placeholder = field.edit.text().is_empty()
                                && field.edit.display().selection.is_none()
                                && field.edit.display().text == field.edit.text();
                            let display = field.edit.display();
                            let (shown, color, caret, selection) = if placeholder {
                                (
                                    field.placeholder.clone(),
                                    Color {
                                        a: text_style.color.a * 0.45,
                                        ..text_style.color
                                    },
                                    if focused && self.caret_on {
                                        Some(0)
                                    } else {
                                        None
                                    },
                                    None,
                                )
                            } else {
                                (
                                    display.text.clone(),
                                    text_style.color,
                                    if focused && self.caret_on {
                                        Some(display.caret)
                                    } else {
                                        None
                                    },
                                    display.selection,
                                )
                            };
                            primitives.push(Primitive::Text(TextPrimitive {
                                rect: content,
                                text: Arc::from(shown),
                                color: fade_color(color, opacity),
                                font_size: text_style.font_size * space.scale,
                                line_height: text_style.line_height_px() * space.scale,
                                weight: text_style.weight.0,
                                clip,
                                caret,
                                selection,
                                wrap: false,
                                scroll_x: field.scroll_x * space.scale,
                            }));
                        }
                        super::NodeContent::Image { pixels, fit, .. } => {
                            primitives.push(Primitive::Image(ImagePrimitive {
                                rect,
                                pixels: pixels.clone(),
                                fit: *fit,
                                corner_radius: visual.corner_radius,
                                clip,
                                opacity,
                            }));
                        }
                        super::NodeContent::Container => {}
                    }
                    let child_clip = if node.style.layout.clips() {
                        nest_clip(
                            clip,
                            axis_clip(
                                scrollport(node.rect, node.style.visual.border_width),
                                node.style.layout.overflow_x.clips(),
                                node.style.layout.overflow_y.clips(),
                            ),
                        )
                    } else {
                        clip
                    };
                    let children = node.children.clone();
                    let mut in_flow = Vec::new();
                    let mut overlays = Vec::new();
                    for child in children {
                        if self.nodes[child.index()].overlay {
                            overlays.push(child);
                        } else {
                            in_flow.push(child);
                        }
                    }
                    if !overlays.is_empty() {
                        stack.push(Painted::Layers {
                            ids: overlays,
                            opacity,
                            space,
                        });
                    }
                    stack.push(Painted::Thumb {
                        id,
                        clip: child_clip,
                        opacity,
                    });
                    stack.extend(in_flow.into_iter().rev().map(|child| Painted::Enter {
                        id: child,
                        clip: child_clip,
                        opacity,
                        space,
                    }));
                }
            }
        }
        for (node, ink) in self.nodes.iter_mut().zip(painted) {
            node.painted = ink;
        }
        self.display_list = Arc::new(primitives);
    }

    fn push_thumbs(
        &self,
        primitives: &mut Vec<Primitive>,
        id: NodeId,
        clip: Option<Rect>,
        opacity: f32,
    ) {
        let node = &self.nodes[id.index()];
        let layout = &node.style.layout;
        if !layout.overflow_x.scrolls() && !layout.overflow_y.scrolls() {
            return;
        }
        let port = scrollport(node.rect, node.style.visual.border_width);
        let max = self.scroll_max(id);
        if layout.overflow_y.scrolls()
            && let Some(rect) = vertical_thumb(port, node.scroll.y, max.y)
        {
            push_thumb(primitives, rect, clip, opacity);
        }
        if layout.overflow_x.scrolls()
            && let Some(rect) = horizontal_thumb(port, node.scroll.x, max.x)
        {
            push_thumb(primitives, rect, clip, opacity);
        }
    }
}

struct Placed {
    id: NodeId,
    origin: Point,
    clip: Option<Rect>,
}

#[derive(Clone, Copy)]
struct PaintSpace {
    pivot: Point,
    scale: f32,
    rotation: f32,
}

impl PaintSpace {
    const IDENTITY: Self = Self {
        pivot: Point::new(0.0, 0.0),
        scale: 1.0,
        rotation: 0.0,
    };

    fn moves(self) -> bool {
        (self.scale - 1.0).abs() > 1.0e-4 || self.rotation.abs() > 1.0e-4
    }

    /// The first transformed node owns the pivot. Descendants multiply into that same pivot.
    fn inherit(self, rect: Rect, scale: f32, rotation: f32) -> Self {
        let child_moves = (scale - 1.0).abs() > 1.0e-4 || rotation.abs() > 1.0e-4;
        if !child_moves {
            return self;
        }
        if !self.moves() {
            return Self {
                pivot: rect_center(rect),
                scale,
                rotation,
            };
        }
        Self {
            pivot: self.pivot,
            scale: self.scale * scale,
            rotation: self.rotation + rotation,
        }
    }
}

fn text_content_rect(node: &super::node::Node, space: PaintSpace) -> Rect {
    let padding = node.style.layout.padding;
    scale_about(
        Rect::new(
            node.rect.x + padding.left,
            node.rect.y + padding.top,
            (node.rect.width - padding.horizontal()).max(0.0),
            (node.rect.height - padding.vertical()).max(0.0),
        ),
        space.pivot,
        space.scale,
    )
}

fn map_paint_rect(rect: Rect, space: PaintSpace) -> (Rect, Spin) {
    let scaled = scale_about(rect, space.pivot, space.scale);
    let spin = if space.rotation.abs() <= 1.0e-4 {
        Spin::NONE
    } else {
        Spin {
            pivot: space.pivot,
            radians: space.rotation,
        }
    };
    (scaled, spin)
}

fn scale_shadow(shadow: BoxShadow, scale: f32) -> BoxShadow {
    BoxShadow {
        offset_x: shadow.offset_x * scale,
        offset_y: shadow.offset_y * scale,
        blur: shadow.blur * scale,
        spread: shadow.spread * scale,
        color: shadow.color,
    }
}

enum Painted {
    Enter {
        id: NodeId,
        clip: Option<Rect>,
        opacity: f32,
        space: PaintSpace,
    },
    /// Drawn after the node's descendants so the thumb stays above the content.
    Thumb {
        id: NodeId,
        clip: Option<Rect>,
        opacity: f32,
    },
    /// Overlay children, painted after this node's in-flow subtree with the clip cleared.
    Layers {
        ids: Vec<NodeId>,
        opacity: f32,
        space: PaintSpace,
    },
}

fn constraint_for(extent: Extent, available: f32) -> LayoutConstraint {
    match extent {
        Extent::Auto => LayoutConstraint::MaxContent,
        Extent::Px(_) | Extent::Percent(_) => LayoutConstraint::Definite(available),
    }
}

fn clip_rect(rect: Rect, clip: Option<Rect>) -> Option<Rect> {
    match clip {
        Some(clip) => rect.intersection(&clip),
        None => Some(rect),
    }
}

fn push_box(
    primitives: &mut Vec<Primitive>,
    rect: Rect,
    visual: &VisualStyle,
    clip: Option<Rect>,
    opacity: f32,
    spin: Spin,
) {
    if let Some(shape) = shadow_shape(rect, visual.shadow) {
        primitives.push(Primitive::Shadow(ShadowPrimitive {
            rect: shape,
            corner_radius: visual.corner_radius,
            blur: visual.shadow.blur.max(0.0),
            color: fade_color(visual.shadow.color, opacity),
            clip,
            spin,
        }));
    }
    let background = fade_background(visual.background, opacity);
    let border_color = fade_color(visual.border_color, opacity);
    let has_border = visual.border_width > 0.0 && !border_color.is_transparent();
    if !background.is_transparent() || has_border {
        primitives.push(Primitive::Quad(QuadPrimitive {
            rect,
            background,
            border_color,
            border_width: visual.border_width,
            corner_radius: visual.corner_radius,
            clip,
            spin,
        }));
    }
}

fn push_thumb(primitives: &mut Vec<Primitive>, rect: Rect, clip: Option<Rect>, opacity: f32) {
    primitives.push(Primitive::Quad(QuadPrimitive {
        rect,
        background: Background::Solid(fade_color(scroll::THUMB_COLOR, opacity)),
        border_color: Color::TRANSPARENT,
        border_width: 0.0,
        corner_radius: 2.0,
        clip,
        spin: Spin::NONE,
    }));
}

fn fade_color(color: Color, opacity: f32) -> Color {
    Color {
        a: color.a * opacity,
        ..color
    }
}

fn fade_background(background: Background, opacity: f32) -> Background {
    match background {
        Background::Solid(color) => Background::Solid(fade_color(color, opacity)),
        Background::Linear {
            from,
            to,
            start,
            end,
        } => Background::Linear {
            from: fade_color(from, opacity),
            to: fade_color(to, opacity),
            start,
            end,
        },
    }
}

impl Ui {
    /// Installs an image whose path or URL finished loading since the last frame.
    pub(super) fn poll_image_loads(&mut self) {
        let mut finished = Vec::new();
        for index in 0..self.nodes.len() {
            let super::NodeContent::Image {
                load: Some(load), ..
            } = &self.nodes[index].content
            else {
                continue;
            };
            if let Some(result) = load.take() {
                finished.push((index, result));
            }
        }
        for (index, result) in finished {
            match result {
                Ok(clip) => {
                    let pixels = clip.frame_pixels(0);
                    if let super::NodeContent::Image {
                        clip: slot,
                        pixels: shown,
                        elapsed,
                        clock,
                        shown: frame,
                        load,
                        ..
                    } = &mut self.nodes[index].content
                    {
                        *slot = clip;
                        *shown = pixels;
                        *elapsed = std::time::Duration::ZERO;
                        *clock = None;
                        *frame = 0;
                        *load = None;
                    }
                    self.nodes[index].paint_dirty = true;
                    self.paint_dirty = true;
                }
                Err(error) => {
                    log::warn!("{error}");
                    if let super::NodeContent::Image { load, .. } = &mut self.nodes[index].content {
                        *load = None;
                    }
                }
            }
        }
    }

    /// Advances visible GIF clips and damages a node when its frame changes.
    pub(super) fn step_images(&mut self, now: crate::time::Instant) {
        let mut changed = Vec::new();
        for index in 0..self.nodes.len() {
            if !self.nodes[index].alive {
                continue;
            }
            let visible = self.image_is_visible(index);
            let super::NodeContent::Image {
                clip,
                pixels,
                elapsed,
                clock,
                shown,
                ..
            } = &mut self.nodes[index].content
            else {
                continue;
            };
            if !clip.is_animated() {
                continue;
            }
            if !visible {
                *clock = None;
                continue;
            }
            if let Some(previous) = *clock {
                *elapsed += now.saturating_duration_since(previous);
            }
            *clock = Some(now);
            let next = clip.index_at(*elapsed);
            if next != *shown {
                *shown = next;
                *pixels = clip.frame_pixels(next);
                changed.push(index);
            }
        }
        for index in changed {
            self.nodes[index].paint_dirty = true;
            self.paint_dirty = true;
        }
    }

    /// When the next visible GIF frame is due.
    pub(super) fn image_frame_deadline(
        &self,
        now: crate::time::Instant,
    ) -> Option<crate::time::Instant> {
        let mut soonest: Option<crate::time::Instant> = self
            .nodes
            .iter()
            .any(|node| {
                node.alive
                    && matches!(
                        node.content,
                        super::NodeContent::Image { load: Some(_), .. }
                    )
            })
            .then_some(now + std::time::Duration::from_millis(16));
        for (index, node) in self.nodes.iter().enumerate() {
            let super::NodeContent::Image { clip, elapsed, .. } = &node.content else {
                continue;
            };
            if !clip.is_animated() || !self.image_is_visible(index) {
                continue;
            }
            let Some(remaining) = clip.remaining(*elapsed) else {
                continue;
            };
            let due = now + remaining;
            soonest = Some(soonest.map_or(due, |current| current.min(due)));
        }
        soonest
    }

    fn image_is_visible(&self, index: usize) -> bool {
        let node = &self.nodes[index];
        node.alive && !node.suppressed && node.style.layout.display != Display::None
    }
}

fn text_leaf(text: &str, style: &TextStyle) -> TextLeaf {
    TextLeaf {
        text: text.to_owned(),
        font_size: style.font_size,
        line_height: style.line_height_px(),
        weight: style.weight.0,
    }
}
